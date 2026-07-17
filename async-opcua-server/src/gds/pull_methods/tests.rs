use opcua_core::sync::RwLock;
use opcua_crypto::SecurityPolicy;
use opcua_types::{AnonymousIdentityToken, ApplicationDescription, MessageSecurityMode, UAString};

use crate::{
    address_space::AddressSpace, authenticator::UserToken,
    gds::directory_instance::instantiate_certificate_directory, identity_token::IdentityToken,
    node_manager::RequestContextInner, session::instance::Session, ServerBuilder,
};

use super::*;

fn xml_present() -> bool {
    std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../schemas/companion/GDS/Opc.Ua.Gds.NodeSet2.xml"
    ))
    .exists()
}

fn unique_test_pki_dir() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    tempfile::Builder::new()
        .prefix(&format!(
            "async-opcua-gds-pull-test-pki-{}-{id}-",
            std::process::id()
        ))
        .tempdir()
        .expect("failed to create a securely-permissioned test PKI directory")
        .keep()
}

fn request_context(
    security_mode: MessageSecurityMode,
    user_roles: Vec<NodeId>,
) -> (RequestContext, crate::ServerHandle) {
    let (_server, handle) = ServerBuilder::new_anonymous("gds pull method test")
        .without_node_managers()
        .pki_dir(unique_test_pki_dir())
        .create_sample_keypair(true)
        .build()
        .expect("test server should build");
    let info = Arc::clone(handle.info());
    let user_roles = Arc::new(user_roles);
    let session = Arc::new(RwLock::new(Session::create(
        &info,
        NodeId::new(0, 1),
        1,
        60_000,
        0,
        0,
        UAString::from("opc.tcp://localhost"),
        SecurityPolicy::Basic256Sha256.to_uri().to_string(),
        IdentityToken::Anonymous(AnonymousIdentityToken {
            policy_id: UAString::from("anonymous"),
        }),
        None,
        ByteString::null(),
        UAString::from("gds-pull-method-test"),
        ApplicationDescription::default(),
        security_mode,
    )));

    let context = RequestContext::new_test(Arc::new(RequestContextInner {
        session,
        session_id: 1,
        authenticator: info.authenticator.clone(),
        token: UserToken("gds-pull-method-test".to_string()),
        user_roles,
        type_tree: info.type_tree.clone(),
        type_tree_getter: info.type_tree_getter.clone(),
        subscriptions: handle.subscriptions().clone(),
        info,
    }));
    (context, handle)
}

fn security_admin_request_context(
    security_mode: MessageSecurityMode,
) -> (RequestContext, crate::ServerHandle) {
    request_context(security_mode, vec![WellKnownRole::SecurityAdmin.node_id()])
}

fn handler_with_directory() -> Option<(GdsPullMethodHandler, DirectoryInstanceNodeIds)> {
    if !xml_present() {
        eprintln!("skipping: schemas/companion/GDS/Opc.Ua.Gds.NodeSet2.xml not present locally");
        return None;
    }
    let address_space = AddressSpace::new();
    let rw = RwLock::new(address_space);
    crate::companion::import_gds(&rw);
    let directory = {
        let guard = rw.read();
        instantiate_certificate_directory(&guard)
    }
    .expect("companion XML is present, instantiation should succeed");

    let registry = GdsPullMethodRegistry::default();
    let handler = GdsPullMethodHandler::new(registry, directory.clone());
    Some((handler, directory))
}

fn signing_request_args(application_id: NodeId, csr_der: Vec<u8>) -> Vec<Variant> {
    vec![
        Variant::from(application_id),
        Variant::from(NodeId::null()),
        Variant::from(NodeId::null()),
        Variant::from(ByteString::from(csr_der)),
    ]
}

fn new_key_pair_args(application_id: NodeId) -> Vec<Variant> {
    vec![
        Variant::from(application_id),
        Variant::from(NodeId::null()),
        Variant::from(NodeId::null()),
        Variant::from(UAString::null()),
        Variant::Empty,
        Variant::from(UAString::null()),
        Variant::from(UAString::null()),
    ]
}

#[tokio::test]
async fn start_new_key_pair_request_then_finish_request_returns_real_certificate_and_key() {
    let Some((handler, _directory)) = handler_with_directory() else {
        return;
    };
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
    let application_id = handler
        .registry()
        .register_application("urn:test:pull-application", NodeId::new(1, "group"));

    let outputs = handler
        .handle_start_new_key_pair_request(&context, &new_key_pair_args(application_id.clone()))
        .expect("start new key pair request should succeed");
    let Variant::NodeId(request_id) = &outputs[0] else {
        panic!("expected NodeId output");
    };

    let finish_args = vec![
        Variant::from(application_id),
        Variant::from(request_id.as_ref().clone()),
    ];
    let finished = handler
        .handle_finish_request(&context, &finish_args)
        .expect("finish request should succeed");

    let Variant::ByteString(cert) = &finished[0] else {
        panic!("expected ByteString certificate");
    };
    let cert_der = cert.value.as_ref().expect("certificate should not be null");
    let issued = X509::from_der(cert_der).expect("issued certificate should parse");
    assert!(!issued.is_self_signed());
    assert!(issued
        .is_application_uri_valid("urn:test:pull-application")
        .is_ok());

    let Variant::ByteString(pkey) = &finished[1] else {
        panic!("expected ByteString private key");
    };
    assert!(!pkey.is_null_or_empty(), "private key should be present");
}

#[tokio::test]
async fn start_signing_request_with_matching_csr_returns_certificate_without_private_key() {
    let Some((handler, _directory)) = handler_with_directory() else {
        return;
    };
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
    let application_id = handler
        .registry()
        .register_application("urn:test:csr-application", NodeId::new(1, "group"));

    let requester_pkey =
        opcua_crypto::PrivateKey::new(2048).expect("requester key pair should generate");
    let csr_der =
        X509::create_signing_request(&requester_pkey, "CN=requester", "urn:test:csr-application")
            .expect("CSR should build");

    let outputs = handler
        .handle_start_signing_request(
            &context,
            &signing_request_args(application_id.clone(), csr_der),
        )
        .expect("start signing request should succeed");
    let Variant::NodeId(request_id) = &outputs[0] else {
        panic!("expected NodeId output");
    };

    let finish_args = vec![
        Variant::from(application_id),
        Variant::from(request_id.as_ref().clone()),
    ];
    let finished = handler
        .handle_finish_request(&context, &finish_args)
        .expect("finish request should succeed");

    let Variant::ByteString(cert) = &finished[0] else {
        panic!("expected ByteString certificate");
    };
    assert!(!cert.is_null_or_empty());

    let Variant::ByteString(pkey) = &finished[1] else {
        panic!("expected ByteString private key output slot");
    };
    assert!(
        pkey.is_null_or_empty(),
        "StartSigningRequest must not return a private key"
    );
}

#[tokio::test]
async fn finish_request_on_a_pending_request_reports_nothing_to_do() {
    let Some((handler, _directory)) = handler_with_directory() else {
        return;
    };
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
    let application_id = handler
        .registry()
        .register_application("urn:test:pending-application", NodeId::new(1, "group"));
    let request_id = handler
        .registry()
        .stage_pending_request(application_id.namespace, application_id.clone());

    let finish_args = vec![Variant::from(application_id), Variant::from(request_id)];

    assert_eq!(
        handler.handle_finish_request(&context, &finish_args),
        Err(StatusCode::BadNothingToDo)
    );
}

#[tokio::test]
async fn finish_request_with_unknown_request_id_reports_invalid_argument() {
    let Some((handler, _directory)) = handler_with_directory() else {
        return;
    };
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
    let application_id = handler
        .registry()
        .register_application("urn:test:unknown-request", NodeId::new(1, "group"));

    let finish_args = vec![
        Variant::from(application_id),
        Variant::from(NodeId::new(1, "does-not-exist")),
    ];

    assert_eq!(
        handler.handle_finish_request(&context, &finish_args),
        Err(StatusCode::BadInvalidArgument)
    );
}

#[tokio::test]
async fn methods_reject_unregistered_application() {
    let Some((handler, _directory)) = handler_with_directory() else {
        return;
    };
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
    let unknown_application_id = NodeId::new(1, "unregistered");

    assert_eq!(
        handler.handle_start_new_key_pair_request(
            &context,
            &new_key_pair_args(unknown_application_id.clone())
        ),
        Err(StatusCode::BadNotFound)
    );
    assert_eq!(
        handler.handle_get_certificate_groups(&context, &[Variant::from(unknown_application_id)]),
        Err(StatusCode::BadNotFound)
    );
}

#[tokio::test]
async fn methods_require_security_admin() {
    let Some((handler, _directory)) = handler_with_directory() else {
        return;
    };
    let (context, _handle) = request_context(
        MessageSecurityMode::SignAndEncrypt,
        vec![WellKnownRole::AuthenticatedUser.node_id()],
    );

    assert_eq!(
        handler.handle_start_new_key_pair_request(&context, &new_key_pair_args(NodeId::null())),
        Err(StatusCode::BadUserAccessDenied)
    );
}

#[tokio::test]
async fn start_new_key_pair_request_requires_encrypted_channel() {
    let Some((handler, _directory)) = handler_with_directory() else {
        return;
    };
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::Sign);

    assert_eq!(
        handler.handle_start_new_key_pair_request(&context, &new_key_pair_args(NodeId::null())),
        Err(StatusCode::BadSecurityModeInsufficient)
    );
}

#[tokio::test]
async fn get_certificate_groups_returns_the_real_default_application_group() {
    let Some((handler, directory)) = handler_with_directory() else {
        return;
    };
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
    let application_id = handler.registry().register_application(
        "urn:test:groups-application",
        directory.default_application_group_id.clone(),
    );

    let outputs = handler
        .handle_get_certificate_groups(&context, &[Variant::from(application_id)])
        .expect("get certificate groups should succeed");
    let Variant::Array(array) = &outputs[0] else {
        panic!("expected array output");
    };
    assert_eq!(array.values.len(), 1);
    assert_eq!(
        array.values[0],
        Variant::from(directory.default_application_group_id.clone())
    );
}

#[tokio::test]
async fn get_trust_list_returns_the_real_trust_list_node_id() {
    let Some((handler, directory)) = handler_with_directory() else {
        return;
    };
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
    let application_id = handler.registry().register_application(
        "urn:test:trustlist-application",
        directory.default_application_group_id.clone(),
    );

    let outputs = handler
        .handle_get_trust_list(
            &context,
            &[
                Variant::from(application_id),
                Variant::from(directory.default_application_group_id.clone()),
            ],
        )
        .expect("get trust list should succeed");

    assert_eq!(
        outputs[0],
        Variant::from(directory.default_application_group_trust_list_id.clone())
    );
}

#[tokio::test]
async fn get_certificate_status_reports_update_not_required() {
    let Some((handler, directory)) = handler_with_directory() else {
        return;
    };
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
    let application_id = handler.registry().register_application(
        "urn:test:status-application",
        directory.default_application_group_id.clone(),
    );

    let outputs = handler
        .handle_get_certificate_status(
            &context,
            &[
                Variant::from(application_id),
                Variant::from(NodeId::null()),
                Variant::from(NodeId::null()),
            ],
        )
        .expect("get certificate status should succeed");

    assert_eq!(outputs[0], Variant::from(false));
}
