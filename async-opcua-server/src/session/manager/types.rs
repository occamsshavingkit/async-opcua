use std::{sync::Arc, time::Instant};

use parking_lot::RwLock;
use tracing::{debug, error};

use opcua_core::{comms::secure_channel::SecureChannel, trace_read_lock};
use opcua_crypto::{random, CertificateStore, SecurityPolicy, X509};
use opcua_types::{
    ByteString, CreateSessionRequest, CreateSessionResponse, EndpointDescription, NodeId,
    ResponseHeader, SignatureData, StatusCode, UAString,
};

use crate::{identity_token::IdentityToken, info::ServerInfo, session::instance::Session};

#[derive(Debug, Clone)]
pub(super) struct SessionExpiryEntry {
    pub(super) deadline: Instant,
    pub(super) session_id: NodeId,
}

impl Eq for SessionExpiryEntry {}

impl PartialEq for SessionExpiryEntry {
    fn eq(&self, other: &Self) -> bool {
        self.deadline == other.deadline && self.session_id == other.session_id
    }
}

impl PartialOrd for SessionExpiryEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SessionExpiryEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.deadline.cmp(&other.deadline)
    }
}

#[derive(Debug, Clone)]
pub(super) struct UnactivatedSessionEntry {
    pub(super) created_at: Instant,
    pub(super) session_id: NodeId,
}

impl Eq for UnactivatedSessionEntry {}

impl PartialEq for UnactivatedSessionEntry {
    fn eq(&self, other: &Self) -> bool {
        self.created_at == other.created_at && self.session_id == other.session_id
    }
}

impl PartialOrd for UnactivatedSessionEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for UnactivatedSessionEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.created_at.cmp(&other.created_at)
    }
}

pub(super) struct CreateSessionEndpointSelection {
    pub(super) server_endpoints: Vec<EndpointDescription>,
}

impl CreateSessionEndpointSelection {
    pub(super) fn preflight(
        info: &ServerInfo,
        request: &CreateSessionRequest,
    ) -> Result<Self, StatusCode> {
        // OPC-10000-4 5.7.2: CreateSession endpoint validation is safe to
        // prepare before the short manager commit as long as statuses remain unchanged.
        let endpoints = info.new_endpoint_descriptions(request.endpoint_url.as_ref());
        if request.endpoint_url.is_empty() {
            error!("Create session was passed an null endpoint url");
            return Err(StatusCode::BadTcpEndpointUrlInvalid);
        }

        let Some(server_endpoints) = endpoints else {
            return Err(StatusCode::BadTcpEndpointUrlInvalid);
        };

        info.validate_endpoint_hostname(request.endpoint_url.as_ref())?;

        Ok(Self { server_endpoints })
    }
}

pub(super) struct CreateSessionCertificateValidation {
    pub(super) client_certificate: Option<X509>,
}

impl CreateSessionCertificateValidation {
    pub(super) fn preflight(
        certificate_store: &RwLock<CertificateStore>,
        security_policy: SecurityPolicy,
        request: &CreateSessionRequest,
    ) -> Result<Self, StatusCode> {
        // OPC-10000-4 5.7.2: secured CreateSession requests bind the client
        // application certificate to the requested ApplicationDescription.
        let client_certificate = if security_policy != SecurityPolicy::None {
            let cert = X509::from_byte_string(&request.client_certificate)?;
            let application_uri = if request.client_description.application_uri.is_empty() {
                None
            } else {
                Some(request.client_description.application_uri.as_ref())
            };
            let store = trace_read_lock!(certificate_store);
            store.validate_or_reject_application_instance_cert(
                &cert,
                security_policy,
                None,
                application_uri,
            )?;
            Some(cert)
        } else {
            None
        };

        Ok(Self { client_certificate })
    }
}

pub(super) struct CreateSessionServerSignature {
    pub(super) server_signature: SignatureData,
    #[cfg(feature = "ecc")]
    pub(super) issued_ecdh_key: Option<(opcua_crypto::ecc::EphemeralKeyPair, SecurityPolicy)>,
    #[cfg(feature = "ecc")]
    pub(super) ecdh_response_header: Option<opcua_types::ExtensionObject>,
}

impl CreateSessionServerSignature {
    pub(super) async fn preflight(
        info: &ServerInfo,
        security_policy: SecurityPolicy,
        request: &CreateSessionRequest,
    ) -> Result<Self, StatusCode> {
        // OPC-10000-4 5.7.2: the server signature proves possession of the
        // server private key for the client certificate and nonce supplied in
        // CreateSession, and can be prepared before the short manager commit.
        //
        // T007: clone the key out of the read guard so no parking_lot guard
        // crosses the spawn_blocking await boundary (R6/G1). The cloned key
        // is also reused for the offloaded ECC keygen below (T008).
        let server_pkey = info.server_pkey.read().clone();

        // T007/T010A: offload the RSA server-signature signing onto the
        // dedicated crypto executor (falls back to spawn_blocking if no
        // executor is configured). The ECC ephemeral keygen (T008) is
        // offloaded below.
        let server_signature = if let Some(ref pkey) = server_pkey {
            let signing_key = pkey.clone();
            let client_certificate = request.client_certificate.clone();
            let client_nonce = request.client_nonce.clone();
            let executor = info
                .crypto_executor
                .as_ref()
                .map(|e| e.clone() as Arc<dyn opcua_core::comms::crypto_offload::CryptoOffload>);
            match opcua_core::comms::crypto_offload::execute_offloaded(
                executor.as_deref(),
                move || {
                    opcua_crypto::create_signature_data(
                        &signing_key,
                        security_policy,
                        &client_certificate,
                        &client_nonce,
                    )
                },
            )
            .await
            {
                Ok(Ok(signature)) => signature,
                Ok(Err(err)) => {
                    error!(
                        "Cannot create signature data from private key, check log and error {:?}",
                        err
                    );
                    if security_policy != SecurityPolicy::None {
                        return Err(StatusCode::BadSecurityChecksFailed);
                    }
                    SignatureData::null()
                }
                // Offload failure (worker panic/cancellation, executor closed):
                // distinct from an inner crypto error (C2/R6). Map to a
                // generic internal error without masking the specific crypto
                // status codes handled above.
                Err(_offload_err) => {
                    error!("CreateSession server-signature crypto executor task failed");
                    return Err(StatusCode::BadInternalError);
                }
            }
        } else {
            SignatureData::null()
        };

        #[cfg(feature = "ecc")]
        let mut issued_ecdh_key: Option<(
            opcua_crypto::ecc::EphemeralKeyPair,
            SecurityPolicy,
        )> = None;
        #[cfg(feature = "ecc")]
        let ecdh_response_header = match opcua_crypto::ecc::read_ecdh_policy_uri(
            &request.request_header.additional_header,
        ) {
            Some(uri) => {
                let policy = SecurityPolicy::from_uri(&uri);
                // T008/T010A: offload ECC ephemeral keygen onto the
                // dedicated crypto executor. server_pkey was cloned out of
                // the read guard in T007; move it here (last use) so no
                // guard crosses the await.
                let ecc_executor = info.crypto_executor.as_ref().map(|e| {
                    e.clone() as Arc<dyn opcua_core::comms::crypto_offload::CryptoOffload>
                });
                match issue_server_ephemeral_key_blocking(uri, server_pkey, ecc_executor).await {
                    EcdhKeygenOutcome::Issued {
                        keypair,
                        ephemeral_key,
                    } => {
                        issued_ecdh_key = Some((keypair, policy));
                        Some(opcua_crypto::ecc::build_ecdh_key_response(ephemeral_key))
                    }
                    EcdhKeygenOutcome::Error { header } => Some(header),
                }
            }
            None => None,
        };

        Ok(Self {
            server_signature,
            #[cfg(feature = "ecc")]
            issued_ecdh_key,
            #[cfg(feature = "ecc")]
            ecdh_response_header,
        })
    }
}

/// Outcome of offloading ECC ephemeral key generation onto the blocking pool
/// (T008, OPC-10000-6 §6.8.2).
#[cfg(feature = "ecc")]
pub(super) enum EcdhKeygenOutcome {
    /// Key pair generated successfully; caller stores the keypair and builds
    /// the response header from `ephemeral_key`.
    Issued {
        keypair: opcua_crypto::ecc::EphemeralKeyPair,
        ephemeral_key: opcua_types::EphemeralKeyType,
    },
    /// A pre-built response header carrying the error status (inner crypto
    /// failure, missing server key, or blocking-task join failure).
    Error {
        header: opcua_types::ExtensionObject,
    },
}

/// Offload `opcua_crypto::ecc::issue_server_ephemeral_key` onto the
/// dedicated crypto executor (or spawn_blocking fallback) so ECC ephemeral
/// keygen does not occupy a request-processing thread (T008/T010A,
/// OPC-10000-6 §6.8.2 / OPC-10000-4 §5.7.2).
///
/// `server_pkey` must already be cloned out of its read guard — no lock guard
/// crosses the `.await` boundary (R6/G1). A `None` key preserves the existing
/// `BadSecurityPolicyRejected` response. An offload failure produces a
/// distinct `BadInternalError` without masking specific crypto status codes
/// (C2/R6).
#[cfg(feature = "ecc")]
pub(super) async fn issue_server_ephemeral_key_blocking(
    policy_uri: String,
    server_pkey: Option<opcua_crypto::PrivateKey>,
    executor: Option<Arc<dyn opcua_core::comms::crypto_offload::CryptoOffload>>,
) -> EcdhKeygenOutcome {
    let Some(key) = server_pkey else {
        return EcdhKeygenOutcome::Error {
            header: opcua_crypto::ecc::build_ecdh_key_error(StatusCode::BadSecurityPolicyRejected),
        };
    };
    match opcua_core::comms::crypto_offload::execute_offloaded(executor.as_deref(), move || {
        opcua_crypto::ecc::issue_server_ephemeral_key(&policy_uri, &key)
    })
    .await
    {
        Ok(Ok((keypair, ephemeral_key))) => EcdhKeygenOutcome::Issued {
            keypair,
            ephemeral_key,
        },
        Ok(Err(e)) => EcdhKeygenOutcome::Error {
            header: opcua_crypto::ecc::build_ecdh_key_error(e.status()),
        },
        Err(_offload_err) => {
            error!("ECC ephemeral key crypto executor task failed");
            EcdhKeygenOutcome::Error {
                header: opcua_crypto::ecc::build_ecdh_key_error(StatusCode::BadInternalError),
            }
        }
    }
}

pub(crate) struct CreateSessionDraft {
    pub(crate) secure_channel_id: u32,
    pub(crate) actor_construction: CreateSessionActorConstruction,
    pub(crate) session_allocation: CreateSessionAllocation,
}

pub(crate) struct CreateSessionActorConstruction {
    pub(crate) authentication_token: NodeId,
    pub(crate) server_nonce: ByteString,
    pub(crate) server_certificate: ByteString,
    pub(crate) session_timeout: u64,
    pub(crate) max_request_message_size: u32,
    pub(crate) server_endpoints: Option<Vec<EndpointDescription>>,
    pub(crate) session_id: NodeId,
    pub(crate) session_id_numeric: u32,
}

pub(crate) struct CreateSessionAllocation {
    pub(crate) session_arc: Arc<RwLock<Session>>,
    pub(crate) response: CreateSessionResponse,
}

impl CreateSessionActorConstruction {
    pub(super) fn prepare(
        info: &ServerInfo,
        channel: &SecureChannel,
        request: &CreateSessionRequest,
        endpoint_selection: &CreateSessionEndpointSelection,
        certificate_validation: &CreateSessionCertificateValidation,
        server_signature: &mut CreateSessionServerSignature,
    ) -> Result<(Self, Session), StatusCode> {
        // OPC-10000-4 5.7.2: these values are part of the session returned by
        // CreateSession, but preparing them does not publish the Session or
        // spawn its actor.
        let nonce_len = info.config.session_nonce_length;
        if !(32..=128).contains(&nonce_len) {
            return Err(StatusCode::BadConfigurationError);
        }
        let authentication_token = NodeId::new(0, random::byte_string(32));
        let server_nonce = random::byte_string(nonce_len);
        let server_certificate = {
            let certs = info.endpoint_certificates.read();
            certs
                .values()
                .find_map(|v| v.as_ref().map(|(cert, _)| cert.as_byte_string()))
                .unwrap_or_default()
        };
        let session_timeout = info
            .config
            .max_session_timeout_ms
            .min(request.requested_session_timeout.floor() as u64);
        let max_request_message_size = info.config.limits.max_message_size as u32;
        let server_endpoints = Some(endpoint_selection.server_endpoints.clone());
        let security_policy = channel.security_policy();

        let session_name = {
            let name = request.session_name.clone();
            if name.is_empty() {
                UAString::from("UnnamedSession")
            } else {
                name
            }
        };
        let session = Session::create(
            info,
            authentication_token.clone(),
            channel.secure_channel_id(),
            session_timeout,
            max_request_message_size,
            request.max_response_message_size,
            request.endpoint_url.clone(),
            security_policy.to_uri().to_string(),
            IdentityToken::None,
            certificate_validation.client_certificate.clone(),
            server_nonce.clone(),
            session_name,
            request.client_description.clone(),
            channel.security_mode(),
        );

        #[cfg(feature = "ecc")]
        let session = {
            let mut session = session;
            if let Some((keypair, policy)) = server_signature.issued_ecdh_key.take() {
                session.set_ecdh_ephemeral_key(keypair, policy);
            }
            session
        };
        #[cfg(not(feature = "ecc"))]
        let _ = server_signature;

        let session_id = session.session_id().clone();
        let session_id_numeric = session.session_id_numeric();

        Ok((
            Self {
                authentication_token,
                server_nonce,
                server_certificate,
                session_timeout,
                max_request_message_size,
                server_endpoints,
                session_id,
                session_id_numeric,
            },
            session,
        ))
    }
}

impl CreateSessionAllocation {
    pub(super) fn prepare(
        session: Session,
        request: &CreateSessionRequest,
        actor_construction: &CreateSessionActorConstruction,
        server_signature: &CreateSessionServerSignature,
    ) -> Self {
        // OPC-10000-4 5.7.2: allocation can prepare the publishable session
        // handle and response body without registering the Session globally.
        let session_arc = Arc::new(RwLock::new(session));
        let response = CreateSessionResponse {
            response_header: ResponseHeader::new_good(&request.request_header),
            session_id: actor_construction.session_id.clone(),
            authentication_token: actor_construction.authentication_token.clone(),
            revised_session_timeout: actor_construction.session_timeout as f64,
            server_nonce: actor_construction.server_nonce.clone(),
            server_certificate: actor_construction.server_certificate.clone(),
            server_endpoints: actor_construction.server_endpoints.clone(),
            server_software_certificates: None,
            server_signature: server_signature.server_signature.clone(),
            max_request_message_size: actor_construction.max_request_message_size,
        };
        #[cfg(feature = "ecc")]
        let response = {
            let mut response = response;
            if let Some(header) = server_signature.ecdh_response_header.clone() {
                response.response_header.additional_header = header;
            }
            response
        };

        Self {
            session_arc,
            response,
        }
    }
}

impl CreateSessionDraft {
    pub(crate) async fn prepare_endpoint_preflight(
        info: &ServerInfo,
        channel: &SecureChannel,
        certificate_store: &RwLock<CertificateStore>,
        request: &CreateSessionRequest,
    ) -> Result<Self, StatusCode> {
        let endpoint_selection = CreateSessionEndpointSelection::preflight(info, request)?;
        if !request.request_header.authentication_token.is_null() {
            debug!("CreateSession received non-null authenticationToken; ignoring per spec");
        }
        let security_policy = channel.security_policy();
        if !matches!(security_policy, SecurityPolicy::None) {
            let min_nonce_len = std::cmp::max(info.config.session_nonce_length, 32usize);
            let max_nonce_len = 128usize;
            if request.client_nonce.len() < min_nonce_len
                || request.client_nonce.len() > max_nonce_len
            {
                error!(
                    "Create session was passed a client nonce of invalid length {} (allowed range [{}, {}])",
                    request.client_nonce.len(), min_nonce_len, max_nonce_len,
                );
                return Err(StatusCode::BadNonceInvalid);
            }
        }
        let certificate_validation = CreateSessionCertificateValidation::preflight(
            certificate_store,
            security_policy,
            request,
        )?;
        let server_signature =
            CreateSessionServerSignature::preflight(info, security_policy, request).await?;
        let mut server_signature = server_signature;
        let (actor_construction, session) = CreateSessionActorConstruction::prepare(
            info,
            channel,
            request,
            &endpoint_selection,
            &certificate_validation,
            &mut server_signature,
        )?;
        let session_allocation = CreateSessionAllocation::prepare(
            session,
            request,
            &actor_construction,
            &server_signature,
        );

        Ok(Self {
            secure_channel_id: channel.secure_channel_id(),
            actor_construction,
            session_allocation,
        })
    }
}
