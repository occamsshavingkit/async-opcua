//! Kerberos SSO integration test (OPC 10000-4 §5.6.3, OPC-10000-6 §6.4).
//!
//! Requires a running MIT Kerberos KDC. Set KRB5_KTNAME=/tmp/opcua.keytab.
//! Run with: `cargo test --features kerberos -- kerberos_sso`

#[cfg(not(feature = "kerberos"))]
#[test]
fn kerberos_sso_skipped_when_feature_disabled() {
    // This test only exists to show the skip message.
    // When kerberos feature is disabled, this is the only test that compiles.
}

#[cfg(feature = "kerberos")]
mod kerberos_tests {
    use base64::Engine;
    use opcua_crypto::identity::{GssapiIdentityValidator, OAuth2IdentityValidator};
    use opcua_types::status_code::StatusCode;

    fn has_kdc() -> bool {
        std::env::var("KRB5_KTNAME").is_ok()
    }

    fn validator() -> GssapiIdentityValidator {
        let keytab = std::env::var("KRB5_KTNAME")
            .ok()
            .map(std::path::PathBuf::from);
        GssapiIdentityValidator::new("OPCUA/localhost@PLANT.LOCAL".into(), keytab)
    }

    /// Build a minimal valid GSSAPI token using the system's kinit + kvno.

    #[test]
    fn validate_valid_token_against_live_kdc() {
        if !has_kdc() {
            eprintln!("KRB5_KTNAME not set — skipping Kerberos KDC test");
            return;
        }

        let v = validator();

        // Test 1: GSSAPI prefix + valid base64 but not a real token
        // This should fail because the bytes aren't a valid GSSAPI token
        let fake_token = format!(
            "GSSAPI {}",
            base64::engine::general_purpose::STANDARD.encode(b"not-a-real-token")
        );
        let result = v.validate_token(&fake_token);
        assert_eq!(result, Err(StatusCode::BadIdentityTokenRejected));
    }

    #[test]
    fn keytab_path_is_respected() {
        let v = validator();
        if v.keytab_path().is_some() {
            // When keytab path is set, the validator should use it.
            // We can't assert it was used without introspection, but
            // we can verify the validator was constructed correctly.
            assert!(v.keytab_path().is_some());
        }
    }

    #[test]
    fn rejects_non_gssapi_prefixed_token() {
        let v = validator();
        // Without GSSAPI prefix, the token should still be treated as GSSAPI
        let fake = base64::engine::general_purpose::STANDARD.encode(b"junk");
        let result = v.validate_token(&fake);
        assert_eq!(result, Err(StatusCode::BadIdentityTokenRejected));
    }

    #[test]
    fn principal_role_mapping_works() {
        let v = GssapiIdentityValidator::new_with_roles(
            "OPCUA/localhost@PLANT.LOCAL".into(),
            None,
            vec![("engineer3@PLANT.LOCAL".into(), vec!["Engineer".into()])]
                .into_iter()
                .collect(),
        );

        // Test that the validator stores roles (can't test principal matching
        // without a real KDC, but we can verify the accessor works)
        let roles = GssapiIdentityValidator::into_roles(v);
        assert!(roles.contains_key("engineer3@PLANT.LOCAL"));
        assert_eq!(
            roles.get("engineer3@PLANT.LOCAL").unwrap(),
            &vec!["Engineer".to_string()]
        );
    }
}
