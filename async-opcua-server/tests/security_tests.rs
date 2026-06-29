//! Security integration tests for PubSub keys, OAuth2 identities, and password identities.

use std::{
    fs,
    future::Future,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, TimeZone, Utc};
use const_oid::db::rfc5280::{ID_KP_CLIENT_AUTH, ID_KP_SERVER_AUTH};
use const_oid::db::rfc5912::SHA_256_WITH_RSA_ENCRYPTION;
use opcua_client::{ClientBuilder, IdentityToken};
use opcua_crypto::{
    create_signature_data, AlternateNames, CertificateStore, KeySize, PrivateKey, SecurityPolicy,
    X509Data, X509,
};
use opcua_server::{
    authenticator::{issued_token_security_policy, user_pass_security_policy_id, AuthManager},
    authorization::SessionAuthorizationProfile,
    diagnostics::NamespaceMetadata,
    node_manager::memory::simple_node_manager,
    services::security::{
        GetSecurityKeysRequest, GetSecurityKeysResponse, SecurityGroupKeys, SecurityKeyService,
        CURRENT_SECURITY_TOKEN_ID,
    },
    ServerBuilder, ServerEndpoint, ServerHandle, ServerUserToken, ANONYMOUS_USER_TOKEN_ID,
};
use opcua_types::{
    issued_token_types, ActivateSessionRequest, ByteString, Error, ExtensionObject,
    IssuedIdentityToken, MessageSecurityMode, SignatureData, StatusCode, UAString,
    UserNameIdentityToken, UserTokenPolicy, UserTokenType, X509IdentityToken,
};
use rsa::{
    pkcs1v15::{Signature as RsaSignature, SigningKey},
    pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding},
    rand_core::OsRng,
    signature::{SignatureEncoding, Signer},
    RsaPrivateKey,
};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use sha2::Sha256;
use tokio::net::TcpListener;
use x509_cert::{
    builder::{Builder, CertificateBuilder, Profile},
    crl::{CertificateList, RevokedCert, TbsCertList},
    der::{
        asn1::{Any, BitString, Null, OctetString},
        Encode,
    },
    ext::pkix::{
        AuthorityKeyIdentifier, BasicConstraints, ExtendedKeyUsage, KeyUsage, KeyUsages,
        SubjectKeyIdentifier,
    },
    name::Name,
    serial_number::SerialNumber,
    spki::{AlgorithmIdentifierOwned, SubjectPublicKeyInfoOwned},
    time::{Time, Validity},
    Version,
};

const OAUTH2_PATH: &str = "/oauth2";
const OAUTH2_ISSUER: &str = "https://issuer.example";
const OAUTH2_AUDIENCE: &str = "opcua-server";
const PUBSUB_SECURITY_POLICY_URI: &str =
    "http://opcfoundation.org/UA/SecurityPolicy#Aes256_Sha256_RsaPss";
const X509_PATH: &str = "/x509";
const X509_USER_TOKEN_ID: &str = "x509-user";
const AUTH_FAILURE_TARPIT_MIN: Duration = Duration::from_millis(100);
const AUTH_FAILURE_TARPIT_TIMEOUT: Duration = Duration::from_secs(1);

static NEXT_TEST_ID: AtomicUsize = AtomicUsize::new(0);

async fn assert_tarpitted_auth_failure<T>(
    auth: impl Future<Output = Result<T, Error>>,
    failure: &str,
) -> Error {
    let started = Instant::now();
    tokio::pin!(auth);

    tokio::select! {
        result = &mut auth => {
            let status = result.err().map(|err| err.status());
            panic!("{failure} returned before tarpitting; status={status:?}");
        }
        probe = tokio::time::timeout(Duration::from_millis(10), tokio::task::yield_now()) => {
            assert!(probe.is_ok(), "auth tarpit must not block the current-thread runtime");
        }
    }

    let result = tokio::time::timeout(AUTH_FAILURE_TARPIT_TIMEOUT, &mut auth)
        .await
        .expect("auth failure tarpit should complete");
    let err = match result {
        Ok(_) => panic!("{failure} should be rejected"),
        Err(err) => err,
    };

    assert_eq!(err.status(), StatusCode::BadUserAccessDenied);
    assert!(
        started.elapsed() >= AUTH_FAILURE_TARPIT_MIN,
        "{failure} returned before the minimum tarpit delay"
    );
    err
}

#[test]
fn get_security_keys_contract_matches_part14_signature() {
    let request = GetSecurityKeysRequest::new("group-1", CURRENT_SECURITY_TOKEN_ID, 2);

    assert_eq!(request.security_group_id.as_ref(), "group-1");
    assert_eq!(request.starting_token_id, CURRENT_SECURITY_TOKEN_ID);
    assert_eq!(request.requested_key_count, 2);

    let response = GetSecurityKeysResponse::new(
        "http://opcfoundation.org/UA/SecurityPolicy#Aes256_Sha256_RsaPss",
        7,
        vec![
            ByteString::from(b"current-key"),
            ByteString::from(b"next-key"),
        ],
        500.0,
        1_000.0,
    );

    assert_eq!(
        response.security_policy_uri.as_ref(),
        "http://opcfoundation.org/UA/SecurityPolicy#Aes256_Sha256_RsaPss"
    );
    assert_eq!(response.first_token_id, 7);
    assert_eq!(response.keys.len(), 2);
    assert_eq!(response.time_to_next_key, 500.0);
    assert_eq!(response.key_lifetime, 1_000.0);
}

#[test]
fn get_security_keys_handler_returns_current_and_future_keys() {
    let service = SecurityKeyService::new();
    service
        .register_security_group("group-1", security_group_keys(7))
        .unwrap();

    let response = service
        .get_security_keys(GetSecurityKeysRequest::new(
            "group-1",
            CURRENT_SECURITY_TOKEN_ID,
            2,
        ))
        .unwrap();

    assert_eq!(
        response.security_policy_uri.as_ref(),
        PUBSUB_SECURITY_POLICY_URI
    );
    assert_eq!(response.first_token_id, 7);
    assert_eq!(response.keys, key_bytes(&["current-key", "next-key"]));
    assert!(response.time_to_next_key <= 60_000.0);
    assert!(response.time_to_next_key > 59_000.0);
    assert_eq!(response.key_lifetime, 60_000.0);
}

#[test]
fn get_security_keys_handler_can_start_at_future_token() {
    let service = SecurityKeyService::new();
    service
        .register_security_group("group-1", security_group_keys(7))
        .unwrap();

    let response = service
        .get_security_keys(GetSecurityKeysRequest::new("group-1", 8, 2))
        .unwrap();

    assert_eq!(response.first_token_id, 8);
    assert_eq!(response.keys, key_bytes(&["next-key"]));
}

#[test]
fn get_security_keys_handler_rejects_unknown_group() {
    let service = SecurityKeyService::new();

    let error = service
        .get_security_keys(GetSecurityKeysRequest::new(
            "missing",
            CURRENT_SECURITY_TOKEN_ID,
            1,
        ))
        .unwrap_err();

    assert_eq!(error, StatusCode::BadNotFound);
}

#[test]
fn get_security_keys_handler_rejects_invalid_requests() {
    let service = SecurityKeyService::new();
    service
        .register_security_group("group-1", security_group_keys(7))
        .unwrap();

    let empty_group = service
        .get_security_keys(GetSecurityKeysRequest::new(
            "",
            CURRENT_SECURITY_TOKEN_ID,
            1,
        ))
        .unwrap_err();
    let zero_count = service
        .get_security_keys(GetSecurityKeysRequest::new(
            "group-1",
            CURRENT_SECURITY_TOKEN_ID,
            0,
        ))
        .unwrap_err();

    assert_eq!(empty_group, StatusCode::BadInvalidArgument);
    assert_eq!(zero_count, StatusCode::BadInvalidArgument);
}

#[tokio::test]
async fn open_secure_channel_untrusted_client_cert_returns_bad_security_checks_failed() {
    let temp = TempPath::new("open-secure-channel-untrusted-client");
    let server_pki = temp.path().join("server-pki");
    let client_pki = temp.path().join("client-pki");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("security test listener should bind");
    let endpoint_url = format!(
        "opc.tcp://127.0.0.1:{}/",
        listener
            .local_addr()
            .expect("security test listener should have address")
            .port()
    );
    let port = listener
        .local_addr()
        .expect("security test listener should have address")
        .port();

    let (server, handle) = ServerBuilder::new()
        .application_name("OpenSecureChannel Security Test Server")
        .application_uri("urn:open-secure-channel-security-test-server")
        .product_uri("urn:open-secure-channel-security-test-server")
        .host("127.0.0.1")
        .port(port)
        .pki_dir(&server_pki)
        .create_sample_keypair(true)
        .trust_client_certs(false)
        .discovery_urls(vec![endpoint_url.clone()])
        .add_endpoint(
            "secured",
            (
                "/",
                SecurityPolicy::Aes128Sha256RsaOaep,
                MessageSecurityMode::SignAndEncrypt,
                &[ANONYMOUS_USER_TOKEN_ID] as &[&str],
            ),
        )
        .with_node_manager(simple_node_manager(
            NamespaceMetadata {
                namespace_uri: "urn:open-secure-channel-security-test".to_string(),
                namespace_index: 2,
                ..Default::default()
            },
            "open-secure-channel-security-test",
        ))
        .build()
        .expect("OpenSecureChannel security test server should build");
    handle.info().port.store(port, Ordering::Relaxed);
    let endpoint = handle
        .info()
        .endpoints(&UAString::from(endpoint_url.as_str()), &None)
        .expect("security test endpoint should be described")
        .into_iter()
        .find(|endpoint| {
            endpoint.security_policy_uri.as_ref() == SecurityPolicy::Aes128Sha256RsaOaep.to_uri()
                && endpoint.security_mode == MessageSecurityMode::SignAndEncrypt
        })
        .expect("secured security test endpoint should be advertised");
    let server_task = tokio::spawn(async move {
        let _ = server.run_with(listener).await;
    });

    let mut client = ClientBuilder::new()
        .application_name("OpenSecureChannel Security Test Client")
        .application_uri("urn:open-secure-channel-security-test-client")
        .product_uri("urn:open-secure-channel-security-test-client")
        .pki_dir(&client_pki)
        .create_sample_keypair(true)
        .trust_server_certs(true)
        .session_retry_limit(0)
        .session_retry_initial(Duration::from_millis(20))
        .client()
        .expect("OpenSecureChannel security test client should build");

    let (_session, event_loop) = client
        .connect_to_endpoint_directly(endpoint, IdentityToken::Anonymous)
        .expect("session event loop should be created before channel polling");

    // OPC UA Part 4 6.1.3: an untrusted application certificate fails the
    // secured OpenSecureChannel trust check.
    let status = tokio::time::timeout(Duration::from_secs(10), event_loop.run())
        .await
        .expect("OpenSecureChannel rejection should complete");

    handle.cancel();
    server_task.abort();

    assert_eq!(status, StatusCode::BadSecurityChecksFailed);
}

fn security_group_keys(first_token_id: u32) -> SecurityGroupKeys {
    SecurityGroupKeys::with_current_key_started_at(
        PUBSUB_SECURITY_POLICY_URI,
        first_token_id,
        key_bytes(&["current-key", "next-key"]),
        Duration::from_secs(60),
        Instant::now(),
    )
    .unwrap()
}

fn key_bytes(keys: &[&str]) -> Vec<ByteString> {
    keys.iter()
        .map(|key| ByteString::from(key.as_bytes()))
        .collect()
}

struct TempPath {
    path: PathBuf,
}

impl TempPath {
    fn new(name: &str) -> Self {
        let id = NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::current_dir()
            .expect("current dir")
            .join("target")
            .join("security_tests")
            .join(format!("{name}-{}-{id}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("temporary test directory should be created");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct TestKey {
    rsa: RsaPrivateKey,
    private_key: PrivateKey,
}

#[derive(Clone, Copy)]
enum UserCertEku {
    None,
    Client,
    Server,
    Both,
}

#[derive(Clone, Copy)]
enum UserCertTrust {
    Trusted,
    Untrusted,
    Expired,
    WrongUsage,
    IncompleteChain,
    Revoked,
}

struct UserCertMaterial {
    cert: X509,
    private_key: PrivateKey,
}

struct X509UserFixture {
    endpoint_url: String,
    handle: ServerHandle,
    server_nonce: ByteString,
    user: UserCertMaterial,
    _pki: TempPath,
}

impl X509UserFixture {
    fn new(kind: UserCertTrust) -> Self {
        let pki = TempPath::new("x509-user-pki");
        let store = CertificateStore::new(pki.path());
        store
            .ensure_pki_path()
            .expect("X.509 user PKI structure should be created");

        let root_key = test_key(&pki, "root");
        let intermediate_key = test_key(&pki, "intermediate");
        let user_key = test_key(&pki, "user");

        let root = issue_test_cert(&TestCertSpec {
            subject_cn: "x509 user root",
            subject_key: &root_key.rsa,
            issuer_cn: "x509 user root",
            issuer_key: &root_key.rsa,
            signer_key: &root_key.rsa,
            is_ca: true,
            not_before: dt(2020, 1, 1),
            not_after: dt(2035, 1, 1),
            eku: UserCertEku::None,
            key_usage: KeyUsage(KeyUsages::KeyCertSign | KeyUsages::CRLSign),
            serial: 10,
        });
        let intermediate = issue_test_cert(&TestCertSpec {
            subject_cn: "x509 user intermediate",
            subject_key: &intermediate_key.rsa,
            issuer_cn: "x509 user root",
            issuer_key: &root_key.rsa,
            signer_key: &root_key.rsa,
            is_ca: true,
            not_before: dt(2020, 1, 1),
            not_after: dt(2035, 1, 1),
            eku: UserCertEku::None,
            key_usage: KeyUsage(KeyUsages::KeyCertSign | KeyUsages::CRLSign),
            serial: 11,
        });

        let (cert, private_key) = match kind {
            UserCertTrust::Trusted => (
                user_leaf(
                    &user_key.rsa,
                    &root_key.rsa,
                    &root_key.rsa,
                    dt(2035, 1, 1),
                    UserCertEku::Both,
                    100,
                ),
                user_key.private_key,
            ),
            UserCertTrust::Untrusted => (
                user_leaf(
                    &user_key.rsa,
                    &user_key.rsa,
                    &user_key.rsa,
                    dt(2035, 1, 1),
                    UserCertEku::Client,
                    101,
                ),
                user_key.private_key,
            ),
            UserCertTrust::Expired => (
                user_leaf(
                    &user_key.rsa,
                    &root_key.rsa,
                    &root_key.rsa,
                    dt(2021, 1, 1),
                    UserCertEku::Client,
                    102,
                ),
                user_key.private_key,
            ),
            UserCertTrust::WrongUsage => (
                user_leaf(
                    &user_key.rsa,
                    &root_key.rsa,
                    &root_key.rsa,
                    dt(2035, 1, 1),
                    UserCertEku::Server,
                    103,
                ),
                user_key.private_key,
            ),
            UserCertTrust::IncompleteChain => (
                issue_test_cert(&TestCertSpec {
                    subject_cn: "x509 user leaf",
                    subject_key: &user_key.rsa,
                    issuer_cn: "x509 user intermediate",
                    issuer_key: &intermediate_key.rsa,
                    signer_key: &intermediate_key.rsa,
                    is_ca: false,
                    not_before: dt(2020, 1, 1),
                    not_after: dt(2035, 1, 1),
                    eku: UserCertEku::Client,
                    key_usage: KeyUsage(KeyUsages::DigitalSignature | KeyUsages::KeyEncipherment),
                    serial: 104,
                }),
                user_key.private_key,
            ),
            UserCertTrust::Revoked => (
                user_leaf(
                    &user_key.rsa,
                    &root_key.rsa,
                    &root_key.rsa,
                    dt(2035, 1, 1),
                    UserCertEku::Client,
                    105,
                ),
                user_key.private_key,
            ),
        };

        if !matches!(kind, UserCertTrust::Untrusted) {
            write_cert_to(&store.trusted_certs_dir(), "root.der", &root);
        }
        if matches!(kind, UserCertTrust::Trusted) {
            write_cert_to(&store.issuer_certs_dir(), "intermediate.der", &intermediate);
        }
        if matches!(kind, UserCertTrust::Revoked) {
            let crl = make_test_crl("x509 user root", &root_key.rsa, &[105]);
            write_crl_to(&store.trusted_crls_dir(), "root.der", &crl);
        }

        let user_cert_path = pki.path().join("configured-user.der");
        write_cert_to(pki.path(), "configured-user.der", &cert);

        let endpoint = ServerEndpoint::new_none(X509_PATH, &[X509_USER_TOKEN_ID.into()]);
        let (_server, handle) = ServerBuilder::new()
            .without_node_managers()
            .application_name("X509 User Security Test Server")
            .application_uri("urn:x509-user-security-test-server")
            .product_uri("urn:x509-user-security-test-server")
            .host("127.0.0.1")
            .pki_dir(pki.path())
            .create_sample_keypair(true)
            .discovery_urls(vec!["opc.tcp://127.0.0.1:4857/x509".to_string()])
            .add_user_token(
                X509_USER_TOKEN_ID,
                ServerUserToken::x509("certificate-user", &user_cert_path),
            )
            .add_endpoint("x509", endpoint)
            .build()
            .expect("X.509 user security test server should build");

        Self {
            endpoint_url: format!("{}{}", handle.info().base_endpoint(), X509_PATH),
            handle,
            server_nonce: ByteString::from(b"x509-user-nonce".as_slice()),
            user: UserCertMaterial { cert, private_key },
            _pki: pki,
        }
    }

    async fn authenticate(&self) -> Result<(), Error> {
        self.authenticate_with_cert(&self.user.cert, &self.user.private_key)
            .await
    }

    async fn authenticate_with_tampered_signature(&self) -> Result<(), Error> {
        let mut request = self.activate_session_request(&self.user.cert, &self.user.private_key);
        let mut signature = request.user_token_signature.signature.as_ref().to_vec();
        let last = signature
            .last_mut()
            .expect("fixture X.509 user-token signature should not be empty");
        *last ^= 0xFF;
        request.user_token_signature.signature = ByteString::from(signature);

        self.handle
            .info()
            .authenticate_endpoint(
                &request,
                &self.endpoint_url,
                SecurityPolicy::None,
                MessageSecurityMode::None,
                request.user_identity_token.clone(),
                &self.server_nonce,
            )
            .await
            .map(|_| ())
    }

    async fn authenticate_with_cert(
        &self,
        cert: &X509,
        private_key: &PrivateKey,
    ) -> Result<(), Error> {
        let request = self.activate_session_request(cert, private_key);
        self.handle
            .info()
            .authenticate_endpoint(
                &request,
                &self.endpoint_url,
                SecurityPolicy::None,
                MessageSecurityMode::None,
                request.user_identity_token.clone(),
                &self.server_nonce,
            )
            .await
            .map(|_| ())
    }

    async fn authenticate_malformed_certificate(&self) -> Result<(), Error> {
        let request = ActivateSessionRequest {
            request_header: Default::default(),
            client_signature: SignatureData::null(),
            client_software_certificates: None,
            locale_ids: None,
            user_identity_token: ExtensionObject::from_message(X509IdentityToken {
                policy_id: UAString::from("x509"),
                certificate_data: ByteString::from(&[0x30, 0x03, 0x02, 0x01]),
            }),
            user_token_signature: SignatureData::null(),
        };
        self.handle
            .info()
            .authenticate_endpoint(
                &request,
                &self.endpoint_url,
                SecurityPolicy::None,
                MessageSecurityMode::None,
                request.user_identity_token.clone(),
                &self.server_nonce,
            )
            .await
            .map(|_| ())
    }

    fn activate_session_request(
        &self,
        cert: &X509,
        private_key: &PrivateKey,
    ) -> ActivateSessionRequest {
        let server_cert = self
            .handle
            .info()
            .server_certificate
            .read()
            .clone()
            .expect("test server should have a certificate");
        let signature = create_signature_data(
            private_key,
            SecurityPolicy::Basic256Sha256,
            &server_cert.as_byte_string(),
            &self.server_nonce,
        )
        .expect("X.509 user-token signature should be created");

        ActivateSessionRequest {
            request_header: Default::default(),
            client_signature: SignatureData::null(),
            client_software_certificates: None,
            locale_ids: None,
            user_identity_token: ExtensionObject::from_message(X509IdentityToken {
                policy_id: UAString::from("x509"),
                certificate_data: cert.as_byte_string(),
            }),
            user_token_signature: signature,
        }
    }
}

struct TestCertSpec<'a> {
    subject_cn: &'a str,
    subject_key: &'a RsaPrivateKey,
    issuer_cn: &'a str,
    issuer_key: &'a RsaPrivateKey,
    signer_key: &'a RsaPrivateKey,
    is_ca: bool,
    not_before: DateTime<Utc>,
    not_after: DateTime<Utc>,
    eku: UserCertEku,
    key_usage: KeyUsage,
    serial: u32,
}

fn test_key(temp: &TempPath, name: &str) -> TestKey {
    let rsa = RsaPrivateKey::new(&mut OsRng, 2048).expect("RSA fixture key should generate");
    let pem = rsa
        .to_pkcs8_pem(LineEnding::LF)
        .expect("RSA fixture key should encode as PKCS8 PEM");
    let path = temp.path().join(format!("{name}.pem"));
    fs::write(&path, pem.as_bytes()).expect("RSA fixture key should be written");
    let private_key =
        CertificateStore::read_pkey(&path).expect("RSA fixture key should be read by store");
    TestKey { rsa, private_key }
}

fn dt(year: i32, month: u32, day: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(year, month, day, 0, 0, 0).unwrap()
}

fn x509_time(dt: DateTime<Utc>) -> Time {
    let secs = u64::try_from(dt.timestamp()).expect("non-negative X.509 fixture timestamp");
    Time::try_from(UNIX_EPOCH + Duration::from_secs(secs)).expect("valid X.509 fixture time")
}

fn spki_from_rsa(key: &RsaPrivateKey) -> SubjectPublicKeyInfoOwned {
    let public_key_der = key
        .to_public_key()
        .to_public_key_der()
        .expect("fixture public key should encode");
    SubjectPublicKeyInfoOwned::try_from(public_key_der.as_bytes())
        .expect("fixture SubjectPublicKeyInfo should parse")
}

fn ski_of(spki: &SubjectPublicKeyInfoOwned) -> Vec<u8> {
    let mut hasher = Sha1::new();
    hasher.update(spki.subject_public_key.raw_bytes());
    hasher.finalize().to_vec()
}

fn issue_test_cert(spec: &TestCertSpec<'_>) -> X509 {
    let subject_spki = spki_from_rsa(spec.subject_key);
    let issuer_spki = spki_from_rsa(spec.issuer_key);
    let signing_key = SigningKey::<Sha256>::new(spec.signer_key.clone());
    let subject = Name::from_str(&format!("CN={}", spec.subject_cn)).expect("fixture subject");
    let issuer = Name::from_str(&format!("CN={}", spec.issuer_cn)).expect("fixture issuer");
    let mut builder = CertificateBuilder::new(
        Profile::Manual {
            issuer: Some(issuer),
        },
        SerialNumber::from(spec.serial),
        Validity {
            not_before: x509_time(spec.not_before),
            not_after: x509_time(spec.not_after),
        },
        subject,
        subject_spki.clone(),
        &signing_key,
    )
    .expect("fixture certificate builder should initialize");

    builder
        .add_extension(&SubjectKeyIdentifier(
            OctetString::new(ski_of(&subject_spki)).expect("fixture SKI"),
        ))
        .expect("fixture certificate should accept SKI");
    builder
        .add_extension(&AuthorityKeyIdentifier {
            authority_cert_issuer: None,
            key_identifier: Some(OctetString::new(ski_of(&issuer_spki)).expect("fixture AKI")),
            authority_cert_serial_number: None,
        })
        .expect("fixture certificate should accept AKI");
    builder
        .add_extension(&BasicConstraints {
            ca: spec.is_ca,
            path_len_constraint: None,
        })
        .expect("fixture certificate should accept basic constraints");
    builder
        .add_extension(&spec.key_usage)
        .expect("fixture certificate should accept key usage");

    match spec.eku {
        UserCertEku::None => {}
        UserCertEku::Client => builder
            .add_extension(&ExtendedKeyUsage(vec![ID_KP_CLIENT_AUTH]))
            .expect("fixture certificate should accept client EKU"),
        UserCertEku::Server => builder
            .add_extension(&ExtendedKeyUsage(vec![ID_KP_SERVER_AUTH]))
            .expect("fixture certificate should accept server EKU"),
        UserCertEku::Both => builder
            .add_extension(&ExtendedKeyUsage(vec![
                ID_KP_CLIENT_AUTH,
                ID_KP_SERVER_AUTH,
            ]))
            .expect("fixture certificate should accept both EKUs"),
    }

    let cert = builder
        .build::<RsaSignature>()
        .expect("fixture certificate should build");
    X509::from_der(&cert.to_der().expect("fixture certificate should encode"))
        .expect("fixture certificate should parse")
}

fn user_leaf(
    subject_key: &RsaPrivateKey,
    issuer_key: &RsaPrivateKey,
    signer_key: &RsaPrivateKey,
    not_after: DateTime<Utc>,
    eku: UserCertEku,
    serial: u32,
) -> X509 {
    let issuer_cn = if std::ptr::eq(subject_key, issuer_key) {
        "x509 user leaf"
    } else {
        "x509 user root"
    };
    issue_test_cert(&TestCertSpec {
        subject_cn: "x509 user leaf",
        subject_key,
        issuer_cn,
        issuer_key,
        signer_key,
        is_ca: false,
        not_before: dt(2020, 1, 1),
        not_after,
        eku,
        key_usage: KeyUsage(KeyUsages::DigitalSignature | KeyUsages::KeyEncipherment),
        serial,
    })
}

fn make_test_crl(
    issuer_cn: &str,
    issuer_key: &RsaPrivateKey,
    revoked_serials: &[u32],
) -> CertificateList {
    let issuer = Name::from_str(&format!("CN={issuer_cn}")).expect("fixture CRL issuer");
    let algorithm = AlgorithmIdentifierOwned {
        oid: SHA_256_WITH_RSA_ENCRYPTION,
        parameters: Some(Any::from(Null)),
    };
    let revoked_certificates = if revoked_serials.is_empty() {
        None
    } else {
        Some(
            revoked_serials
                .iter()
                .map(|serial| RevokedCert {
                    serial_number: SerialNumber::from(*serial),
                    revocation_date: x509_time(dt(2024, 1, 1)),
                    crl_entry_extensions: None,
                })
                .collect(),
        )
    };
    let tbs = TbsCertList {
        version: Version::V2,
        signature: algorithm.clone(),
        issuer,
        this_update: x509_time(dt(2024, 1, 1)),
        next_update: Some(x509_time(dt(2035, 1, 1))),
        revoked_certificates,
        crl_extensions: None,
    };
    let tbs_der = tbs.to_der().expect("fixture CRL TBS should encode");
    let signing_key = SigningKey::<Sha256>::new(issuer_key.clone());
    let signature: RsaSignature = signing_key.sign(&tbs_der);

    CertificateList {
        tbs_cert_list: tbs,
        signature_algorithm: algorithm,
        signature: BitString::from_bytes(&signature.to_vec()).expect("fixture CRL signature"),
    }
}

fn write_cert_to(dir: &Path, name: &str, cert: &X509) {
    fs::write(
        dir.join(name),
        cert.to_der().expect("fixture certificate should encode"),
    )
    .expect("fixture certificate should be written");
}

fn write_crl_to(dir: &Path, name: &str, crl: &CertificateList) {
    fs::write(
        dir.join(name),
        crl.to_der().expect("fixture CRL should encode"),
    )
    .expect("fixture CRL should be written");
}

#[tokio::test]
async fn x509_user_token_untrusted_configured_thumbprint_is_rejected() {
    let fixture = X509UserFixture::new(UserCertTrust::Untrusted);

    let err = fixture
        .authenticate()
        .await
        .expect_err("configured but untrusted X.509 user certificate must be rejected");

    assert_eq!(err.status(), StatusCode::BadCertificateUntrusted);
}

#[tokio::test]
async fn x509_user_token_expired_configured_thumbprint_is_rejected() {
    let fixture = X509UserFixture::new(UserCertTrust::Expired);

    let err = fixture
        .authenticate()
        .await
        .expect_err("configured but expired X.509 user certificate must be rejected");

    assert_eq!(err.status(), StatusCode::BadCertificateTimeInvalid);
}

#[tokio::test]
async fn x509_user_token_wrong_usage_configured_thumbprint_is_rejected() {
    let fixture = X509UserFixture::new(UserCertTrust::WrongUsage);

    let err = fixture
        .authenticate()
        .await
        .expect_err("configured but wrong-usage X.509 user certificate must be rejected");

    assert_eq!(err.status(), StatusCode::BadCertificateUseNotAllowed);
}

#[tokio::test]
async fn x509_user_token_incomplete_or_revoked_chain_is_rejected() {
    let incomplete = X509UserFixture::new(UserCertTrust::IncompleteChain);
    let incomplete_err = incomplete
        .authenticate()
        .await
        .expect_err("configured but incomplete-chain X.509 user certificate must be rejected");
    assert_eq!(
        incomplete_err.status(),
        StatusCode::BadCertificateChainIncomplete
    );

    let revoked = X509UserFixture::new(UserCertTrust::Revoked);
    let revoked_err = revoked
        .authenticate()
        .await
        .expect_err("configured but revoked X.509 user certificate must be rejected");
    assert_eq!(revoked_err.status(), StatusCode::BadCertificateRevoked);
}

#[tokio::test]
async fn x509_user_token_malformed_certificate_is_rejected() {
    let fixture = X509UserFixture::new(UserCertTrust::Trusted);

    let err = fixture
        .authenticate_malformed_certificate()
        .await
        .expect_err("malformed X.509 user certificate bytes must be rejected");

    assert_eq!(err.status(), StatusCode::BadCertificateInvalid);
}

#[tokio::test]
async fn x509_user_token_bad_signature_is_distinguishable() {
    let fixture = X509UserFixture::new(UserCertTrust::Trusted);

    let err = fixture
        .authenticate_with_tampered_signature()
        .await
        .expect_err("trusted X.509 user certificate with bad signature must be rejected");

    assert_eq!(err.status(), StatusCode::BadUserSignatureInvalid);
}

struct IssuedTokenAuthenticator;

#[async_trait]
impl AuthManager for IssuedTokenAuthenticator {
    fn user_token_policies(&self, endpoint: &ServerEndpoint) -> Vec<UserTokenPolicy> {
        if endpoint.path == OAUTH2_PATH {
            vec![UserTokenPolicy {
                policy_id: issued_token_security_policy(endpoint),
                token_type: UserTokenType::IssuedToken,
                issued_token_type: UAString::from(issued_token_types::JSON_WEB_TOKEN),
                issuer_endpoint_url: UAString::from(OAUTH2_ISSUER),
                security_policy_uri: UAString::null(),
            }]
        } else {
            Vec::new()
        }
    }
}

struct OAuth2Fixture {
    endpoint_url: String,
    handle: ServerHandle,
    private_key: PrivateKey,
    policy_id: UAString,
    _pki: TempPath,
}

impl OAuth2Fixture {
    fn new() -> Self {
        let pki = TempPath::new("oauth2-pki");
        let (private_key, policy_id, issuer_cert_path) =
            setup_trusted_oauth2_certificate(pki.path());
        let endpoint = ServerEndpoint::new_none(OAUTH2_PATH, &[]);
        let policy_id = {
            assert_eq!(issued_token_security_policy(&endpoint), policy_id);
            policy_id
        };

        let (_server, handle) = ServerBuilder::new()
            .without_node_managers()
            .application_name("OAuth2 Security Test Server")
            .application_uri("urn:oauth2-security-test-server")
            .product_uri("urn:oauth2-security-test-server")
            .host("127.0.0.1")
            .pki_dir(pki.path())
            .create_sample_keypair(true)
            .discovery_urls(vec!["opc.tcp://127.0.0.1:4855/oauth2".to_string()])
            .oauth2_issuer(OAUTH2_ISSUER)
            .oauth2_audience(OAUTH2_AUDIENCE)
            .oauth2_issuer_certificate_path(issuer_cert_path)
            .with_authenticator(Arc::new(IssuedTokenAuthenticator))
            .add_endpoint("oauth2", endpoint)
            .build()
            .expect("OAuth2 security test server should build");

        Self {
            endpoint_url: format!("{}{}", handle.info().base_endpoint(), OAUTH2_PATH),
            handle,
            private_key,
            policy_id,
            _pki: pki,
        }
    }

    async fn authenticate(
        &self,
        token: &str,
    ) -> Result<opcua_server::authenticator::UserToken, Error> {
        let request = activate_session_request(token, self.policy_id.clone());
        self.handle
            .info()
            .authenticate_endpoint(
                &request,
                &self.endpoint_url,
                SecurityPolicy::None,
                MessageSecurityMode::None,
                request.user_identity_token.clone(),
                &ByteString::null(),
            )
            .await
            .map(|(user_token, _claims)| user_token)
    }

    async fn authenticate_encrypted_token(
        &self,
        encrypted_token: ByteString,
        encryption_algorithm: UAString,
    ) -> Result<opcua_server::authenticator::UserToken, Error> {
        let request = activate_session_request_with_issued_token(IssuedIdentityToken {
            policy_id: self.policy_id.clone(),
            token_data: encrypted_token,
            encryption_algorithm,
        });
        self.handle
            .info()
            .authenticate_endpoint(
                &request,
                &self.endpoint_url,
                SecurityPolicy::None,
                MessageSecurityMode::None,
                request.user_identity_token.clone(),
                &ByteString::null(),
            )
            .await
            .map(|(user_token, _claims)| user_token)
    }

    fn server_private_key(&self) -> PrivateKey {
        self.handle
            .info()
            .server_pkey
            .read()
            .clone()
            .expect("test server should have a private key")
    }

    async fn authenticate_with_claims(
        &self,
        token: &str,
    ) -> Result<
        (
            opcua_server::authenticator::UserToken,
            opcua_crypto::identity::ClaimProfile,
        ),
        Error,
    > {
        let request = activate_session_request(token, self.policy_id.clone());
        self.handle
            .info()
            .authenticate_endpoint(
                &request,
                &self.endpoint_url,
                SecurityPolicy::None,
                MessageSecurityMode::None,
                request.user_identity_token.clone(),
                &ByteString::null(),
            )
            .await
            .map(|(user_token, claims)| {
                (
                    user_token,
                    claims.expect("OAuth2 authentication should return claims"),
                )
            })
    }
}

struct PasswordFixture {
    endpoint_url: String,
    handle: ServerHandle,
    policy_id: UAString,
    _pki: TempPath,
}

impl PasswordFixture {
    fn new() -> Self {
        const PASSWORD_PATH: &str = "/password";
        const PASSWORD_USER_TOKEN_ID: &str = "password-user";

        let pki = TempPath::new("password-pki");
        let endpoint = ServerEndpoint::new_none(PASSWORD_PATH, &[PASSWORD_USER_TOKEN_ID.into()]);
        let policy_id = user_pass_security_policy_id(&endpoint);
        let (_server, handle) = ServerBuilder::new()
            .without_node_managers()
            .application_name("Password Security Test Server")
            .application_uri("urn:password-security-test-server")
            .product_uri("urn:password-security-test-server")
            .host("127.0.0.1")
            .pki_dir(pki.path())
            .discovery_urls(vec!["opc.tcp://127.0.0.1:4856/password".to_string()])
            .add_user_token(
                PASSWORD_USER_TOKEN_ID,
                ServerUserToken::user_pass("brew-operator", "correct-password"),
            )
            .add_endpoint("password", endpoint)
            .build()
            .expect("password security test server should build");

        Self {
            endpoint_url: format!("{}{}", handle.info().base_endpoint(), PASSWORD_PATH),
            handle,
            policy_id,
            _pki: pki,
        }
    }

    async fn authenticate(&self, username: &str, password: &str) -> Result<(), Error> {
        let request = activate_session_request_with_username_token(
            self.policy_id.clone(),
            username,
            password,
        );
        self.handle
            .info()
            .authenticate_endpoint(
                &request,
                &self.endpoint_url,
                SecurityPolicy::None,
                MessageSecurityMode::None,
                request.user_identity_token.clone(),
                &ByteString::null(),
            )
            .await
            .map(|_| ())
    }
}

// Feature 025 US1: also returns the issuer cert path — the validator now pins to it.
fn setup_trusted_oauth2_certificate(pki_path: &Path) -> (PrivateKey, UAString, std::path::PathBuf) {
    let certificate_store = CertificateStore::new(pki_path);
    certificate_store
        .ensure_pki_path()
        .expect("PKI structure should be created");

    let (cert, private_key) = oauth2_cert_and_key("oauth2-idp");
    let cert_path = certificate_store
        .trusted_certs_dir()
        .join(CertificateStore::cert_file_name(&cert));
    fs::write(
        &cert_path,
        cert.to_der().expect("certificate should encode"),
    )
    .expect("trusted OAuth2 certificate should be written");

    let endpoint = ServerEndpoint::new_none(OAUTH2_PATH, &[]);
    (
        private_key,
        issued_token_security_policy(&endpoint),
        cert_path,
    )
}

fn oauth2_cert_and_key(common_name: &str) -> (X509, PrivateKey) {
    let mut alt_host_names = AlternateNames::new();
    alt_host_names.add_dns("localhost");
    alt_host_names.add_uri("urn:oauth2-idp");
    let x509_data = X509Data {
        key_size: 2048,
        common_name: common_name.to_string(),
        organization: "async-opcua tests".to_string(),
        organizational_unit: "security".to_string(),
        country: "US".to_string(),
        state: "test".to_string(),
        alt_host_names,
        certificate_duration_days: 30,
    };
    X509::cert_and_pkey(&x509_data).expect("OAuth2 test certificate should be generated")
}

fn activate_session_request(token: &str, policy_id: UAString) -> ActivateSessionRequest {
    activate_session_request_with_issued_token(IssuedIdentityToken {
        policy_id,
        token_data: ByteString::from(token.as_bytes()),
        encryption_algorithm: UAString::null(),
    })
}

fn activate_session_request_with_issued_token(
    token: IssuedIdentityToken,
) -> ActivateSessionRequest {
    ActivateSessionRequest {
        request_header: Default::default(),
        client_signature: SignatureData::null(),
        client_software_certificates: None,
        locale_ids: None,
        user_identity_token: ExtensionObject::from_message(token),
        user_token_signature: SignatureData::null(),
    }
}

fn activate_session_request_with_username_token(
    policy_id: UAString,
    username: &str,
    password: &str,
) -> ActivateSessionRequest {
    ActivateSessionRequest {
        request_header: Default::default(),
        client_signature: SignatureData::null(),
        client_software_certificates: None,
        locale_ids: None,
        user_identity_token: ExtensionObject::from_message(UserNameIdentityToken {
            policy_id,
            user_name: UAString::from(username),
            password: ByteString::from(password.as_bytes()),
            encryption_algorithm: UAString::null(),
        }),
        user_token_signature: SignatureData::null(),
    }
}

fn signed_jwt(payload: Value, private_key: &PrivateKey) -> String {
    let header = json!({"alg": "RS256", "typ": "JWT"});
    let encoded_header = URL_SAFE_NO_PAD.encode(header.to_string());
    let encoded_payload = URL_SAFE_NO_PAD.encode(payload.to_string());
    let signing_input = format!("{encoded_header}.{encoded_payload}");
    let mut signature = vec![0u8; private_key.size()];
    let signature_len = private_key
        .sign_sha256(signing_input.as_bytes(), &mut signature)
        .expect("JWT signing should succeed");
    let encoded_signature = URL_SAFE_NO_PAD.encode(&signature[..signature_len]);

    format!("{signing_input}.{encoded_signature}")
}

fn rsa_oaep_encrypt(
    policy: SecurityPolicy,
    private_key: &PrivateKey,
    plaintext: &[u8],
) -> ByteString {
    let public_key = private_key.to_public_key();
    let mut ciphertext = vec![0u8; policy.calculate_cipher_text_size(plaintext.len(), &public_key)];
    let ciphertext_len = policy
        .asymmetric_encrypt(&public_key, plaintext, &mut ciphertext)
        .expect("RSA-OAEP test encryption should succeed");
    ciphertext.truncate(ciphertext_len);
    ByteString::from(ciphertext)
}

fn future_expiration() -> i64 {
    epoch_seconds() + 3600
}

fn past_expiration() -> i64 {
    epoch_seconds() - 3600
}

fn epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after Unix epoch")
        .as_secs() as i64
}

#[tokio::test]
async fn oauth2_valid_jwt_maps_to_session_authorization_profile() {
    let fixture = OAuth2Fixture::new();
    let token = signed_jwt(
        json!({
            "iss": OAUTH2_ISSUER,
            "aud": ["engineering-tools", OAUTH2_AUDIENCE],
            "exp": future_expiration(),
            "sub": "brew-operator",
            "roles": ["operator", "observer"],
            "permissions": ["read", "write"]
        }),
        &fixture.private_key,
    );

    let (user_token, claims) = fixture
        .authenticate_with_claims(&format!("Bearer {token}"))
        .await
        .expect("valid OAuth2 JWT should authenticate");
    let profile = SessionAuthorizationProfile::from_claims(&claims);

    assert_eq!(user_token.0, "brew-operator");
    assert_eq!(profile.username, "brew-operator");
    assert_eq!(profile.roles, vec!["operator", "observer"]);
    assert_eq!(profile.permissions, vec!["read", "write"]);
    assert!(profile.is_operator);
    assert!(profile.is_observer);
    assert!(!profile.is_admin);
    assert!(profile.can_read());
    assert!(profile.can_write());

    let endpoints = fixture
        .handle
        .info()
        .endpoints(&UAString::from(fixture.endpoint_url.as_str()), &None)
        .expect("OAuth2 endpoint should be returned");
    let issued_policy = endpoints[0]
        .find_policy(UserTokenType::IssuedToken)
        .expect("OAuth2 issued token policy should be advertised");
    assert_eq!(issued_policy.issuer_endpoint_url.as_ref(), OAUTH2_ISSUER);
    assert_eq!(
        fixture.handle.info().config.oauth2_issuer.as_deref(),
        Some(OAUTH2_ISSUER)
    );
    assert_eq!(
        fixture.handle.info().config.oauth2_audience.as_deref(),
        Some(OAUTH2_AUDIENCE)
    );
}

#[tokio::test]
async fn oauth2_invalid_jwts_are_rejected() {
    let fixture = OAuth2Fixture::new();
    let (_untrusted_cert, untrusted_key) = oauth2_cert_and_key("untrusted-oauth2-idp");
    let invalid_tokens = [
        signed_jwt(
            json!({
                "iss": OAUTH2_ISSUER,
                "aud": OAUTH2_AUDIENCE,
                "exp": past_expiration(),
                "sub": "brew-operator"
            }),
            &fixture.private_key,
        ),
        signed_jwt(
            json!({
                "iss": "https://wrong-issuer.example",
                "aud": OAUTH2_AUDIENCE,
                "exp": future_expiration(),
                "sub": "brew-operator"
            }),
            &fixture.private_key,
        ),
        signed_jwt(
            json!({
                "iss": OAUTH2_ISSUER,
                "aud": "wrong-audience",
                "exp": future_expiration(),
                "sub": "brew-operator"
            }),
            &fixture.private_key,
        ),
        signed_jwt(
            json!({
                "iss": OAUTH2_ISSUER,
                "aud": OAUTH2_AUDIENCE,
                "exp": future_expiration(),
                "sub": "brew-operator"
            }),
            &untrusted_key,
        ),
    ];

    for token in invalid_tokens {
        let err = fixture
            .authenticate(&token)
            .await
            .expect_err("invalid OAuth2 JWT should be rejected");

        assert_eq!(err.status(), StatusCode::BadUserAccessDenied);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_oauth2_jwt_validation_failure_is_tarpitted() {
    let fixture = OAuth2Fixture::new();
    let token = signed_jwt(
        json!({
            "iss": OAUTH2_ISSUER,
            "aud": OAUTH2_AUDIENCE,
            "exp": past_expiration(),
            "sub": "brew-operator"
        }),
        &fixture.private_key,
    );

    assert_tarpitted_auth_failure(fixture.authenticate(&token), "invalid OAuth2 JWT").await;
}

#[tokio::test(flavor = "current_thread")]
async fn username_password_auth_failure_is_tarpitted() {
    let fixture = PasswordFixture::new();

    assert_tarpitted_auth_failure(
        fixture.authenticate("brew-operator", "wrong-password"),
        "invalid username password",
    )
    .await;
}

#[tokio::test]
async fn oauth2_rsa_oaep_encrypted_secret_authenticates() {
    let fixture = OAuth2Fixture::new();
    let token = format!(
        "Bearer {}",
        signed_jwt(
            json!({
                "iss": OAUTH2_ISSUER,
                "aud": OAUTH2_AUDIENCE,
                "exp": future_expiration(),
                "sub": "brew-operator"
            }),
            &fixture.private_key,
        )
    );
    let policy = SecurityPolicy::Aes128Sha256RsaOaep;
    let encrypted_token = rsa_oaep_encrypt(policy, &fixture.server_private_key(), token.as_bytes());
    let encryption_algorithm = UAString::from(
        policy
            .asymmetric_encryption_algorithm()
            .expect("Aes128Sha256RsaOaep should define RSA-OAEP encryption"),
    );

    let user_token = fixture
        .authenticate_encrypted_token(encrypted_token, encryption_algorithm)
        .await
        .expect("valid encrypted OAuth2 token should authenticate");

    assert_eq!(user_token.0, "brew-operator");
}

#[tokio::test(flavor = "current_thread")]
async fn encrypted_secret_decryption_failure_is_tarpitted() {
    let fixture = OAuth2Fixture::new();
    let invalid_ciphertext = ByteString::from(vec![0xa5; 256]);
    let encryption_algorithm = UAString::from(
        SecurityPolicy::Aes128Sha256RsaOaep
            .asymmetric_encryption_algorithm()
            .expect("Aes128Sha256RsaOaep should define RSA-OAEP encryption"),
    );

    assert_tarpitted_auth_failure(
        fixture.authenticate_encrypted_token(invalid_ciphertext, encryption_algorithm),
        "decryption failure",
    )
    .await;
}
