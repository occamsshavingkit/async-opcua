use std::cmp::Reverse;
use std::sync::{atomic::Ordering, Arc};
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use tracing::{error, info};

#[cfg(feature = "subscriptions")]
use crate::subscriptions::SubscriptionCache;
use opcua_core::{comms::secure_channel::SecureChannel, trace_read_lock, trace_write_lock};
use opcua_crypto::{SecurityPolicy, X509};

use opcua_types::{
    ActivateSessionRequest, ActivateSessionResponse, ByteString, CloseSessionRequest,
    CloseSessionResponse, CreateSessionResponse, Error, MessageSecurityMode, NodeId,
    ResponseHeader, SignatureData, StatusCode,
};

use crate::{
    identity_token::IdentityToken,
    info::ServerInfo,
    node_manager::NodeManagers,
    rbac::resolver::ResolvedIdentity,
    session::{actor::SessionMessage, instance::Session, message_handler::MessageHandler},
};

use super::{
    audit, clear_session_locale_ids, clear_session_locale_ids_for_node_id, set_session_locale_ids,
    types::{
        CreateSessionActorConstruction, CreateSessionAllocation, CreateSessionDraft,
        SessionExpiryEntry, UnactivatedSessionEntry,
    },
    SessionManager,
};

#[cfg(feature = "ecc")]
use super::types::{issue_server_ephemeral_key_blocking, EcdhKeygenOutcome};

impl SessionManager {
    pub(crate) fn commit_create_session_draft(
        &mut self,
        draft: CreateSessionDraft,
        channel: &mut SecureChannel,
        node_managers: NodeManagers,
        #[cfg(feature = "subscriptions")] subscriptions: Arc<SubscriptionCache>,
    ) -> Result<CreateSessionResponse, StatusCode> {
        // OPC-10000-4 5.7.2: CreateSession publishes a Session and its
        // authentication token, so the global session limit must be checked
        // immediately before those identifiers become visible.
        if self.sessions.len() >= self.info.config.limits.max_sessions {
            let eviction_candidate: Option<NodeId> = {
                let mut heap = self.unactivated_heap.lock();
                let mut oldest = None;
                loop {
                    let next = heap.pop();
                    let Some(Reverse(UnactivatedSessionEntry { session_id, .. })) = next else {
                        break;
                    };
                    if !self.sessions.contains_key(&session_id) {
                        continue;
                    }
                    let session_arc = self.sessions.get(&session_id).unwrap();
                    let Some(session) = session_arc.try_read() else {
                        continue;
                    };
                    if session.is_activated() {
                        continue;
                    }
                    oldest = Some(session_id);
                    break;
                }
                oldest
            };
            if let Some(evicted_id) = eviction_candidate {
                if let Some(evicted_arc) = self.sessions.remove(&evicted_id) {
                    let (auth_token, channel_id) = {
                        let arc = evicted_arc.clone();
                        let mut evicted = trace_write_lock!(arc);
                        let auth_token = evicted.authentication_token.clone();
                        let channel_id = evicted.secure_channel_id();
                        evicted.close();
                        (auth_token, channel_id)
                    };
                    self.auth_tokens.remove(&auth_token);
                    self.actor_senders.remove(&auth_token);
                    self.closed_auth_tokens.insert(auth_token, Instant::now());
                    clear_session_locale_ids_for_node_id(&self.info, &evicted_id);
                    if let Some(counter) = self.unactivated_by_channel.get(&channel_id) {
                        counter.fetch_sub(1, Ordering::Release);
                    }
                }
            } else {
                return Err(StatusCode::BadTooManySessions);
            }
        }
        let unactivated_count = self
            .unactivated_by_channel
            .entry(draft.secure_channel_id)
            .or_default()
            .load(Ordering::Acquire);
        if unactivated_count >= self.info.config.limits.max_unactivated_sessions_per_channel {
            return Err(StatusCode::BadTooManySessions);
        }
        if channel.secure_channel_id() != draft.secure_channel_id {
            // CreateSession binds the new Session to the SecureChannel that
            // carried the request; a stale draft must not publish on another channel.
            return Err(StatusCode::BadSecureChannelIdInvalid);
        }

        let CreateSessionDraft {
            actor_construction,
            session_allocation,
            ..
        } = draft;
        let CreateSessionActorConstruction {
            authentication_token,
            session_id,
            session_id_numeric,
            ..
        } = actor_construction;
        let CreateSessionAllocation {
            session_arc,
            response,
        } = session_allocation;

        info!("Created new session with ID {}", session_id);
        self.sessions.insert(session_id, Arc::clone(&session_arc));
        self.unactivated_by_channel
            .entry(draft.secure_channel_id)
            .or_default()
            .fetch_add(1, Ordering::Release);
        self.register_token(authentication_token.clone(), Arc::clone(&session_arc));
        {
            let session = session_arc.read();
            let deadline = session.deadline();
            if !session.is_activated() {
                self.unactivated_heap
                    .lock()
                    .push(Reverse(UnactivatedSessionEntry {
                        created_at: session.created_at(),
                        session_id: session.session_id().clone(),
                    }));
                let unactivated_deadline = session.created_at()
                    + Duration::from_millis(self.info.config.limits.unactivated_session_timeout_ms);
                self.expiry_heap.lock().push(Reverse(SessionExpiryEntry {
                    deadline: deadline.min(unactivated_deadline),
                    session_id: session.session_id().clone(),
                }));
            } else {
                self.expiry_heap.lock().push(Reverse(SessionExpiryEntry {
                    deadline,
                    session_id: session.session_id().clone(),
                }));
            }
        }
        self.spawn_session_actor(
            authentication_token,
            session_arc,
            session_id_numeric,
            node_managers,
            #[cfg(feature = "subscriptions")]
            subscriptions,
        );
        self.refresh_client_response_body_limit_for_channel(channel);

        #[cfg(feature = "diagnostics")]
        {
            self.info
                .diagnostics
                .set_current_session_count(self.sessions.len() as u32);
            self.info.diagnostics.inc_session_count();
        }

        self.notify.notify_waiters();

        Ok(response)
    }

    #[allow(dead_code)]
    pub(super) fn verify_client_signature(
        security_policy: SecurityPolicy,
        info: &ServerInfo,
        session: &Session,
        client_signature: &SignatureData,
    ) -> Result<(), Error> {
        if let Some(client_certificate) = session.client_certificate() {
            let server_cert = {
                let certs = info.endpoint_certificates.read();
                certs
                    .values()
                    .find_map(|v| v.as_ref().map(|(cert, _)| cert.clone()))
            };
            if let Some(ref server_certificate) = server_cert {
                opcua_crypto::verify_signature_data(
                    client_signature,
                    security_policy,
                    client_certificate,
                    server_certificate,
                    session.session_nonce().as_ref(),
                )?;
                Ok(())
            } else {
                Err(Error::new(
                    StatusCode::BadUnexpectedError,
                    "Client signature verification failed, server has no server certificate",
                ))
            }
        } else {
            Err(Error::new(
                StatusCode::BadUnexpectedError,
                "Client signature verification failed, session has no client certificate",
            ))
        }
    }
}

// This is a non-self method to avoid holding the manager
// across an await point.
#[cfg_attr(not(feature = "subscriptions"), allow(unused_variables))]
pub(crate) async fn close_session(
    mgr_lck: &RwLock<SessionManager>,
    channel: &mut SecureChannel,
    handler: &mut MessageHandler,
    request: &CloseSessionRequest,
) -> Result<CloseSessionResponse, StatusCode> {
    let (session, id, token, actor_sender, was_unactivated, channel_id_for_counter) = {
        let mgr = trace_read_lock!(mgr_lck);
        let Some(session) = mgr.find_by_token(&request.request_header.authentication_token) else {
            return Err(StatusCode::BadSessionIdInvalid);
        };
        let (id, token, authentication_token, was_unactivated, channel_id_for_counter) = {
            let session = trace_read_lock!(session);
            let id = session.session_id_numeric();
            let token = session.user_token().cloned();
            let authentication_token = session.authentication_token.clone();
            let was_unactivated = !session.is_activated();
            let channel_id_for_counter = session.secure_channel_id();

            let secure_channel_id = channel.secure_channel_id();
            if was_unactivated && channel_id_for_counter != secure_channel_id {
                error!(
                    "close_session rejected, secure channel id {} for inactive session does not match one used to create session, {}",
                    secure_channel_id,
                    channel_id_for_counter
                );
                return Err(StatusCode::BadSecureChannelIdInvalid);
            }
            (
                id,
                token,
                authentication_token,
                was_unactivated,
                channel_id_for_counter,
            )
        };

        let Some(actor_sender) = mgr.actor_sender(&authentication_token) else {
            return Err(StatusCode::BadSessionClosed);
        };

        (
            session,
            id,
            token,
            actor_sender,
            was_unactivated,
            channel_id_for_counter,
        )
    };

    let (acknowledge, acknowledged) = tokio::sync::oneshot::channel();
    actor_sender
        .send(SessionMessage::Terminate {
            reason: StatusCode::Good,
            acknowledge,
        })
        .await
        .map_err(|_| StatusCode::BadSessionClosed)?;

    let terminated = acknowledged
        .await
        .map_err(|_| StatusCode::BadSessionClosed)?;
    {
        let mut mgr = trace_write_lock!(mgr_lck);
        if was_unactivated {
            if let Some(counter) = mgr.unactivated_by_channel.get(&channel_id_for_counter) {
                counter.fetch_sub(1, Ordering::Release);
            }
        }
        mgr.sessions.remove(&terminated.session_id);
        clear_session_locale_ids(&mgr.info, id);
        #[cfg(feature = "diagnostics")]
        mgr.info
            .diagnostics
            .set_current_session_count(mgr.sessions.len() as u32);
        mgr.refresh_client_response_body_limit_for_channel(channel);
    }
    info!("Closed session with ID {}", terminated.session_id);

    #[cfg(feature = "subscriptions")]
    if request.delete_subscriptions {
        if let Some(token) = token {
            handler
                .delete_session_subscriptions(id, session, token)
                .await;
        }
        // The token might be None if the session was never activated. No need to delete subscriptions in that case.
    }

    Ok(CloseSessionResponse {
        response_header: ResponseHeader::new_good(&request.request_header),
    })
}

pub(crate) async fn activate_session(
    mgr_lck: &RwLock<SessionManager>,
    channel: &mut SecureChannel,
    request: &ActivateSessionRequest,
    handler: &mut MessageHandler,
) -> Result<ActivateSessionResponse, StatusCode> {
    let security_policy = channel.security_policy();
    let security_mode = channel.security_mode();
    let secure_channel_id = channel.secure_channel_id();
    let mut verify_data: Option<(Option<X509>, Option<X509>, ByteString, SignatureData)> = None;
    let (
        endpoint_url,
        session_nonce,
        previous_secure_channel_id,
        was_unactivated,
        session_lck,
        info,
    ) = {
        let mgr = trace_read_lock!(mgr_lck);
        let Some(session_lck) = mgr.find_by_token(&request.request_header.authentication_token)
        else {
            return Err(StatusCode::BadSessionIdInvalid);
        };

        let (endpoint_url, session_nonce, previous_secure_channel_id, was_unactivated) = {
            let session = trace_read_lock!(session_lck);
            session.validate_timed_out()?;

            let endpoint_url = session.endpoint_url().to_string();

            if !mgr
                .info
                .endpoint_exists(&endpoint_url, security_policy, security_mode)
            {
                error!(
                    "activate_session, Endpoint does not exist for requested url & mode {}, {:?} / {:?}",
                    endpoint_url, security_policy, security_mode
                );
                return Err(StatusCode::BadTcpEndpointUrlInvalid);
            }

            let is_cross_channel_activation = session.secure_channel_id() != secure_channel_id;
            if is_cross_channel_activation && session.message_security_mode() != security_mode {
                error!(
                    "activate_session, rejected secure channel id {} with SecurityMode {:?}; session channel {} was created with SecurityMode {:?}",
                    secure_channel_id,
                    security_mode,
                    session.secure_channel_id(),
                    session.message_security_mode()
                );
                return Err(StatusCode::BadSecureChannelIdInvalid);
            }

            if is_cross_channel_activation
                && SecurityPolicy::from_uri(session.security_policy_uri()) != security_policy
            {
                error!(
                    "activate_session, rejected secure channel id {} with SecurityPolicy {:?}; session channel {} was created with SecurityPolicy {}",
                    secure_channel_id,
                    security_policy,
                    session.secure_channel_id(),
                    session.security_policy_uri()
                );
                return Err(StatusCode::BadSecureChannelIdInvalid);
            }

            if security_policy != SecurityPolicy::None {
                verify_data = Some((
                    session.client_certificate().cloned(),
                    {
                        let certs = mgr.info.endpoint_certificates.read();
                        certs
                            .values()
                            .find_map(|v| v.as_ref().map(|(cert, _)| cert.clone()))
                    },
                    session.session_nonce().clone(),
                    request.client_signature.clone(),
                ));
            }

            let requested_identity = IdentityToken::new(request.user_identity_token.clone());
            if matches!(requested_identity, IdentityToken::X509(_))
                && request.user_token_signature.signature.is_null()
            {
                error!("activate_session rejected: X509 identity token requires a non-null user_token_signature");
                return Err(StatusCode::BadUserSignatureInvalid);
            }
            let session_identity_is_non_anonymous = !matches!(
                session.user_identity(),
                IdentityToken::Anonymous(_) | IdentityToken::None
            );
            if is_cross_channel_activation
                && security_mode == MessageSecurityMode::Sign
                && matches!(requested_identity, IdentityToken::Anonymous(_))
                && session_identity_is_non_anonymous
            {
                error!(
                    "activate_session, rejected anonymous ActivateSession over new Sign-only secure channel {} for session channel {} with non-anonymous identity",
                    secure_channel_id,
                    session.secure_channel_id()
                );
                return Err(StatusCode::BadIdentityTokenRejected);
            }
            let previous_secure_channel_id = session.secure_channel_id();
            let was_unactivated = !session.is_activated();
            (
                endpoint_url,
                session.session_nonce().clone(),
                previous_secure_channel_id,
                was_unactivated,
            )
        };
        (
            endpoint_url,
            session_nonce,
            previous_secure_channel_id,
            was_unactivated,
            session_lck,
            mgr.info.clone(),
        )
    };

    // OPC-10000-4 §5.7.3.2: the ActivateSession serverNonce shall have a length
    // between 32 and 128 bytes inclusive for every SecurityPolicy, and the Client
    // shall check the length. SecurityPolicy::None must therefore NOT reuse the
    // (null) secure-channel nonce; mirror CreateSession and draw a session-length
    // nonce. Secured policies keep their policy-sized secure-channel nonce.
    let server_nonce = match security_policy {
        SecurityPolicy::None => opcua_crypto::random::byte_string(info.config.session_nonce_length),
        _ => security_policy.random_nonce(),
    };

    if let Some((client_cert, server_cert, nonce, sig_data)) = verify_data.take() {
        let sec_policy = security_policy;
        let result = tokio::task::spawn_blocking(move || -> Result<(), StatusCode> {
            let client_cert = client_cert.ok_or(StatusCode::BadUnexpectedError)?;
            let server_cert = server_cert.ok_or(StatusCode::BadUnexpectedError)?;
            opcua_crypto::verify_signature_data(
                &sig_data,
                sec_policy,
                &client_cert,
                &server_cert,
                nonce.as_ref(),
            )
            .map_err(|_| StatusCode::BadSecurityChecksFailed)
        })
        .await
        .map_err(|_| StatusCode::BadUnexpectedError)?;
        result?;
    }

    #[cfg(feature = "ecc")]
    let ecc_ctx = {
        let session = trace_read_lock!(session_lck);
        let server_ephemeral = match session.ecdh_ephemeral_key() {
            Some((kp, _policy)) => Some(
                opcua_crypto::ecc::EphemeralPrivateKey::from_scalar_bytes(
                    kp.private_key().curve(),
                    kp.private_key().scalar(),
                )
                .map_err(|_| StatusCode::BadIdentityTokenRejected)?,
            ),
            None => None,
        };
        let client_certificate = session.client_certificate().cloned();
        crate::session::negotiate::EccSecretContext {
            server_ephemeral,
            client_certificate,
        }
    };
    #[cfg(not(feature = "ecc"))]
    let ecc_ctx = crate::session::negotiate::EccSecretContext::default();

    let authentication = match info
        .authenticate_endpoint_with_ecc_ctx(
            request,
            &endpoint_url,
            security_policy,
            security_mode,
            request.user_identity_token.clone(),
            &session_nonce,
            ecc_ctx,
        )
        .await
    {
        Ok(authentication) => {
            info.security_checks.write().record_pass(
                crate::security_checks::SecurityCheckCategory::UserAuthentication,
                "user",
            );
            authentication
        }
        Err(error) => {
            let status = error.status();
            info.security_checks.write().record_fail(
                crate::security_checks::SecurityCheckCategory::UserAuthentication,
                status,
                "user",
            );
            if let Some(certificate) = x509_user_certificate_from_request(request) {
                let session_id = {
                    let session = trace_read_lock!(session_lck);
                    Some(session.session_id().clone())
                };
                audit::dispatch_user_certificate_audit(
                    #[cfg(feature = "events")]
                    handler.subscriptions(),
                    &info,
                    &request.request_header,
                    certificate,
                    session_id,
                    status,
                );
            }
            return Err(error.status());
        }
    };
    if let Some(validation) = authentication.x509_user_certificate_validation.as_ref() {
        if !validation.suppressed_findings.is_empty() {
            let session_id = {
                let session = trace_read_lock!(session_lck);
                Some(session.session_id().clone())
            };
            for finding in &validation.suppressed_findings {
                audit::dispatch_user_certificate_audit(
                    #[cfg(feature = "events")]
                    handler.subscriptions(),
                    &info,
                    &request.request_header,
                    validation.certificate.clone(),
                    session_id.clone(),
                    finding.status,
                );
            }
        }
    }
    #[cfg(feature = "ecc")]
    let ecc_secret_consumed = matches!(
        security_policy,
        SecurityPolicy::EccNistP256 | SecurityPolicy::EccNistP384
    ) && matches!(
        IdentityToken::new(request.user_identity_token.clone()),
        IdentityToken::UserName(_) | IdentityToken::IssuedToken(_)
    );

    let (server_nonce, session_id, user_changed, user_token) = {
        let mut session = trace_write_lock!(session_lck);
        if is_cross_channel_transfer_forbidden(
            previous_secure_channel_id,
            secure_channel_id,
            !was_unactivated,
            security_policy,
        ) {
            error!(
                "activate session, rejected secure channel id {} does not match session channel {} (transfer not permitted for SecurityPolicy::None)",
                secure_channel_id, previous_secure_channel_id
            );
            return Err(StatusCode::BadSecureChannelIdInvalid);
        }

        let channel_cert = channel.remote_cert();
        if is_client_certificate_channel_mismatch(
            session.client_certificate(),
            channel_cert.as_ref(),
            security_policy,
        ) {
            error!(
                "activate session rejected: client certificate presented at CreateSession does not match the certificate securing the channel (secure channel id {})",
                secure_channel_id
            );
            let mismatch_cert = session
                .client_certificate()
                .map(|cert| cert.as_byte_string())
                .unwrap_or_else(ByteString::null);
            audit::dispatch_certificate_mismatch(
                #[cfg(feature = "events")]
                handler.subscriptions(),
                &info,
                &request.request_header,
                Some(session.session_id().clone()),
                mismatch_cert,
            );
            return Err(StatusCode::BadSecurityChecksFailed);
        }

        if session.session_nonce() != &session_nonce {
            return Err(StatusCode::BadNonceInvalid);
        }

        let user_changed = session
            .user_token()
            .is_some_and(|previous| previous != &authentication.user_token);
        let crate::info::EndpointAuthentication {
            user_token, claims, ..
        } = authentication;
        let activated_identity = IdentityToken::new(request.user_identity_token.clone());
        let application_uri =
            non_empty_ua_string(&session.application_description().application_uri);
        let resolved_identity = resolved_identity_from_activation(
            &activated_identity,
            claims.as_ref(),
            application_uri,
            Some(endpoint_url.clone()),
        )?;
        let roles = Arc::new(info.role_resolver.read().resolve(&resolved_identity));
        #[cfg(feature = "rbac")]
        info.security_checks.write().record_pass(
            crate::security_checks::SecurityCheckCategory::RbacDecision,
            "session-activation",
        );
        let locale_ids = match request.locale_ids {
            Some(ref ids) if !ids.is_empty() => request.locale_ids.clone(),
            _ => session.locale_ids().clone(),
        };
        session.activate(
            secure_channel_id,
            server_nonce,
            activated_identity,
            locale_ids.clone(),
            user_token.clone(),
            claims,
            roles,
        );
        set_session_locale_ids(&info, session.session_id_numeric(), &locale_ids);
        (
            session.session_nonce().clone(),
            session.session_id_numeric(),
            user_changed,
            user_token,
        )
    };

    if was_unactivated {
        let mgr = trace_read_lock!(mgr_lck);
        if let Some(counter) = mgr.unactivated_by_channel.get(&previous_secure_channel_id) {
            counter.fetch_sub(1, Ordering::Release);
        }
        let session = trace_read_lock!(session_lck);
        mgr.expiry_heap.lock().push(Reverse(SessionExpiryEntry {
            deadline: session.deadline(),
            session_id: session.session_id().clone(),
        }));
    }

    {
        let mgr = trace_read_lock!(mgr_lck);
        mgr.refresh_client_response_body_limit_for_channel(channel);
    }

    #[cfg(feature = "ecc")]
    let ecdh_response_header = {
        use opcua_crypto::ecc::EcdhKeyAction;

        // Phase 1 — acquire the session write lock only long enough to read
        // state, mark the key consumed if applicable, and decide the action.
        // Drop before any await (T008: no session guard crosses .await).
        let ecdh_action = {
            let mut session = trace_write_lock!(session_lck);
            let requested_uri =
                opcua_crypto::ecc::read_ecdh_policy_uri(&request.request_header.additional_header);
            let previous_policy = session.ecdh_ephemeral_key().map(|(_, policy)| *policy);
            if ecc_secret_consumed {
                session.mark_ecdh_key_consumed();
            }
            let previous_key_consumed = session.ecdh_key_consumed();
            opcua_crypto::ecc::decide_ecdh_key_action(
                requested_uri.as_deref(),
                previous_policy,
                previous_key_consumed,
            )
        };

        // Phase 2 — execute the action without holding the session lock.
        match ecdh_action {
            EcdhKeyAction::Issue(policy) => {
                // Clone the key out of the read guard so no guard crosses the
                // await boundary (T008/R6).
                let server_pkey = info.server_pkey.read().clone();
                match issue_server_ephemeral_key_blocking(
                    policy.to_uri().to_owned(),
                    server_pkey,
                    info.crypto_executor.as_ref().map(|e| {
                        e.clone() as Arc<dyn opcua_core::comms::crypto_offload::CryptoOffload>
                    }),
                )
                .await
                {
                    EcdhKeygenOutcome::Issued {
                        keypair,
                        ephemeral_key,
                    } => {
                        // Reacquire the write lock only to store the new key.
                        // On error the session state is left untouched,
                        // matching the previous inline behavior.
                        let mut session = trace_write_lock!(session_lck);
                        session.set_ecdh_ephemeral_key(keypair, policy);
                        Some(opcua_crypto::ecc::build_ecdh_key_response(ephemeral_key))
                    }
                    EcdhKeygenOutcome::Error { header } => Some(header),
                }
            }
            EcdhKeyAction::Reject => Some(opcua_crypto::ecc::build_ecdh_key_error(
                StatusCode::BadSecurityPolicyRejected,
            )),
            EcdhKeyAction::Retain | EcdhKeyAction::None => None,
        }
    };

    let namespaces =
        handler.get_namespaces_for_user(session_lck.clone(), session_id, user_token.clone());
    {
        channel.set_namespaces(namespaces);
    }

    #[cfg(feature = "subscriptions")]
    if user_changed {
        handler
            .revalidate_monitored_items_for_user(session_lck, session_id, user_token)
            .await;
    }
    #[cfg(not(feature = "subscriptions"))]
    let _ = user_changed;

    // TODO: Audit

    let response = ActivateSessionResponse {
        response_header: ResponseHeader::new_good(&request.request_header),
        server_nonce,
        results: None,
        diagnostic_infos: None,
    };
    #[cfg(feature = "ecc")]
    let response = {
        let mut response = response;
        if let Some(header) = ecdh_response_header {
            response.response_header.additional_header = header;
        }
        response
    };
    Ok(response)
}

fn x509_user_certificate_from_request(request: &ActivateSessionRequest) -> Option<ByteString> {
    match IdentityToken::new(request.user_identity_token.clone()) {
        IdentityToken::X509(token) => Some(token.certificate_data),
        _ => None,
    }
}

/// Returns true if activating a session on `request_channel_id` must be refused
/// because it differs from the channel the session belongs to and the session
/// either is not yet activated or uses SecurityPolicy::None (which has no
/// cryptographic channel binding, so cross-channel transfer would be a hijack).
pub(super) fn is_cross_channel_transfer_forbidden(
    session_channel_id: u32,
    request_channel_id: u32,
    session_activated: bool,
    security_policy: SecurityPolicy,
) -> bool {
    session_channel_id != request_channel_id
        && (!session_activated || security_policy == SecurityPolicy::None)
}

/// Returns true when the client application certificate bound to the session at CreateSession does
/// NOT match the certificate that secured the activating channel -- a Part 4 §5.6 binding violation
/// that must be rejected. For `SecurityPolicy::None` there is no channel certificate, so the binding
/// is not checked (returns false). Under any secured policy, both certificates must be present and
/// equal (by thumbprint); a missing certificate on either side is treated as a violation (fail closed).
pub(super) fn is_client_certificate_channel_mismatch(
    session_cert: Option<&X509>,
    channel_cert: Option<&X509>,
    security_policy: SecurityPolicy,
) -> bool {
    if security_policy == SecurityPolicy::None {
        return false;
    }
    match (session_cert, channel_cert) {
        (Some(session_cert), Some(channel_cert)) => {
            session_cert.thumbprint() != channel_cert.thumbprint()
        }
        _ => true,
    }
}

fn non_empty_ua_string(value: &opcua_types::UAString) -> Option<String> {
    value
        .value()
        .as_ref()
        .filter(|value| !value.is_empty())
        .cloned()
}

fn resolved_identity_from_activation(
    identity: &IdentityToken,
    claims: Option<&opcua_crypto::identity::ClaimProfile>,
    application_uri: Option<String>,
    endpoint_url: Option<String>,
) -> Result<ResolvedIdentity, StatusCode> {
    match identity {
        IdentityToken::Anonymous(_) => {
            Ok(ResolvedIdentity::anonymous(application_uri, endpoint_url))
        }
        IdentityToken::UserName(token) => Ok(ResolvedIdentity::username(
            token.user_name.as_ref(),
            application_uri,
            endpoint_url,
        )),
        IdentityToken::X509(token) => {
            let signing_cert =
                X509::from_byte_string(&token.certificate_data).map_err(|err| err.status())?;
            Ok(ResolvedIdentity::x509_thumbprint(
                signing_cert.thumbprint().as_hex_string(),
                application_uri,
                endpoint_url,
            ))
        }
        IdentityToken::IssuedToken(_) => {
            let group_ids = claims
                .map(|claims| claims.roles.clone())
                .unwrap_or_default();
            Ok(ResolvedIdentity::issued_token(
                group_ids,
                std::iter::empty::<NodeId>(),
                application_uri,
                endpoint_url,
            ))
        }
        IdentityToken::None | IdentityToken::Invalid(_) => Err(StatusCode::BadIdentityTokenInvalid),
    }
}
