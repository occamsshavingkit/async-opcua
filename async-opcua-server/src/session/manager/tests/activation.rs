use crate::{
    authenticator::{AuthManager, UserToken},
    rbac::rules::IdentityMappingRule,
};
use opcua_crypto::{random, SecurityPolicy};
use opcua_types::{ExtensionObject, NodeId, StatusCode};
use std::sync::Arc;

use super::*;

/// US1 (FR-001): the client application certificate bound at CreateSession must match the
/// certificate that secured the activating channel, under any secured policy. `None` policy has
/// no channel certificate, so the binding is not checked.
#[test]
fn client_certificate_channel_binding_rules() {
    let c1 = make_cert("client-one");
    let c2 = make_cert("client-two");

    // Matching certificate under a secured policy: no violation.
    assert!(!is_client_certificate_channel_mismatch(
        Some(&c1),
        Some(&c1),
        SecurityPolicy::Basic256Sha256
    ));
    // Different certificate: a binding violation (must be rejected).
    assert!(is_client_certificate_channel_mismatch(
        Some(&c1),
        Some(&c2),
        SecurityPolicy::Basic256Sha256
    ));
    // Secured policy but the channel presented no peer certificate: fail closed.
    assert!(is_client_certificate_channel_mismatch(
        Some(&c1),
        None,
        SecurityPolicy::Basic256Sha256
    ));
    // None policy: no channel certificate exists, so the binding is not checked.
    assert!(!is_client_certificate_channel_mismatch(
        Some(&c1),
        Some(&c2),
        SecurityPolicy::None
    ));
    assert!(!is_client_certificate_channel_mismatch(
        None,
        None,
        SecurityPolicy::None
    ));
}

/// US2 (FR-005 lock-in / SC-002): a session is bound to its secure channel — a request whose
/// secure-channel id differs from the session's is rejected with `BadSecureChannelIdInvalid`.
/// This is the check `SessionController::validate_request` runs on every session-scoped request.
#[tokio::test]
async fn session_rejects_request_from_a_different_secure_channel() {
    let fixture = ActivationFixture::new(Arc::new(AuthenticationGate::open()));
    let session = fixture.session.read();
    // The fixture session belongs to secure channel 7.
    assert!(session.validate_secure_channel_id(7).is_ok());
    assert_eq!(
        session.validate_secure_channel_id(8).unwrap_err(),
        StatusCode::BadSecureChannelIdInvalid,
        "a session must reject a request arriving on a different secure channel"
    );
}

/// US2 (FR-002 lock-in): under `SecurityPolicy::None` there is no channel certificate, so the
/// new client-cert↔channel binding must be skipped and activation must still succeed.
#[tokio::test]
async fn none_policy_activation_skips_certificate_binding() {
    let fixture = ActivationFixture::new(Arc::new(AuthenticationGate::open()));
    let result = fixture.activate_with(SecurityPolicy::None, 7).await;
    assert!(
        result.is_ok(),
        "None-policy activation must succeed without a channel certificate, got {result:?}"
    );
}

/// T048 / H1: an activated session under SecurityPolicy::None must not be
/// transferable to a different secure channel (there is no cryptographic
/// channel binding, so a transfer would be a session hijack). Sessions that
/// are not yet activated can never move channels; activated sessions on a
/// *secured* policy may legitimately move (e.g. reconnect).
#[test]
fn cross_channel_transfer_rules() {
    // Same channel is always permitted, regardless of state/policy.
    assert!(!is_cross_channel_transfer_forbidden(
        1,
        1,
        true,
        SecurityPolicy::None
    ));
    assert!(!is_cross_channel_transfer_forbidden(
        1,
        1,
        false,
        SecurityPolicy::Basic256Sha256
    ));

    // Different channel + not yet activated → always refused (any policy).
    assert!(is_cross_channel_transfer_forbidden(
        1,
        2,
        false,
        SecurityPolicy::None
    ));
    assert!(is_cross_channel_transfer_forbidden(
        1,
        2,
        false,
        SecurityPolicy::Basic256Sha256
    ));

    // H1 core: activated None-policy session cannot move channels.
    assert!(is_cross_channel_transfer_forbidden(
        1,
        2,
        true,
        SecurityPolicy::None
    ));

    // Activated session on a secured policy MAY transfer channels.
    assert!(!is_cross_channel_transfer_forbidden(
        1,
        2,
        true,
        SecurityPolicy::Basic256Sha256
    ));
}

/// T007a / P4-SESS-07: OPC-10000-4 5.7.3.1 requires a cross-channel
/// ActivateSession to use the same SecurityMode as the original SecureChannel.
/// The secure-channel mismatch must be rejected before user authentication.
#[tokio::test]
async fn activate_session_rejects_cross_channel_security_mode_mismatch_before_authentication() {
    let gate = Arc::new(AuthenticationGate::open());
    let authenticator: Arc<dyn AuthManager> = gate.clone();
    let (client_cert, client_key) = make_cert_and_key("security-mode-client");
    let fixture = ActivationFixture::with_secured_session(authenticator, client_cert);
    let original_identity = anonymous_identity_with_policy("already-authenticated");
    let original_nonce = random::byte_string(fixture.info.config.session_nonce_length);

    fixture.mutate_session_activation(
        7,
        original_nonce,
        original_identity,
        UserToken("already-authenticated-user".to_string()),
    );
    let previous_identity = fixture.user_identity();

    let result = fixture
        .activate_with_signed_client_proof(
            SecurityPolicy::Basic256Sha256,
            MessageSecurityMode::Sign,
            8,
            &client_key,
        )
        .await;

    let error = result
        .expect_err("SecurityMode mismatch on a cross-channel ActivateSession must be rejected");
    assert_eq!(
        error,
        StatusCode::BadSecureChannelIdInvalid,
        "SecurityMode mismatch must use the secure-channel mismatch status"
    );
    assert!(
        !gate.was_called(),
        "SecurityMode mismatch must be rejected before user authentication"
    );
    assert_eq!(
        fixture.secure_channel_id(),
        7,
        "failed cross-channel activation must not rebind the session"
    );
    assert_eq!(
        fixture.user_identity(),
        previous_identity,
        "failed cross-channel activation must not change session identity"
    );
}

/// T007b / P4-SESS-07: OPC-10000-4 5.7.3.1 requires a cross-channel
/// ActivateSession to use the same SecurityPolicy as the original SecureChannel.
/// The secure-channel mismatch must be rejected before user authentication.
#[tokio::test]
async fn activate_session_rejects_cross_channel_security_policy_mismatch_before_authentication() {
    let gate = Arc::new(AuthenticationGate::open());
    let authenticator: Arc<dyn AuthManager> = gate.clone();
    let (client_cert, client_key) = make_cert_and_key("security-policy-client");
    let fixture = ActivationFixture::with_secured_session(authenticator, client_cert);
    let original_identity = anonymous_identity_with_policy("already-authenticated");
    let original_nonce = random::byte_string(fixture.info.config.session_nonce_length);

    fixture.mutate_session_activation(
        7,
        original_nonce,
        original_identity,
        UserToken("already-authenticated-user".to_string()),
    );
    let previous_identity = fixture.user_identity();

    let result = fixture
        .activate_with_signed_client_proof(
            SecurityPolicy::Aes128Sha256RsaOaep,
            MessageSecurityMode::SignAndEncrypt,
            8,
            &client_key,
        )
        .await;

    let error = result
        .expect_err("SecurityPolicy mismatch on a cross-channel ActivateSession must be rejected");
    assert_eq!(
        error,
        StatusCode::BadSecureChannelIdInvalid,
        "SecurityPolicy mismatch must use the secure-channel mismatch status"
    );
    assert!(
        !gate.was_called(),
        "SecurityPolicy mismatch must be rejected before user authentication"
    );
    assert_eq!(
        fixture.secure_channel_id(),
        7,
        "failed cross-channel activation must not rebind the session"
    );
    assert_eq!(
        fixture.user_identity(),
        previous_identity,
        "failed cross-channel activation must not change session identity"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn activate_session_signature_verification_does_not_starve_runtime_timers() {
    let gate = Arc::new(AuthenticationGate::open());
    let authenticator: Arc<dyn AuthManager> = gate.clone();
    let (client_cert, client_key) = make_cert_and_key("activate-session-offload-client");
    let fixture = ActivationFixture::with_secured_session(authenticator, client_cert);
    let timer_started = tokio::time::Instant::now();
    let timer = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        timer_started.elapsed()
    });

    let activation = fixture
        .activate_with_signed_client_proof(
            SecurityPolicy::Basic256Sha256,
            MessageSecurityMode::SignAndEncrypt,
            7,
            &client_key,
        )
        .await;

    assert!(
        activation.is_ok(),
        "signed activation failed: {activation:?}"
    );
    assert!(gate.was_called(), "activation should reach authentication");
    let timer_elapsed = tokio::time::timeout(std::time::Duration::from_millis(150), timer)
        .await
        .expect("runtime timer should not be starved by signature verification")
        .unwrap();
    assert!(
        timer_elapsed < std::time::Duration::from_millis(150),
        "runtime timer was delayed by signature verification: {timer_elapsed:?}"
    );
}

/// T008 / P4-SESS-08: OPC-10000-4 5.7.3.1 requires anonymous
/// ActivateSession over a new Sign-only SecureChannel to fail because a
/// non-anonymous user is required. The rejection must happen before user
/// authentication or session rebinding.
#[tokio::test]
async fn activate_session_rejects_anonymous_transfer_to_new_sign_channel_before_authentication() {
    let gate = Arc::new(AuthenticationGate::open());
    let authenticator: Arc<dyn AuthManager> = gate.clone();
    let (client_cert, client_key) = make_cert_and_key("sign-only-anonymous-transfer-client");
    let fixture = ActivationFixture::with_secured_session_created_with_mode(
        authenticator,
        client_cert,
        MessageSecurityMode::Sign,
    );
    let original_identity = IdentityToken::UserName(UserNameIdentityToken {
        policy_id: UAString::from("already-authenticated"),
        user_name: UAString::from("already-authenticated-user"),
        password: ByteString::null(),
        encryption_algorithm: UAString::null(),
    });
    let original_nonce = random::byte_string(fixture.info.config.session_nonce_length);

    fixture.mutate_session_activation(
        7,
        original_nonce,
        original_identity,
        UserToken("already-authenticated-user".to_string()),
    );
    let previous_identity = fixture.user_identity();

    let result = fixture
        .activate_with_signed_client_proof(
            SecurityPolicy::Basic256Sha256,
            MessageSecurityMode::Sign,
            8,
            &client_key,
        )
        .await;

    let error = result.expect_err(
        "anonymous ActivateSession over a new Sign-only SecureChannel must be rejected",
    );
    assert_eq!(
        error,
        StatusCode::BadIdentityTokenRejected,
        "anonymous transfer to a new Sign-only channel must reject the identity token"
    );
    assert!(
        !gate.was_called(),
        "anonymous Sign-only transfer must be rejected before user authentication"
    );
    assert_eq!(
        fixture.secure_channel_id(),
        7,
        "failed anonymous Sign-only transfer must not rebind the session"
    );
    assert_eq!(
        fixture.user_identity(),
        previous_identity,
        "failed anonymous Sign-only transfer must not change session identity"
    );
}

/// T013 / P4-SESS-09: OPC-10000-4 5.7.3.2 defines the X.509
/// `userIdentityToken` and `userTokenSignature` carried by ActivateSession,
/// and 5.7.3.3 defines rejection results for invalid or rejected identity
/// tokens. A failed X.509 activation must not store the rejected identity on
/// the session before a later valid X.509 activation succeeds.
#[tokio::test]
async fn activate_session_failed_x509_activation_does_not_leave_rejected_identity_state() {
    let (rejected_cert, _) = make_cert_and_key("rejected-x509-state-user");
    let (accepted_cert, accepted_key) = make_cert_and_key("accepted-x509-state-user");
    let accepted_identity = IdentityTokenSnapshot::X509(accepted_cert.thumbprint().as_hex_string());
    let authenticator = Arc::new(X509AuthenticationGate::new(accepted_cert.thumbprint()));
    let fixture = ActivationFixture::with_x509_session(authenticator);
    fixture.trust_x509_user_certificate(&rejected_cert);
    fixture.trust_x509_user_certificate(&accepted_cert);
    let original_identity = fixture.user_identity();

    let rejected = fixture
        .activate_x509_with(&rejected_cert, &accepted_key)
        .await;

    assert_eq!(
        rejected.expect_err("bad X.509 user-token signature must be rejected"),
        StatusCode::BadUserSignatureInvalid,
        "failed X.509 activation must surface the user-token signature rejection"
    );
    assert_eq!(
        fixture.user_identity(),
        original_identity,
        "failed X.509 activation must not store the rejected identity"
    );

    fixture
        .activate_x509_with(&accepted_cert, &accepted_key)
        .await
        .expect("accepted X.509 identity should activate the session");

    assert_eq!(
        fixture.user_identity(),
        accepted_identity,
        "later valid activation must store only the accepted X.509 identity"
    );
}

#[tokio::test]
async fn activate_session_rejects_stale_nonce_after_intervening_activation() {
    let stale_gate = Arc::new(AuthenticationGate::open());
    let fixture = ActivationFixture::new(stale_gate.clone());

    let baseline = fixture.activate_with(SecurityPolicy::None, 7).await;
    assert!(
        baseline.is_ok(),
        "normal uncontended activation should succeed, got {baseline:?}"
    );

    let stale_nonce = fixture.session_nonce();
    stale_gate.pause_next_authentication();
    let stale_activation = {
        let fixture = fixture.clone();
        tokio::spawn(async move { fixture.activate_with(SecurityPolicy::None, 7).await })
    };

    stale_gate.wait_until_entered().await;

    let intervening_nonce = random::byte_string(fixture.info.config.session_nonce_length);
    fixture.mutate_session_activation(
        7,
        intervening_nonce.clone(),
        anonymous_identity_with_policy("intervening-anonymous"),
        UserToken("intervening-user".to_string()),
    );
    let intervening_identity = fixture.user_identity();
    let intervening_channel_id = fixture.secure_channel_id();
    assert_ne!(
        stale_nonce, intervening_nonce,
        "intervening activation must rotate the nonce observed by the stale activation"
    );

    stale_gate.release();
    let stale_result = stale_activation
        .await
        .expect("stale activation task should not panic");

    assert!(
        matches!(
            stale_result,
            Err(StatusCode::BadNonceInvalid | StatusCode::BadSessionIdInvalid)
        ),
        "stale activation should fail closed after nonce rotation, got {stale_result:?}"
    );
    assert_eq!(
        fixture.session_nonce(),
        intervening_nonce,
        "stale activation must not overwrite the nonce from the intervening activation"
    );
    assert_eq!(
        fixture.secure_channel_id(),
        intervening_channel_id,
        "stale activation must not overwrite the secure channel"
    );
    assert_eq!(
        fixture.user_identity(),
        intervening_identity,
        "stale activation must not overwrite session identity"
    );
}

/// US4 (FR-002/FR-006): a None-policy ActivateSession that carries no `ECDHPolicyUri` must leave
/// the response `AdditionalHeader` null — the ECC EphemeralKey wiring is inert on non-ECDH flows,
/// byte-identical to before the feature (and holds identically whether or not `ecc` is compiled
/// in). Anchored to §6.8.2: an absent `ECDHPolicyUri` yields no `ECDHKey`.
#[tokio::test]
async fn activate_session_without_ecdh_policy_leaves_response_header_null() {
    let fixture = ActivationFixture::new(Arc::new(AuthenticationGate::open()));
    let response = fixture
        .activate_with(SecurityPolicy::None, 7)
        .await
        .expect("anonymous None-policy activation should succeed");
    assert_eq!(
        response.response_header.additional_header,
        ExtensionObject::null(),
        "an ActivateSession with no ECDHPolicyUri must not add an ECDHKey to the response header"
    );
}

#[tokio::test]
async fn anonymous_activation_stores_anonymous_role_on_session() {
    let fixture = ActivationFixture::new(Arc::new(AuthenticationGate::open()));
    fixture
        .activate_with(SecurityPolicy::None, 7)
        .await
        .expect("anonymous activation should succeed");

    assert_eq!(
        fixture.session.read().roles().as_slice(),
        [WellKnownRole::Anonymous.node_id()]
    );
}

#[tokio::test]
async fn activation_reads_runtime_mutable_role_resolver() {
    let dynamic_role = NodeId::new(1, "RuntimeResolvedRole");
    let fixture = ActivationFixture::with_username_user("alice", "correct-password");

    {
        let mut resolver = fixture.info.role_resolver.write();
        resolver.register_role(dynamic_role.clone());
        resolver.add_mapping(
            dynamic_role.clone(),
            IdentityMappingRule::UserName("alice".into()),
        );
    }

    fixture
        .activate_username_with(SecurityPolicy::None, 7, "alice", "correct-password")
        .await
        .expect("username activation should succeed");

    assert!(fixture.session.read().roles().contains(&dynamic_role));
}
