#![allow(clippy::needless_borrow)]

use std::{
    fs,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};

use async_trait::async_trait;
use opcua_core::{comms::secure_channel::SecureChannel, sync::RwLock};
use opcua_crypto::{random, CertificateStore, PrivateKey, SecurityPolicy, Thumbprint};
use opcua_types::{
    ActivateSessionRequest, ActivateSessionResponse, AnonymousIdentityToken,
    ApplicationDescription, ByteString, Error, ExtensionObject, MessageSecurityMode, NodeId,
    RequestHeader, SignatureData, StatusCode, UAString, UserNameIdentityToken, UserTokenPolicy,
    UserTokenType, X509IdentityToken,
};
use tokio::sync::Notify;

use crate::{
    authenticator::{AuthManager, UserToken},
    config::{ServerEndpoint, ServerUserToken},
    identity_token::{
        IdentityToken, POLICY_ID_ANONYMOUS, POLICY_ID_USER_PASS_NONE, POLICY_ID_X509,
    },
    node_manager::NodeManagers,
    rbac::WellKnownRole,
    session::{instance::Session, message_handler::MessageHandler},
    ServerBuilder,
};

use super::lifecycle::{
    is_client_certificate_channel_mismatch, is_cross_channel_transfer_forbidden,
};
use super::{activate_session, SessionManager};

mod activation;
mod expiry;
mod lifecycle;

const X509_STATE_CLEANUP_USER_TOKEN: &str = "x509-state-cleanup-user";

pub(super) struct TempPath {
    pub(super) dir: tempfile::TempDir,
}

impl TempPath {
    pub(super) fn new(name: &str) -> Self {
        let dir = tempfile::Builder::new()
            .prefix(&format!("async-opcua-manager-{name}-"))
            .tempdir()
            .expect("test temp directory should be created");
        Self { dir }
    }

    pub(super) fn path(&self) -> &Path {
        self.dir.path()
    }
}

/// Mint a self-signed application certificate for binding tests.
pub(super) fn make_cert_and_key(common_name: &str) -> (opcua_crypto::X509, PrivateKey) {
    let data = opcua_crypto::X509Data {
        key_size: 2048,
        common_name: common_name.to_string(),
        organization: "async-opcua test".to_string(),
        organizational_unit: "test".to_string(),
        country: "IE".to_string(),
        state: "test".to_string(),
        alt_host_names: vec!["urn:async-opcua-test".to_string(), "localhost".to_string()].into(),
        certificate_duration_days: 60,
    };
    opcua_crypto::X509::cert_and_pkey(&data).expect("generate self-signed test certificate")
}

pub(super) fn make_cert(common_name: &str) -> opcua_crypto::X509 {
    make_cert_and_key(common_name).0
}

#[derive(Clone)]
pub(super) struct ActivationFixture {
    pub(super) info: Arc<crate::ServerInfo>,
    pub(super) manager: Arc<RwLock<SessionManager>>,
    pub(super) session: Arc<RwLock<Session>>,
    pub(super) token: NodeId,
    pub(super) node_managers: NodeManagers,
    pub(super) subscriptions: Arc<crate::SubscriptionCache>,
    pub(super) certificate_store: Arc<RwLock<opcua_crypto::CertificateStore>>,
    pub(super) _temp_path: Option<Arc<TempPath>>,
}

impl ActivationFixture {
    pub(super) fn new(authenticator: Arc<dyn AuthManager>) -> Self {
        let (_server, handle) = ServerBuilder::new_anonymous("activation nonce replay test")
            .without_node_managers()
            .with_authenticator(authenticator)
            .build()
            .expect("test server should build");
        Self::from_handle(handle)
    }

    pub(super) fn with_username_user(username: &str, password: &str) -> Self {
        let token_id = "runtime-role-user";
        let (_server, handle) = ServerBuilder::new()
            .without_node_managers()
            .application_name("activation runtime role resolver test")
            .add_user_token(token_id, ServerUserToken::user_pass(username, password))
            .add_endpoint(
                "none",
                (
                    "/",
                    SecurityPolicy::None,
                    MessageSecurityMode::None,
                    &[token_id] as &[&str],
                ),
            )
            .discovery_urls(vec!["/".to_owned()])
            .build()
            .expect("test server should build");
        Self::from_handle(handle)
    }

    pub(super) fn from_handle(handle: crate::ServerHandle) -> Self {
        Self::from_handle_with_session_binding(
            handle,
            SecurityPolicy::None,
            MessageSecurityMode::None,
            None,
        )
    }

    pub(super) fn with_x509_session(authenticator: Arc<dyn AuthManager>) -> Self {
        let pki = Arc::new(TempPath::new("x509-state-cleanup-pki"));
        let no_configured_user_tokens: [&str; 0] = [];
        let (_server, handle) = ServerBuilder::new()
            .without_node_managers()
            .application_name("activation x509 state cleanup test")
            .pki_dir(pki.path())
            .with_authenticator(authenticator)
            .add_endpoint(
                "x509_state_cleanup",
                (
                    "/",
                    SecurityPolicy::None,
                    MessageSecurityMode::None,
                    &no_configured_user_tokens as &[&str],
                ),
            )
            .discovery_urls(vec!["/".to_owned()])
            .build()
            .expect("test server should build");
        let (server_cert, server_key) = make_cert_and_key("x509-state-cleanup-server");
        {
            let mut certs = handle.info().endpoint_certificates.write();
            let ep = crate::config::EndpointIdentifier {
                path: "/".into(),
                security_policy: "None".into(),
                security_mode: "None".into(),
            };
            certs.insert(ep, Some((server_cert, server_key)));
        }

        let mut fixture = Self::from_handle(handle);
        fixture._temp_path = Some(pki);
        fixture
    }

    pub(super) fn with_secured_session(
        authenticator: Arc<dyn AuthManager>,
        client_certificate: opcua_crypto::X509,
    ) -> Self {
        Self::with_secured_session_created_with_mode(
            authenticator,
            client_certificate,
            MessageSecurityMode::SignAndEncrypt,
        )
    }

    pub(super) fn with_secured_session_created_with_mode(
        authenticator: Arc<dyn AuthManager>,
        client_certificate: opcua_crypto::X509,
        original_mode: MessageSecurityMode,
    ) -> Self {
        let anonymous_tokens = [crate::config::ANONYMOUS_USER_TOKEN_ID];
        let (_server, handle) = ServerBuilder::new()
            .without_node_managers()
            .application_name("activation security mode binding test")
            .with_authenticator(authenticator)
            .add_endpoint(
                "basic256sha256_sign",
                (
                    "/",
                    SecurityPolicy::Basic256Sha256,
                    MessageSecurityMode::Sign,
                    &anonymous_tokens as &[&str],
                ),
            )
            .add_endpoint(
                "basic256sha256_sign_encrypt",
                (
                    "/",
                    SecurityPolicy::Basic256Sha256,
                    MessageSecurityMode::SignAndEncrypt,
                    &anonymous_tokens as &[&str],
                ),
            )
            .add_endpoint(
                "aes128sha256rsa_oaep_sign_encrypt",
                (
                    "/",
                    SecurityPolicy::Aes128Sha256RsaOaep,
                    MessageSecurityMode::SignAndEncrypt,
                    &anonymous_tokens as &[&str],
                ),
            )
            .discovery_urls(vec!["/".to_owned()])
            .build()
            .expect("test server should build");
        let (server_cert, server_key) = make_cert_and_key("security-mode-server");
        {
            let mut certs = handle.info().endpoint_certificates.write();
            let ep = crate::config::EndpointIdentifier {
                path: "/".into(),
                security_policy: "None".into(),
                security_mode: "None".into(),
            };
            certs.insert(ep, Some((server_cert, server_key)));
        }

        Self::from_handle_with_session_binding(
            handle,
            SecurityPolicy::Basic256Sha256,
            original_mode,
            Some(client_certificate),
        )
    }

    pub(super) fn from_handle_with_session_binding(
        handle: crate::ServerHandle,
        security_policy: SecurityPolicy,
        message_security_mode: MessageSecurityMode,
        client_certificate: Option<opcua_crypto::X509>,
    ) -> Self {
        let info = Arc::clone(handle.info());
        let token = NodeId::new(1, 42);
        let endpoint_url = UAString::from(handle.info().base_endpoint());
        let session = Arc::new(RwLock::new(Session::create(
            &info,
            token.clone(),
            7,
            60_000,
            0,
            0,
            endpoint_url.clone(),
            security_policy.to_uri().to_string(),
            anonymous_identity(),
            client_certificate,
            random::byte_string(info.config.session_nonce_length),
            UAString::from("activation-nonce-replay-test"),
            ApplicationDescription::default(),
            message_security_mode,
        )));
        let manager = Arc::new(RwLock::new(SessionManager::new(
            Arc::clone(&info),
            Arc::new(Notify::new()),
        )));
        {
            let mut manager_lck = manager.write();
            manager_lck
                .sessions
                .insert(token.clone(), Arc::clone(&session));
            manager_lck.register_token(token.clone(), Arc::clone(&session));
        }

        Self {
            info,
            manager,
            session,
            token,
            node_managers: handle.node_managers().clone(),
            subscriptions: Arc::clone(handle.subscriptions()),
            certificate_store: Arc::clone(handle.certificate_store()),
            _temp_path: None,
        }
    }

    pub(super) async fn activate_with(
        &self,
        security_policy: SecurityPolicy,
        secure_channel_id: u32,
    ) -> Result<ActivateSessionResponse, StatusCode> {
        let mut channel = SecureChannel::new(
            Arc::clone(&self.certificate_store),
            opcua_core::comms::secure_channel::Role::Server,
            Arc::new(RwLock::new(Default::default())),
        );
        channel.set_security_policy(security_policy);
        channel.set_security_mode(MessageSecurityMode::None);
        channel.set_secure_channel_id(secure_channel_id);

        let request = activate_request(&self.token);
        let mut handler = MessageHandler::new(
            Arc::clone(&self.info),
            self.node_managers.clone(),
            Arc::clone(&self.subscriptions),
        );
        activate_session(&self.manager, &mut channel, &request, &mut handler).await
    }

    pub(super) async fn activate_x509_with(
        &self,
        cert: &opcua_crypto::X509,
        private_key: &PrivateKey,
    ) -> Result<ActivateSessionResponse, StatusCode> {
        let mut channel = SecureChannel::new(
            Arc::clone(&self.certificate_store),
            opcua_core::comms::secure_channel::Role::Server,
            Arc::new(RwLock::new(Default::default())),
        );
        channel.set_security_policy(SecurityPolicy::None);
        channel.set_security_mode(MessageSecurityMode::None);
        channel.set_secure_channel_id(7);

        let server_certificate = self
            .info
            .endpoint_certificates
            .read()
            .values()
            .find_map(|v| v.as_ref().map(|(cert, _)| cert))
            .expect("X.509 activation test must configure a server certificate")
            .clone();
        let request = x509_activate_request(
            &self.token,
            cert,
            private_key,
            &server_certificate,
            &self.session_nonce(),
        );
        let mut handler = MessageHandler::new(
            Arc::clone(&self.info),
            self.node_managers.clone(),
            Arc::clone(&self.subscriptions),
        );
        activate_session(&self.manager, &mut channel, &request, &mut handler).await
    }

    pub(super) fn trust_x509_user_certificate(&self, cert: &opcua_crypto::X509) {
        let store = self.certificate_store.write();
        store
            .ensure_pki_path()
            .expect("X.509 test PKI directories should exist");
        let path = store
            .trusted_certs_dir()
            .join(CertificateStore::cert_file_name(cert));
        fs::write(path, cert.to_der().expect("test certificate should encode"))
            .expect("trusted X.509 user certificate should be written");
    }

    pub(super) async fn activate_with_signed_client_proof(
        &self,
        security_policy: SecurityPolicy,
        security_mode: MessageSecurityMode,
        secure_channel_id: u32,
        client_key: &PrivateKey,
    ) -> Result<ActivateSessionResponse, StatusCode> {
        let mut channel = SecureChannel::new(
            Arc::clone(&self.certificate_store),
            opcua_core::comms::secure_channel::Role::Server,
            Arc::new(RwLock::new(Default::default())),
        );
        channel.set_security_policy(security_policy);
        channel.set_security_mode(security_mode);
        channel.set_secure_channel_id(secure_channel_id);
        channel.set_remote_cert(self.session.read().client_certificate().cloned());

        let mut request = activate_request(&self.token);
        request.client_signature = self.client_signature(client_key, security_policy);
        let mut handler = MessageHandler::new(
            Arc::clone(&self.info),
            self.node_managers.clone(),
            Arc::clone(&self.subscriptions),
        );
        activate_session(&self.manager, &mut channel, &request, &mut handler).await
    }

    pub(super) fn client_signature(
        &self,
        client_key: &PrivateKey,
        security_policy: SecurityPolicy,
    ) -> SignatureData {
        let server_certificate = self
            .info
            .endpoint_certificates
            .read()
            .values()
            .find_map(|v| v.as_ref().map(|(cert, _)| cert))
            .expect("secured activation test must configure a server certificate")
            .as_byte_string();
        opcua_crypto::create_signature_data(
            client_key,
            security_policy,
            &server_certificate,
            &self.session_nonce(),
        )
        .expect("test client signature should be valid")
    }

    pub(super) async fn activate_username_with(
        &self,
        security_policy: SecurityPolicy,
        secure_channel_id: u32,
        username: &str,
        password: &str,
    ) -> Result<ActivateSessionResponse, StatusCode> {
        let mut channel = SecureChannel::new(
            Arc::clone(&self.certificate_store),
            opcua_core::comms::secure_channel::Role::Server,
            Arc::new(RwLock::new(Default::default())),
        );
        channel.set_security_policy(security_policy);
        channel.set_security_mode(MessageSecurityMode::None);
        channel.set_secure_channel_id(secure_channel_id);

        let request = username_activate_request(&self.token, username, password);
        let mut handler = MessageHandler::new(
            Arc::clone(&self.info),
            self.node_managers.clone(),
            Arc::clone(&self.subscriptions),
        );
        activate_session(&self.manager, &mut channel, &request, &mut handler).await
    }

    pub(super) fn session_nonce(&self) -> ByteString {
        self.session.read().session_nonce().clone()
    }

    pub(super) fn secure_channel_id(&self) -> u32 {
        self.session.read().secure_channel_id()
    }

    pub(super) fn user_identity(&self) -> IdentityTokenSnapshot {
        IdentityTokenSnapshot::from(self.session.read().user_identity())
    }

    pub(super) fn mutate_session_activation(
        &self,
        secure_channel_id: u32,
        server_nonce: ByteString,
        identity: IdentityToken,
        user_token: UserToken,
    ) {
        self.session.write().activate(
            secure_channel_id,
            server_nonce,
            identity,
            None,
            user_token,
            None,
            Arc::new(vec![WellKnownRole::Anonymous.node_id()]),
        );
    }
}

pub(super) struct X509AuthenticationGate {
    pub(super) accepted_thumbprint: Thumbprint,
}

impl X509AuthenticationGate {
    pub(super) fn new(accepted_thumbprint: Thumbprint) -> Self {
        Self {
            accepted_thumbprint,
        }
    }
}

#[async_trait]
impl AuthManager for X509AuthenticationGate {
    async fn authenticate_x509_identity_token(
        &self,
        _endpoint: &ServerEndpoint,
        signing_thumbprint: &Thumbprint,
    ) -> Result<UserToken, Error> {
        if signing_thumbprint == &self.accepted_thumbprint {
            Ok(UserToken(X509_STATE_CLEANUP_USER_TOKEN.to_string()))
        } else {
            Err(Error::new(
                StatusCode::BadIdentityTokenRejected,
                "X.509 state-cleanup test rejected certificate thumbprint",
            ))
        }
    }

    fn user_token_policies(&self, _endpoint: &ServerEndpoint) -> Vec<UserTokenPolicy> {
        vec![UserTokenPolicy {
            policy_id: UAString::from(POLICY_ID_X509),
            token_type: UserTokenType::Certificate,
            issued_token_type: UAString::null(),
            issuer_endpoint_url: UAString::null(),
            security_policy_uri: UAString::from(SecurityPolicy::Basic256Sha256.to_uri()),
        }]
    }
}

pub(super) struct AuthenticationGate {
    pub(super) called: AtomicBool,
    pub(super) pause_once: AtomicBool,
    pub(super) entered: Notify,
    pub(super) release: Notify,
}

impl AuthenticationGate {
    pub(super) fn open() -> Self {
        Self {
            called: AtomicBool::new(false),
            pause_once: AtomicBool::new(false),
            entered: Notify::new(),
            release: Notify::new(),
        }
    }

    pub(super) fn pause_next_authentication(&self) {
        self.pause_once.store(true, Ordering::Release);
    }

    pub(super) async fn maybe_pause(&self) {
        if self.pause_once.swap(false, Ordering::AcqRel) {
            self.entered.notify_waiters();
            self.release.notified().await;
        }
    }

    pub(super) async fn wait_until_entered(&self) {
        if self.pause_once.load(Ordering::Acquire) {
            self.entered.notified().await;
        }
    }

    pub(super) fn release(&self) {
        self.release.notify_waiters();
    }

    pub(super) fn was_called(&self) -> bool {
        self.called.load(Ordering::Acquire)
    }
}

#[async_trait]
impl AuthManager for AuthenticationGate {
    async fn authenticate_anonymous_token(&self, _endpoint: &ServerEndpoint) -> Result<(), Error> {
        self.called.store(true, Ordering::Release);
        self.maybe_pause().await;
        Ok(())
    }

    fn user_token_policies(&self, _endpoint: &ServerEndpoint) -> Vec<UserTokenPolicy> {
        vec![UserTokenPolicy {
            policy_id: UAString::from(POLICY_ID_ANONYMOUS),
            token_type: UserTokenType::Anonymous,
            issued_token_type: UAString::null(),
            issuer_endpoint_url: UAString::null(),
            security_policy_uri: UAString::null(),
        }]
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum IdentityTokenSnapshot {
    Anonymous(UAString),
    X509(String),
    Other,
}

impl From<&IdentityToken> for IdentityTokenSnapshot {
    fn from(value: &IdentityToken) -> Self {
        match value {
            IdentityToken::Anonymous(token) => Self::Anonymous(token.policy_id.clone()),
            IdentityToken::X509(token) => {
                opcua_crypto::X509::from_byte_string(&token.certificate_data)
                    .map(|cert| Self::X509(cert.thumbprint().as_hex_string()))
                    .unwrap_or(Self::Other)
            }
            _ => Self::Other,
        }
    }
}

pub(super) fn activate_request(authentication_token: &NodeId) -> ActivateSessionRequest {
    ActivateSessionRequest {
        request_header: RequestHeader {
            authentication_token: authentication_token.clone(),
            ..Default::default()
        },
        client_signature: SignatureData::null(),
        client_software_certificates: None,
        locale_ids: None,
        user_identity_token: ExtensionObject::from_message(AnonymousIdentityToken {
            policy_id: UAString::from(POLICY_ID_ANONYMOUS),
        }),
        user_token_signature: SignatureData::null(),
    }
}

pub(super) fn username_activate_request(
    authentication_token: &NodeId,
    username: &str,
    password: &str,
) -> ActivateSessionRequest {
    ActivateSessionRequest {
        request_header: RequestHeader {
            authentication_token: authentication_token.clone(),
            ..Default::default()
        },
        client_signature: SignatureData::null(),
        client_software_certificates: None,
        locale_ids: None,
        user_identity_token: ExtensionObject::from_message(UserNameIdentityToken {
            policy_id: UAString::from(POLICY_ID_USER_PASS_NONE),
            user_name: UAString::from(username),
            password: ByteString::from(password.as_bytes()),
            encryption_algorithm: UAString::null(),
        }),
        user_token_signature: SignatureData::null(),
    }
}

pub(super) fn x509_activate_request(
    authentication_token: &NodeId,
    cert: &opcua_crypto::X509,
    private_key: &PrivateKey,
    server_certificate: &opcua_crypto::X509,
    server_nonce: &ByteString,
) -> ActivateSessionRequest {
    let signature = opcua_crypto::create_signature_data(
        private_key,
        SecurityPolicy::Basic256Sha256,
        &server_certificate.as_byte_string(),
        server_nonce,
    )
    .expect("X.509 user-token signature should be created");

    ActivateSessionRequest {
        request_header: RequestHeader {
            authentication_token: authentication_token.clone(),
            ..Default::default()
        },
        client_signature: SignatureData::null(),
        client_software_certificates: None,
        locale_ids: None,
        user_identity_token: ExtensionObject::from_message(X509IdentityToken {
            policy_id: UAString::from(POLICY_ID_X509),
            certificate_data: cert.as_byte_string(),
        }),
        user_token_signature: signature,
    }
}

pub(super) fn anonymous_identity() -> IdentityToken {
    anonymous_identity_with_policy(POLICY_ID_ANONYMOUS)
}

pub(super) fn anonymous_identity_with_policy(policy_id: &str) -> IdentityToken {
    IdentityToken::Anonymous(AnonymousIdentityToken {
        policy_id: UAString::from(policy_id),
    })
}
