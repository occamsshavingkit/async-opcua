use opcua_types::ByteString;

use super::*;

#[test]
fn update_certificate_redacts_certificate_issuers_and_private_key() {
    let args = vec![
        Variant::from(NodeId::new(1, 1u32)),
        Variant::from(NodeId::new(1, 2u32)),
        Variant::from(ByteString::from(vec![1, 2, 3])),
        Variant::from(vec![ByteString::from(vec![4, 5])]),
        Variant::from("PEM"),
        Variant::from(ByteString::from(vec![6, 7, 8])),
    ];

    assert_eq!(
        sanitize_input_arguments(AuditAction::UpdateCertificate, &args),
        vec![
            args[0].clone(),
            args[1].clone(),
            Variant::Empty,
            Variant::Empty,
            args[4].clone(),
            Variant::Empty,
        ]
    );
}

#[test]
fn partial_update_certificate_redacts_all_arguments() {
    // Given: an incomplete UpdateCertificate argument list.
    let args = vec![Variant::from(NodeId::new(1, 1u32))];

    // When: the audit input arguments are sanitized.
    let sanitized = sanitize_input_arguments(AuditAction::UpdateCertificate, &args);

    // Then: no positional value is exposed from the incomplete list.
    assert_eq!(sanitized, vec![Variant::Empty]);
}

#[test]
fn start_signing_request_redacts_csr() {
    let args = vec![
        Variant::from(NodeId::new(1, 1u32)),
        Variant::from(NodeId::new(1, 2u32)),
        Variant::from(NodeId::new(1, 3u32)),
        Variant::from(ByteString::from(vec![1, 2, 3])),
    ];

    assert_eq!(
        sanitize_input_arguments(AuditAction::StartSigningRequest, &args),
        vec![
            args[0].clone(),
            args[1].clone(),
            args[2].clone(),
            Variant::Empty,
        ]
    );
}

#[test]
fn start_new_key_pair_request_redacts_private_key_password() {
    let args = vec![
        Variant::from(NodeId::new(1, 1u32)),
        Variant::from(NodeId::new(1, 2u32)),
        Variant::from(NodeId::new(1, 3u32)),
        Variant::from("CN=example"),
        Variant::from(vec!["example.com".to_string()]),
        Variant::from("PEM"),
        Variant::from("secret"),
    ];

    assert_eq!(
        sanitize_input_arguments(AuditAction::StartNewKeyPairRequest, &args),
        vec![
            args[0].clone(),
            args[1].clone(),
            args[2].clone(),
            args[3].clone(),
            args[4].clone(),
            args[5].clone(),
            Variant::Empty,
        ]
    );
}

#[test]
fn add_certificate_redacts_certificate_bytes() {
    let args = vec![
        Variant::from(ByteString::from(vec![1, 2, 3])),
        Variant::from(true),
    ];

    assert_eq!(
        sanitize_input_arguments(AuditAction::AddCertificate, &args),
        vec![Variant::Empty, args[1].clone()]
    );
}

#[test]
fn details_redacts_add_certificate_bytes() {
    // Given: AddCertificate arguments containing certificate bytes.
    let args = vec![
        Variant::from(ByteString::from(vec![1, 2, 3])),
        Variant::from(true),
    ];

    // When: the audit details builder receives the typed AddCertificate action.
    let event = details(
        NodeId::new(0, 1u32),
        NodeId::new(0, 2u32),
        NodeId::new(0, 3u32),
        AuditAction::AddCertificate,
        &args,
    );

    // Then: the certificate bytes are redacted at the details wiring boundary.
    assert_eq!(event.input_arguments[0], Variant::Empty);
}

#[test]
fn unknown_action_redacts_all_arguments() {
    let args = vec![Variant::from("unexpected"), Variant::from(true)];

    assert_eq!(
        sanitize_input_arguments(AuditAction::Unknown("UnknownAction"), &args),
        vec![Variant::Empty, Variant::Empty]
    );
}

#[test]
fn known_action_string_uses_known_visible_argument_schema() {
    let args = vec![
        Variant::from(ByteString::from(vec![1, 2, 3])),
        Variant::from(true),
    ];

    assert_eq!(
        sanitize_input_arguments(AuditAction::from("AddCertificate"), &args),
        vec![Variant::Empty, args[1].clone()]
    );
}

#[test]
fn remove_certificate_preserves_arguments() {
    let args = vec![Variant::from(NodeId::new(1, 1u32)), Variant::from(true)];

    assert_eq!(
        sanitize_input_arguments(AuditAction::RemoveCertificate, &args),
        args
    );
}

#[test]
fn remove_certificate_with_mismatched_arity_redacts_all_arguments() {
    let args = vec![
        Variant::from("00112233445566778899aabbccddeeff00112233"),
        Variant::from(true),
        Variant::from("unexpected"),
    ];

    assert_eq!(
        sanitize_input_arguments(AuditAction::RemoveCertificate, &args),
        vec![Variant::Empty, Variant::Empty, Variant::Empty]
    );
}
