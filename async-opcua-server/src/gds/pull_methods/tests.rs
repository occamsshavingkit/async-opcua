use opcua_core::sync::RwLock;
use opcua_crypto::SecurityPolicy;
use opcua_types::{
    AnonymousIdentityToken, ApplicationDescription, ExtensionObject, MessageSecurityMode, UAString,
};

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

fn no_role_request_context() -> (RequestContext, crate::ServerHandle) {
    request_context(MessageSecurityMode::None, vec![])
}

#[allow(clippy::too_many_arguments)]
fn application_record(
    application_id: NodeId,
    uri: &str,
    name: &str,
    product_uri: &str,
    discovery_urls: &[&str],
    capabilities: &[&str],
    application_type: ApplicationType,
) -> ApplicationRecordDataType {
    ApplicationRecordDataType {
        application_id,
        application_uri: UAString::from(uri),
        application_type,
        application_names: Some(vec![LocalizedText::from(name)]),
        product_uri: UAString::from(product_uri),
        discovery_urls: (!discovery_urls.is_empty())
            .then(|| discovery_urls.iter().map(|u| UAString::from(*u)).collect()),
        server_capabilities: (!capabilities.is_empty())
            .then(|| capabilities.iter().map(|c| UAString::from(*c)).collect()),
    }
}

fn record_arg(record: ApplicationRecordDataType) -> Variant {
    Variant::from(ExtensionObject::new(record))
}

fn query_applications_args(
    starting_record_id: u32,
    max_records_to_return: u32,
    application_name: &str,
    application_uri: &str,
    application_type_mask: u32,
    product_uri: &str,
    capabilities: &[&str],
) -> Vec<Variant> {
    vec![
        Variant::from(starting_record_id),
        Variant::from(max_records_to_return),
        Variant::from(application_name),
        Variant::from(application_uri),
        Variant::from(application_type_mask),
        Variant::from(product_uri),
        string_array_variant(capabilities),
    ]
}

fn string_array_variant(values: &[&str]) -> Variant {
    let array = Array::new(
        opcua_types::VariantScalarTypeId::String,
        values.iter().map(|v| Variant::from(*v)).collect::<Vec<_>>(),
    )
    .expect("string array should construct");
    Variant::Array(Box::new(array))
}

#[tokio::test]
async fn register_application_assigns_id_and_is_retrievable() {
    let Some((handler, _directory)) = handler_with_directory() else {
        return;
    };
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);

    let record = application_record(
        NodeId::null(),
        "urn:test:register-app",
        "Register App",
        "urn:test:products:register-app",
        &["opc.tcp://localhost:4840"],
        &["DA"],
        ApplicationType::Server,
    );
    let outputs = handler
        .handle_register_application(&context, &[record_arg(record)])
        .expect("register application should succeed");
    let Variant::NodeId(application_id) = &outputs[0] else {
        panic!("expected NodeId output");
    };
    assert!(!application_id.is_null());

    let (read_context, _handle2) = no_role_request_context();
    let get_outputs = handler
        .handle_get_application(
            &read_context,
            &[Variant::from(application_id.as_ref().clone())],
        )
        .expect("get application should succeed");
    let Variant::ExtensionObject(obj) = &get_outputs[0] else {
        panic!("expected ExtensionObject output");
    };
    let decoded = obj
        .clone()
        .into_inner_as::<ApplicationRecordDataType>()
        .expect("should decode as ApplicationRecordDataType");
    assert_eq!(
        decoded.application_uri,
        UAString::from("urn:test:register-app")
    );
}

#[tokio::test]
async fn register_application_rejects_duplicate_uri() {
    let Some((handler, _directory)) = handler_with_directory() else {
        return;
    };
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);

    let first = application_record(
        NodeId::null(),
        "urn:test:duplicate-app",
        "First",
        "urn:test:products:first",
        &[],
        &[],
        ApplicationType::Server,
    );
    handler
        .handle_register_application(&context, &[record_arg(first)])
        .expect("first registration should succeed");

    let second = application_record(
        NodeId::null(),
        "urn:test:duplicate-app",
        "Second",
        "urn:test:products:second",
        &[],
        &[],
        ApplicationType::Server,
    );
    assert_eq!(
        handler.handle_register_application(&context, &[record_arg(second)]),
        Err(StatusCode::BadEntryExists)
    );
}

#[tokio::test]
async fn register_application_requires_security_admin() {
    let Some((handler, _directory)) = handler_with_directory() else {
        return;
    };
    let (context, _handle) = request_context(
        MessageSecurityMode::SignAndEncrypt,
        vec![WellKnownRole::AuthenticatedUser.node_id()],
    );

    let record = application_record(
        NodeId::null(),
        "urn:test:denied-app",
        "Denied",
        "urn:test:products:denied",
        &[],
        &[],
        ApplicationType::Server,
    );
    assert_eq!(
        handler.handle_register_application(&context, &[record_arg(record)]),
        Err(StatusCode::BadUserAccessDenied)
    );
}

#[tokio::test]
async fn update_application_changes_fields_but_rejects_uri_change() {
    let Some((handler, _directory)) = handler_with_directory() else {
        return;
    };
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);

    let record = application_record(
        NodeId::null(),
        "urn:test:update-app",
        "Before",
        "urn:test:products:update-app",
        &[],
        &[],
        ApplicationType::Server,
    );
    let outputs = handler
        .handle_register_application(&context, &[record_arg(record)])
        .expect("register application should succeed");
    let Variant::NodeId(application_id) = &outputs[0] else {
        panic!("expected NodeId output");
    };

    let updated = application_record(
        application_id.as_ref().clone(),
        "urn:test:update-app",
        "After",
        "urn:test:products:update-app",
        &["opc.tcp://localhost:4840"],
        &[],
        ApplicationType::Server,
    );
    handler
        .handle_update_application(&context, &[record_arg(updated)])
        .expect("update with unchanged uri should succeed");

    let (read_context, _handle2) = no_role_request_context();
    let get_outputs = handler
        .handle_get_application(
            &read_context,
            &[Variant::from(application_id.as_ref().clone())],
        )
        .expect("get application should succeed");
    let Variant::ExtensionObject(obj) = &get_outputs[0] else {
        panic!("expected ExtensionObject output");
    };
    let decoded = obj
        .clone()
        .into_inner_as::<ApplicationRecordDataType>()
        .expect("should decode as ApplicationRecordDataType");
    assert_eq!(
        decoded.application_names.unwrap()[0].text,
        UAString::from("After")
    );

    let changed_uri = application_record(
        application_id.as_ref().clone(),
        "urn:test:update-app-changed",
        "After",
        "urn:test:products:update-app",
        &[],
        &[],
        ApplicationType::Server,
    );
    assert_eq!(
        handler.handle_update_application(&context, &[record_arg(changed_uri)]),
        Err(StatusCode::BadWriteNotSupported)
    );
}

#[tokio::test]
async fn update_application_unknown_id_reports_not_found() {
    let Some((handler, _directory)) = handler_with_directory() else {
        return;
    };
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);

    let record = application_record(
        NodeId::new(1, "does-not-exist"),
        "urn:test:missing-update",
        "Missing",
        "urn:test:products:missing",
        &[],
        &[],
        ApplicationType::Server,
    );
    assert_eq!(
        handler.handle_update_application(&context, &[record_arg(record)]),
        Err(StatusCode::BadNotFound)
    );
}

#[tokio::test]
async fn unregister_application_removes_record() {
    let Some((handler, _directory)) = handler_with_directory() else {
        return;
    };
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);

    let record = application_record(
        NodeId::null(),
        "urn:test:unregister-app",
        "Unregister",
        "urn:test:products:unregister-app",
        &[],
        &[],
        ApplicationType::Server,
    );
    let outputs = handler
        .handle_register_application(&context, &[record_arg(record)])
        .expect("register application should succeed");
    let Variant::NodeId(application_id) = &outputs[0] else {
        panic!("expected NodeId output");
    };

    handler
        .handle_unregister_application(&context, &[Variant::from(application_id.as_ref().clone())])
        .expect("unregister should succeed");

    let (read_context, _handle2) = no_role_request_context();
    assert_eq!(
        handler.handle_get_application(
            &read_context,
            &[Variant::from(application_id.as_ref().clone())]
        ),
        Err(StatusCode::BadNotFound)
    );
}

#[tokio::test]
async fn unregister_application_unknown_id_reports_not_found() {
    let Some((handler, _directory)) = handler_with_directory() else {
        return;
    };
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);

    assert_eq!(
        handler.handle_unregister_application(
            &context,
            &[Variant::from(NodeId::new(1, "does-not-exist"))]
        ),
        Err(StatusCode::BadNotFound)
    );
}

#[tokio::test]
async fn get_application_unknown_id_reports_not_found() {
    let Some((handler, _directory)) = handler_with_directory() else {
        return;
    };
    let (context, _handle) = no_role_request_context();

    assert_eq!(
        handler
            .handle_get_application(&context, &[Variant::from(NodeId::new(1, "does-not-exist"))]),
        Err(StatusCode::BadNotFound)
    );
}

#[tokio::test]
async fn find_applications_exact_match_returns_single_result() {
    let Some((handler, _directory)) = handler_with_directory() else {
        return;
    };
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);

    let record = application_record(
        NodeId::null(),
        "urn:test:find-app",
        "Find",
        "urn:test:products:find-app",
        &[],
        &[],
        ApplicationType::Server,
    );
    handler
        .handle_register_application(&context, &[record_arg(record)])
        .expect("register application should succeed");

    let (read_context, _handle2) = no_role_request_context();
    let found = handler
        .handle_find_applications(&read_context, &[Variant::from("urn:test:find-app")])
        .expect("find applications should succeed");
    let Variant::Array(array) = &found[0] else {
        panic!("expected array output");
    };
    assert_eq!(array.values.len(), 1);

    // FindApplications does exact matching, unlike QueryApplications' LIKE-based filters.
    let not_found = handler
        .handle_find_applications(&read_context, &[Variant::from("urn:test:find-a")])
        .expect("find applications should succeed even with no match");
    let Variant::Array(empty_array) = &not_found[0] else {
        panic!("expected array output");
    };
    assert_eq!(empty_array.values.len(), 0);
}

#[tokio::test]
async fn query_applications_filters_by_name_uri_type_and_excludes_na_capability() {
    let Some((handler, _directory)) = handler_with_directory() else {
        return;
    };
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);

    for (uri, name, app_type, capabilities) in [
        (
            "urn:test:query-server",
            "QueryServerApp",
            ApplicationType::Server,
            &["DA"][..],
        ),
        (
            "urn:test:query-client",
            "QueryClientApp",
            ApplicationType::Client,
            &[][..],
        ),
        (
            "urn:test:query-na",
            "QueryNaApp",
            ApplicationType::Server,
            &["NA"][..],
        ),
    ] {
        let record = application_record(
            NodeId::null(),
            uri,
            name,
            "urn:test:products:query",
            &[],
            capabilities,
            app_type,
        );
        handler
            .handle_register_application(&context, &[record_arg(record)])
            .expect("register application should succeed");
    }

    let (read_context, _handle2) = no_role_request_context();
    let outputs = handler
        .handle_query_applications(
            &read_context,
            &query_applications_args(0, 0, "Query%", "", QUERY_APPLICATION_TYPE_SERVERS, "", &[]),
        )
        .expect("query applications should succeed");
    let Variant::Array(array) = &outputs[2] else {
        panic!("expected array output");
    };
    // Only "QueryServerApp" should match: NA-tagged records are always excluded, and the
    // ApplicationType mask excludes the Client record.
    assert_eq!(array.values.len(), 1);
}

#[tokio::test]
async fn query_applications_paginates_with_starting_record_id_and_max_records() {
    let Some((handler, _directory)) = handler_with_directory() else {
        return;
    };
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);

    for i in 0..3 {
        let record = application_record(
            NodeId::null(),
            &format!("urn:test:page-app-{i}"),
            &format!("PageApp{i}"),
            "urn:test:products:page",
            &[],
            &[],
            ApplicationType::Server,
        );
        handler
            .handle_register_application(&context, &[record_arg(record)])
            .expect("register application should succeed");
    }

    let (read_context, _handle2) = no_role_request_context();
    let first_page = handler
        .handle_query_applications(
            &read_context,
            &query_applications_args(0, 2, "", "", 0, "", &[]),
        )
        .expect("query applications should succeed");
    let Variant::Array(first_array) = &first_page[2] else {
        panic!("expected array output");
    };
    assert_eq!(first_array.values.len(), 2);
    let Variant::UInt32(next_record_id) = &first_page[1] else {
        panic!("expected UInt32 NextRecordId output");
    };
    let next_record_id = *next_record_id;
    assert_ne!(next_record_id, 0, "a further page should be signalled");

    let second_page = handler
        .handle_query_applications(
            &read_context,
            &query_applications_args(next_record_id, 0, "", "", 0, "", &[]),
        )
        .expect("query applications should succeed");
    let Variant::Array(second_array) = &second_page[2] else {
        panic!("expected array output");
    };
    assert_eq!(second_array.values.len(), 1);
}

#[tokio::test]
async fn query_servers_emits_one_row_per_discovery_url() {
    let Some((handler, _directory)) = handler_with_directory() else {
        return;
    };
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);

    let record = application_record(
        NodeId::null(),
        "urn:test:multi-url-app",
        "MultiUrl",
        "urn:test:products:multi-url",
        &["opc.tcp://host-a:4840", "opc.tcp://host-b:4840"],
        &[],
        ApplicationType::Server,
    );
    handler
        .handle_register_application(&context, &[record_arg(record)])
        .expect("register application should succeed");

    let (read_context, _handle2) = no_role_request_context();
    let outputs = handler
        .handle_query_servers(
            &read_context,
            &[
                Variant::from(0u32),
                Variant::from(0u32),
                Variant::from(""),
                Variant::from(""),
                Variant::from(""),
                string_array_variant(&[]),
            ],
        )
        .expect("query servers should succeed");
    let Variant::Array(array) = &outputs[1] else {
        panic!("expected array output");
    };
    assert_eq!(
        array.values.len(),
        2,
        "one ServerOnNetwork row per discovery URL"
    );
}

#[tokio::test]
async fn query_servers_caps_rows_after_expanding_discovery_urls_not_before() {
    let Some((handler, _directory)) = handler_with_directory() else {
        return;
    };
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);

    // A single application with 2 discovery URLs expands to 2 ServerOnNetwork rows -- if
    // MaxRecordsToReturn=1 truncated the *application* list before expansion (the bug), this
    // would still return 2 rows instead of 1.
    let record = application_record(
        NodeId::null(),
        "urn:test:cap-after-expand-app",
        "CapAfterExpand",
        "urn:test:products:cap-after-expand",
        &["opc.tcp://host-a:4840", "opc.tcp://host-b:4840"],
        &[],
        ApplicationType::Server,
    );
    handler
        .handle_register_application(&context, &[record_arg(record)])
        .expect("register application should succeed");

    let (read_context, _handle2) = no_role_request_context();
    let outputs = handler
        .handle_query_servers(
            &read_context,
            &[
                Variant::from(0u32),
                Variant::from(1u32),
                Variant::from(""),
                Variant::from(""),
                Variant::from(""),
                string_array_variant(&[]),
            ],
        )
        .expect("query servers should succeed");
    let Variant::Array(array) = &outputs[1] else {
        panic!("expected array output");
    };
    assert_eq!(
        array.values.len(),
        1,
        "MaxRecordsToReturn=1 must cap the row count, not the underlying record count"
    );
}

#[tokio::test]
async fn query_applications_rejects_non_string_capability_array_elements() {
    let Some((handler, _directory)) = handler_with_directory() else {
        return;
    };
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);

    let malformed_capabilities = Variant::Array(Box::new(
        Array::new(
            opcua_types::VariantScalarTypeId::UInt32,
            vec![Variant::from(1u32)],
        )
        .expect("array should construct"),
    ));
    let args = vec![
        Variant::from(0u32),
        Variant::from(0u32),
        Variant::from(""),
        Variant::from(""),
        Variant::from(0u32),
        Variant::from(""),
        malformed_capabilities,
    ];
    assert_eq!(
        handler.handle_query_applications(&context, &args),
        Err(StatusCode::BadTypeMismatch)
    );
}
