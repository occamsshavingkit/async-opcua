use opcua_core::sync::RwLock;
use opcua_crypto::{CertificateGroup, SecurityPolicy, X509Data};
use opcua_types::{AnonymousIdentityToken, ApplicationDescription, MessageSecurityMode, UAString};

use crate::{
    authenticator::UserToken, identity_token::IdentityToken, node_manager::RequestContextInner,
    rbac::WellKnownRole, session::instance::Session, ServerBuilder,
};

use super::*;

#[test]
fn method_ids_match_the_verified_standard_nodeset() {
    assert_eq!(trust_list_object_id(), NodeId::new(0, 12642));
    assert_eq!(open_method_id(), NodeId::new(0, 12647));
    assert_eq!(close_method_id(), NodeId::new(0, 12650));
    assert_eq!(read_method_id(), NodeId::new(0, 12652));
    assert_eq!(write_method_id(), NodeId::new(0, 12655));
    assert_eq!(get_position_method_id(), NodeId::new(0, 12657));
    assert_eq!(set_position_method_id(), NodeId::new(0, 12660));
    assert_eq!(open_with_masks_method_id(), NodeId::new(0, 12663));
    assert_eq!(close_and_update_method_id(), NodeId::new(0, 12666));
    assert_eq!(add_certificate_method_id(), NodeId::new(0, 12668));
    assert_eq!(remove_certificate_method_id(), NodeId::new(0, 12670));

    assert_eq!(
        TrustListNodeIds::for_group(CertificateGroup::DefaultHttps),
        TrustListNodeIds {
            object: 14089,
            open: 14095,
            close: 14098,
            read: 14100,
            write: 14103,
            get_position: 14105,
            set_position: 14108,
            open_with_masks: 14111,
            close_and_update: 14114,
            add_certificate: 14117,
            remove_certificate: 14119,
        }
    );
    assert_eq!(
        TrustListNodeIds::for_group(CertificateGroup::DefaultUserToken),
        TrustListNodeIds {
            object: 14123,
            open: 14129,
            close: 14132,
            read: 14134,
            write: 14137,
            get_position: 14139,
            set_position: 14142,
            open_with_masks: 14145,
            close_and_update: 14148,
            add_certificate: 14151,
            remove_certificate: 14153,
        }
    );
}

fn unique_test_pki_dir() -> std::path::PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    tempfile::Builder::new()
        .prefix(&format!(
            "async-opcua-gds-trust-list-test-pki-{}-{id}-",
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
    let (_server, handle) = ServerBuilder::new_anonymous("trust list method test")
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
        UAString::from("trust-list-method-test"),
        ApplicationDescription::default(),
        security_mode,
    )));

    let context = RequestContext::new_test(Arc::new(RequestContextInner {
        session,
        session_id: 1,
        authenticator: info.authenticator.clone(),
        token: UserToken("trust-list-method-test".to_string()),
        user_roles,
        type_tree: info.type_tree.clone(),
        type_tree_getter: info.type_tree_getter.clone(),
        subscriptions: handle.subscriptions().clone(),
        info,
    }));
    (context, handle)
}

pub(super) fn security_admin_request_context(
    security_mode: MessageSecurityMode,
) -> (RequestContext, crate::ServerHandle) {
    request_context(security_mode, vec![WellKnownRole::SecurityAdmin.node_id()])
}

fn self_signed_cert_with_cn(cn: &str) -> X509 {
    let data = X509Data {
        key_size: 2048,
        common_name: cn.to_owned(),
        organization: "async-opcua tests".to_owned(),
        organizational_unit: String::new(),
        country: "IE".to_owned(),
        state: String::new(),
        alt_host_names: opcua_crypto::AlternateNames::new(),
        certificate_duration_days: 365,
    };
    let (cert, _pkey) = X509::cert_and_pkey(&data).expect("cert generation should succeed");
    cert
}

pub(super) fn handler(push_registry: Arc<GdsPushRegistry>) -> TrustListMethodHandler {
    TrustListMethodHandler::new(push_registry)
}

fn handler_for_group(
    push_registry: Arc<GdsPushRegistry>,
    certificate_group: CertificateGroup,
) -> TrustListMethodHandler {
    TrustListMethodHandler::new_for_group(push_registry, certificate_group)
}

#[tokio::test]
async fn open_read_then_read_returns_the_actual_trust_list() {
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
    let cert = self_signed_cert_with_cn("trusted-fixture");
    context
        .info
        .certificate_store
        .read()
        .store_trusted_cert(&cert)
        .expect("fixture cert should store");
    let h = handler(Arc::new(GdsPushRegistry::default()));

    let open_out = h
        .handle_open(&context, &[Variant::from(open_mode::READ)])
        .expect("open should succeed");
    let Variant::UInt32(file_handle) = open_out[0] else {
        panic!("expected UInt32 file handle");
    };

    let read_out = h
        .handle_read(
            &context,
            &[Variant::from(file_handle), Variant::from(i32::MAX)],
        )
        .expect("read should succeed");
    let Variant::ByteString(data) = &read_out[0] else {
        panic!("expected ByteString data");
    };
    let bytes = data.value.as_ref().expect("data should not be null");
    let decoded = decode_trust_list(bytes).expect("should decode as TrustListDataType");
    let trusted = decoded
        .trusted_certificates
        .expect("trusted_certificates should be present");
    assert_eq!(trusted.len(), 1);

    h.handle_close(&context, &[Variant::from(file_handle)])
        .expect("close should succeed");
}

#[tokio::test]
async fn open_with_masks_returns_only_the_requested_subset() {
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
    let cert = self_signed_cert_with_cn("trusted-fixture");
    context
        .info
        .certificate_store
        .read()
        .store_trusted_cert(&cert)
        .expect("fixture cert should store");
    let h = handler(Arc::new(GdsPushRegistry::default()));

    let open_out = h
        .handle_open_with_masks(&context, &[Variant::from(masks::TRUSTED_CERTIFICATES)])
        .expect("open with masks should succeed");
    let Variant::UInt32(file_handle) = open_out[0] else {
        panic!("expected UInt32 file handle");
    };
    let read_out = h
        .handle_read(
            &context,
            &[Variant::from(file_handle), Variant::from(i32::MAX)],
        )
        .expect("read should succeed");
    let Variant::ByteString(data) = &read_out[0] else {
        panic!("expected ByteString data");
    };
    let bytes = data.value.as_ref().expect("data should not be null");
    let decoded = decode_trust_list(bytes).expect("should decode");

    assert!(decoded.trusted_certificates.is_some());
    assert!(decoded.trusted_crls.is_none());
    assert!(decoded.issuer_certificates.is_none());
    assert!(decoded.issuer_crls.is_none());
}

#[tokio::test]
async fn open_write_write_close_and_update_stages_pending_change_without_mutating_store() {
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
    let registry = Arc::new(GdsPushRegistry::default());
    let h = handler(registry.clone());

    let new_cert = self_signed_cert_with_cn("newly-trusted");
    let trust_list = TrustListDataType {
        specified_lists: masks::TRUSTED_CERTIFICATES,
        trusted_certificates: Some(vec![ByteString::from(
            new_cert.to_der().expect("cert should encode"),
        )]),
        ..Default::default()
    };
    let payload = encode_trust_list(&trust_list).expect("should encode");

    let open_out = h
        .handle_open(
            &context,
            &[Variant::from(open_mode::WRITE | open_mode::ERASE_EXISTING)],
        )
        .expect("open for write should succeed");
    let Variant::UInt32(file_handle) = open_out[0] else {
        panic!("expected UInt32 file handle");
    };

    h.handle_write(
        &context,
        &[
            Variant::from(file_handle),
            Variant::from(ByteString::from(payload)),
        ],
    )
    .expect("write should succeed");

    let outputs = h
        .handle_close_and_update(&context, &[Variant::from(file_handle)])
        .expect("close and update should succeed");
    assert_eq!(outputs, vec![Variant::from(true)]);

    // Not yet applied.
    assert!(context
        .info
        .certificate_store
        .read()
        .read_trusted_certs()
        .is_empty());
    assert!(registry.transaction.read().staged().is_some());
}

#[tokio::test]
async fn close_and_update_during_apply_stages_fresh_replacement_without_cloning_current() {
    // Given: this session owns an applying transaction containing every optional field and a
    // pending TrustList for a different certificate group.
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
    let registry = Arc::new(GdsPushRegistry::default());
    let h = handler(registry.clone());
    let current_certificate_der = vec![1, 2, 3];
    let current_private_key_pem = vec![4, 5, 6];
    let current_certificate_group_id = NodeId::new(0, 12555);
    let current_certificate_type_id = NodeId::new(0, 12560);
    let applying_trust_list = TrustListDataType {
        specified_lists: masks::TRUSTED_CRLS,
        trusted_crls: Some(vec![ByteString::from(vec![7, 8, 9])]),
        ..Default::default()
    };
    *registry.transaction.write() = PushTransactionState::Applying {
        current: PushTransaction {
            owning_session_id: context.session_id(),
            certificate_der: Some(current_certificate_der.clone()),
            private_key_pem: Some(current_private_key_pem.clone()),
            certificate_group_id: Some(current_certificate_group_id.clone()),
            certificate_type_id: Some(current_certificate_type_id.clone()),
            pending_trust_lists: std::collections::HashMap::from([(
                CertificateGroup::DefaultHttps,
                applying_trust_list.clone(),
            )]),
        },
        replacement: None,
    };
    let new_cert = self_signed_cert_with_cn("replacement-trusted");
    let replacement_trust_list = TrustListDataType {
        specified_lists: masks::TRUSTED_CERTIFICATES,
        trusted_certificates: Some(vec![ByteString::from(
            new_cert.to_der().expect("cert should encode"),
        )]),
        ..Default::default()
    };
    let payload = encode_trust_list(&replacement_trust_list).expect("should encode");

    // When: the owning session uploads the new TrustList through Open, Write, and
    // CloseAndUpdate while ApplyChanges is active.
    let open_out = h
        .handle_open(
            &context,
            &[Variant::from(open_mode::WRITE | open_mode::ERASE_EXISTING)],
        )
        .expect("open for write should succeed");
    let Variant::UInt32(file_handle) = open_out[0] else {
        panic!("expected UInt32 file handle");
    };
    h.handle_write(
        &context,
        &[
            Variant::from(file_handle),
            Variant::from(ByteString::from(payload)),
        ],
    )
    .expect("write should succeed");
    assert_eq!(
        h.handle_close_and_update(&context, &[Variant::from(file_handle)]),
        Ok(vec![Variant::from(true)])
    );

    // Then: the applying transaction is unchanged and its replacement contains exactly the new
    // TrustList, with none of the fields or TrustLists already being committed.
    let transaction = registry.transaction.read();
    let PushTransactionState::Applying {
        current,
        replacement: Some(replacement),
    } = &*transaction
    else {
        panic!("expected applying transaction with replacement");
    };
    assert_eq!(current.owning_session_id, context.session_id());
    assert_eq!(
        current.certificate_der.as_deref(),
        Some(current_certificate_der.as_slice())
    );
    assert_eq!(
        current.private_key_pem.as_deref(),
        Some(current_private_key_pem.as_slice())
    );
    assert_eq!(
        current.certificate_group_id.as_ref(),
        Some(&current_certificate_group_id)
    );
    assert_eq!(
        current.certificate_type_id.as_ref(),
        Some(&current_certificate_type_id)
    );
    assert_eq!(
        &current.pending_trust_lists,
        &std::collections::HashMap::from([(CertificateGroup::DefaultHttps, applying_trust_list,)])
    );

    assert_eq!(replacement.owning_session_id, context.session_id());
    assert_eq!(replacement.certificate_der, None);
    assert_eq!(replacement.private_key_pem, None);
    assert_eq!(replacement.certificate_group_id, None);
    assert_eq!(replacement.certificate_type_id, None);
    assert_eq!(
        &replacement.pending_trust_lists,
        &std::collections::HashMap::from([(
            CertificateGroup::DefaultApplication,
            replacement_trust_list,
        )])
    );
}

#[tokio::test]
async fn close_and_update_with_invalid_certificate_is_rejected_without_mutating_store() {
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
    let h = handler(Arc::new(GdsPushRegistry::default()));

    let trust_list = TrustListDataType {
        specified_lists: masks::TRUSTED_CERTIFICATES,
        trusted_certificates: Some(vec![ByteString::from(vec![0u8; 16])]),
        ..Default::default()
    };
    let payload = encode_trust_list(&trust_list).expect("should encode");

    let open_out = h
        .handle_open(
            &context,
            &[Variant::from(open_mode::WRITE | open_mode::ERASE_EXISTING)],
        )
        .expect("open for write should succeed");
    let Variant::UInt32(file_handle) = open_out[0] else {
        panic!("expected UInt32 file handle");
    };
    h.handle_write(
        &context,
        &[
            Variant::from(file_handle),
            Variant::from(ByteString::from(payload)),
        ],
    )
    .expect("write should succeed");

    assert_eq!(
        h.handle_close_and_update(&context, &[Variant::from(file_handle)]),
        Err(StatusCode::BadCertificateInvalid)
    );
    assert!(context
        .info
        .certificate_store
        .read()
        .read_trusted_certs()
        .is_empty());
}

#[tokio::test]
async fn open_write_requires_security_admin() {
    let (context, _handle) = request_context(
        MessageSecurityMode::Sign,
        vec![WellKnownRole::AuthenticatedUser.node_id()],
    );
    let h = handler(Arc::new(GdsPushRegistry::default()));

    assert_eq!(
        h.handle_open(&context, &[Variant::from(open_mode::READ)]),
        Err(StatusCode::BadUserAccessDenied)
    );
}

#[tokio::test]
async fn open_write_rejects_unauthenticated_channel() {
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::None);
    let h = handler(Arc::new(GdsPushRegistry::default()));

    assert_eq!(
        h.handle_open(&context, &[Variant::from(open_mode::READ)]),
        Err(StatusCode::BadSecurityModeInsufficient)
    );
}

#[tokio::test]
async fn add_certificate_immediately_adds_a_trusted_certificate() {
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
    let h = handler(Arc::new(GdsPushRegistry::default()));
    let cert = self_signed_cert_with_cn("added-directly");
    let der = cert.to_der().expect("cert should encode");

    h.handle_add_certificate(
        &context,
        &[Variant::from(ByteString::from(der)), Variant::from(true)],
    )
    .expect("add certificate should succeed");

    let trusted = context.info.certificate_store.read().read_trusted_certs();
    assert_eq!(trusted.len(), 1);
}

#[tokio::test]
async fn add_certificate_to_default_https_group_uses_its_own_trust_store() {
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
    let h = handler_for_group(
        Arc::new(GdsPushRegistry::default()),
        CertificateGroup::DefaultHttps,
    );
    let cert = self_signed_cert_with_cn("https-added-directly");
    let der = cert.to_der().expect("cert should encode");

    h.handle_add_certificate(
        &context,
        &[Variant::from(ByteString::from(der)), Variant::from(true)],
    )
    .expect("HTTPS add certificate should succeed");

    let store = context.info.certificate_store.read();
    assert_eq!(
        store
            .read_trusted_certs_for_group(CertificateGroup::DefaultHttps)
            .len(),
        1
    );
    assert!(store.read_trusted_certs().is_empty());
}

#[tokio::test]
async fn add_certificate_rejects_is_trusted_certificate_false() {
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
    let h = handler(Arc::new(GdsPushRegistry::default()));
    let cert = self_signed_cert_with_cn("rejected-issuer-add");
    let der = cert.to_der().expect("cert should encode");

    assert_eq!(
        h.handle_add_certificate(
            &context,
            &[Variant::from(ByteString::from(der)), Variant::from(false)],
        ),
        Err(StatusCode::BadCertificateInvalid)
    );
}

#[tokio::test]
async fn remove_certificate_immediately_removes_a_trusted_certificate() {
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
    let h = handler(Arc::new(GdsPushRegistry::default()));
    let cert = self_signed_cert_with_cn("to-be-removed");
    let store = context.info.certificate_store.read();
    store.store_trusted_cert(&cert).expect("should store");
    drop(store);

    h.handle_remove_certificate(
        &context,
        &[
            Variant::from(UAString::from(cert.thumbprint().as_hex_string())),
            Variant::from(true),
        ],
    )
    .expect("remove certificate should succeed");

    assert!(context
        .info
        .certificate_store
        .read()
        .read_trusted_certs()
        .is_empty());
}

#[tokio::test]
async fn remove_certificate_refuses_a_still_needed_ca() {
    let (context, _handle) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
    let h = handler(Arc::new(GdsPushRegistry::default()));

    // Both certs are self-signed with the same CN, so each one's issuer name equals the
    // other's subject name -- exercising the name-based dependency check without needing a
    // full CA-signed chain (see research.md).
    let ca = self_signed_cert_with_cn("shared-ca-name");
    let dependent = self_signed_cert_with_cn("shared-ca-name");
    let store = context.info.certificate_store.read();
    store.store_trusted_cert(&ca).expect("should store");
    store.store_trusted_cert(&dependent).expect("should store");
    drop(store);

    assert_eq!(
        h.handle_remove_certificate(
            &context,
            &[
                Variant::from(UAString::from(ca.thumbprint().as_hex_string())),
                Variant::from(true),
            ],
        ),
        Err(StatusCode::BadCertificateChainIncomplete)
    );
    assert_eq!(
        context
            .info
            .certificate_store
            .read()
            .read_trusted_certs()
            .len(),
        2
    );
}

#[tokio::test]
async fn add_certificate_rejects_while_write_transaction_open_elsewhere() {
    let registry = Arc::new(GdsPushRegistry::default());
    let (context1, _h1) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
    let h = handler(registry.clone());
    h.handle_open(
        &context1,
        &[Variant::from(open_mode::WRITE | open_mode::ERASE_EXISTING)],
    )
    .expect("open for write should succeed");

    let (context2_base, _h2) = security_admin_request_context(MessageSecurityMode::SignAndEncrypt);
    let context2 = RequestContext::new_test(Arc::new(RequestContextInner {
        session: context2_base.session.clone(),
        session_id: 2,
        authenticator: context2_base.authenticator.clone(),
        token: context2_base.token.clone(),
        user_roles: context2_base.user_roles.clone(),
        type_tree: context2_base.type_tree.clone(),
        type_tree_getter: context2_base.type_tree_getter.clone(),
        subscriptions: context2_base.subscriptions.clone(),
        info: context2_base.info.clone(),
    }));

    // context1's Open(write) doesn't itself reserve the transaction (only CloseAndUpdate
    // does, per Part 12 §7.8.2.2) -- stage one directly to simulate an in-progress
    // transaction from session 1.
    *registry.transaction.write() = PushTransactionState::Staged(PushTransaction {
        owning_session_id: 1,
        certificate_der: None,
        private_key_pem: None,
        certificate_group_id: None,
        certificate_type_id: None,
        pending_trust_lists: std::collections::HashMap::new(),
    });

    let cert = self_signed_cert_with_cn("blocked-add");
    let der = cert.to_der().expect("cert should encode");
    assert_eq!(
        h.handle_add_certificate(
            &context2,
            &[Variant::from(ByteString::from(der)), Variant::from(true)],
        ),
        Err(StatusCode::BadTransactionPending)
    );
}
