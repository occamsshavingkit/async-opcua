use crate::session::audit::client_user_id_from_session;

use super::*;

#[tokio::test]
async fn x509_client_user_id_uses_authenticated_certificate_subject() {
    // Given: a session successfully authenticated with a trusted X.509 user certificate.
    let (certificate, private_key) = make_cert_and_key("audit-client-user");
    let expected_subject = certificate.subject_name();
    let authenticator = Arc::new(X509AuthenticationGate::new(certificate.thumbprint()));
    let fixture = ActivationFixture::with_x509_session(authenticator);
    fixture.trust_x509_user_certificate(&certificate);
    fixture
        .activate_x509_with(&certificate, &private_key)
        .await
        .expect("trusted X.509 identity should activate the session");

    // When: the audit ClientUserId is derived from the authenticated session.
    let client_user_id = client_user_id_from_session(&fixture.session.read());

    // Then: the identifier is the stable certificate subject, not the token kind literal.
    assert_eq!(client_user_id, UAString::from(expected_subject.as_str()));
}

#[tokio::test]
async fn x509_client_user_id_falls_back_when_certificate_is_malformed() {
    // Given: an X.509 session identity whose certificate data cannot be parsed.
    let fixture = ActivationFixture::new(Arc::new(AuthenticationGate::open()));
    let identity = IdentityToken::X509(X509IdentityToken {
        policy_id: UAString::from(POLICY_ID_X509),
        certificate_data: ByteString::from(&[0x30, 0x03, 0x02, 0x01]),
    });
    fixture.mutate_session_activation(
        7,
        random::byte_string(fixture.info.config.session_nonce_length),
        identity,
        UserToken("malformed-x509-user".to_string()),
    );

    // When: the audit ClientUserId is derived from that session.
    let client_user_id = client_user_id_from_session(&fixture.session.read());

    // Then: unavailable certificate data produces an unavailable client user ID.
    assert_eq!(client_user_id, UAString::null());
}

#[tokio::test]
async fn issued_token_client_user_id_is_null_before_authentication() {
    // Given: an unactivated session carrying an IssuedToken identity.
    let fixture = ActivationFixture::new(Arc::new(AuthenticationGate::open()));
    let session = Session::create(
        &fixture.info,
        NodeId::new(1, 1),
        7,
        0,
        0,
        0,
        UAString::null(),
        String::new(),
        IdentityToken::IssuedToken(opcua_types::IssuedIdentityToken {
            policy_id: UAString::from("issued_none"),
            token_data: ByteString::null(),
            encryption_algorithm: UAString::null(),
        }),
        None,
        ByteString::null(),
        UAString::null(),
        ApplicationDescription::default(),
        MessageSecurityMode::None,
    );

    // When: the audit ClientUserId is derived from that session.
    let client_user_id = client_user_id_from_session(&session);

    // Then: an absent authenticated user token produces an unavailable client user ID.
    assert_eq!(client_user_id, UAString::null());
}

#[tokio::test]
async fn issued_token_client_user_id_uses_authenticated_session_user_token() {
    // Given: an IssuedToken session with an authenticated user token.
    let fixture = ActivationFixture::new(Arc::new(AuthenticationGate::open()));
    fixture.mutate_session_activation(
        7,
        random::byte_string(fixture.info.config.session_nonce_length),
        IdentityToken::IssuedToken(opcua_types::IssuedIdentityToken {
            policy_id: UAString::from("issued_none"),
            token_data: ByteString::from(&[1, 2, 3]),
            encryption_algorithm: UAString::null(),
        }),
        UserToken("authenticated-issued-user".to_string()),
    );

    // When: the audit ClientUserId is derived from that session.
    let client_user_id = client_user_id_from_session(&fixture.session.read());

    // Then: the authenticated session user token remains the identifier.
    assert_eq!(client_user_id, UAString::from("authenticated-issued-user"));
}
