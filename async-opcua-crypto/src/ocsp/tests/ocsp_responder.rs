// OPCUA for Rust
// SPDX-License-Identifier: MPL-2.0

//! Independent unit tests for the OCSP responder (`crate::ocsp::responder`).
//!
//! These tests are authored separately from the production implementation:
//! they build real RSA PKI fixtures (CA → EE) with the `x509-cert` builder
//! and the in-tree RSA keys, then drive `build_ocsp_response` end-to-end and
//! assert the RFC 6960 wire shape of the produced `OCSPResponse`.
//!
//! Spec grounding: RFC 6960 (X.509 Internet PKI Online Certificate Status
//! Protocol). In particular §4.1.1 (OCSPRequest), §4.2.1 (OCSPResponse and
//! BasicOcspResponse), and §4.4.1 (Nonce extension).

use std::collections::HashMap;
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use der::{Decode, Encode};
use rsa::pkcs1v15::{Signature, SigningKey};
use sha1::{Digest, Sha1};
use sha2::Sha256;
use x509_cert::builder::{Builder, CertificateBuilder, Profile};
use x509_cert::der::asn1::OctetString;
use x509_cert::ext::pkix::{BasicConstraints, KeyUsage, KeyUsages, SubjectKeyIdentifier};
use x509_cert::name::Name;
use x509_cert::serial_number::SerialNumber;
use x509_cert::time::{Time, Validity};
use x509_ocsp::builder::OcspRequestBuilder;
use x509_ocsp::ext::Nonce;
use x509_ocsp::{BasicOcspResponse, OcspResponse, OcspResponseStatus, Request};

use crate::ocsp::codec::{decode_ocsp_response, encode_ocsp_request};
use crate::ocsp::config::CertStatusResult;
use crate::ocsp::responder::{build_ocsp_response, CertStatusVariant, OcspResponderConfig};
use crate::{PrivateKey, X509};

// --- fixture builders ----------------------------------------------------------------------

/// Issue an X.509 certificate with the requested subject/issuer/serial, signed
/// by `issuer_key`. Mirrors the fixture pattern in
/// `src/tests/cert_chain.rs:issue_with_signing_key_and_path_len` but trimmed to
/// the minimum extensions required for OCSP test fixtures.
fn issue_cert(
    subject_cn: &str,
    subject_key: &PrivateKey,
    issuer_cn: &str,
    issuer_key: &PrivateKey,
    serial: u32,
    is_ca: bool,
) -> X509 {
    let signing_rsa = issuer_key
        .rsa_key_for_x509()
        .expect("issuer rsa key")
        .clone();
    let signing_key = SigningKey::<Sha256>::new(signing_rsa);

    let subject_spki = subject_key
        .public_key_to_info()
        .expect("subject SubjectPublicKeyInfo");
    let subject = Name::from_str(&format!("CN={subject_cn}")).expect("subject name");
    let issuer = Name::from_str(&format!("CN={issuer_cn}")).expect("issuer name");

    // Validity is one year from now — well within RFC 5280 §4.1.2.5 UTCTime
    // vs GeneralizedTime rules, and irrelevant to OCSP responder behaviour.
    let validity = Validity::from_now(Duration::new(86_400 * 365, 0)).expect("validity");

    let profile = Profile::Manual {
        issuer: Some(issuer),
    };
    let mut builder = CertificateBuilder::new(
        profile,
        SerialNumber::from(serial),
        validity,
        subject,
        subject_spki.clone(),
        &signing_key,
    )
    .expect("certificate builder");

    // SKI = SHA-1 of the subject public key bitstring per RFC 5280 §4.2.1.2.
    let mut hasher = Sha1::new();
    hasher.update(subject_spki.subject_public_key.raw_bytes());
    let ski = hasher.finalize();
    builder
        .add_extension(&SubjectKeyIdentifier(
            OctetString::new(ski.as_slice()).expect("ski octets"),
        ))
        .expect("add SKI");

    builder
        .add_extension(&BasicConstraints {
            ca: is_ca,
            path_len_constraint: None,
        })
        .expect("add BasicConstraints");

    let key_usage = if is_ca {
        KeyUsage(KeyUsages::KeyCertSign | KeyUsages::CRLSign)
    } else {
        KeyUsage(KeyUsages::DigitalSignature.into())
    };
    builder.add_extension(&key_usage).expect("add KeyUsage");

    let cert = builder.build::<Signature>().expect("build cert");
    let der = cert.to_der().expect("cert DER");
    X509::from_der(&der).expect("parse fixture cert")
}

/// Test PKI: a self-signed CA and an EE cert issued by that CA with a
/// distinct serial, plus the CA's private key (used as the OCSP responder
/// signer).
struct TestPki {
    ca_cert: X509,
    ca_key: PrivateKey,
    ee_cert: X509,
    /// A second EE cert with a different serial, used to exercise the
    /// "not in status DB" path without reusing the CA's serial.
    ee_cert_other_serial: X509,
}

fn test_pki() -> TestPki {
    let ca_key = PrivateKey::new(2048).expect("CA key");
    let ee_key = PrivateKey::new(2048).expect("EE key");
    let ee_other_key = PrivateKey::new(2048).expect("other EE key");

    let ca_cert = issue_cert(
        "async-opcua ocsp test CA",
        &ca_key,
        "async-opcua ocsp test CA",
        &ca_key,
        1,
        true,
    );
    let ee_cert = issue_cert(
        "async-opcua ocsp test EE",
        &ee_key,
        "async-opcua ocsp test CA",
        &ca_key,
        0xDEAD_BEEFu32,
        false,
    );
    let ee_cert_other_serial = issue_cert(
        "async-opcua ocsp test EE other",
        &ee_other_key,
        "async-opcua ocsp test CA",
        &ca_key,
        0xCAFE_FEEDu32,
        false,
    );

    TestPki {
        ca_cert,
        ca_key,
        ee_cert,
        ee_cert_other_serial,
    }
}

/// Build a minimal responder config whose status DB maps the EE cert serial
/// to `status`. Other serials fall through to "unknown" per RFC 6960 §4.2.1.
fn responder_config(pki: &TestPki, status: CertStatusVariant) -> OcspResponderConfig {
    let mut status_db = HashMap::new();
    status_db.insert(pki.ee_cert.serial_number(), status);
    OcspResponderConfig {
        signer_cert: pki.ca_cert.clone(),
        signer_key: pki.ca_key.clone(),
        response_validity: Duration::from_secs(3600),
        status_db,
    }
}

// --- tests ----------------------------------------------------------------------------------

/// RFC 6960 §4.2.1: a serial present in the status DB as `Good` MUST produce
/// a `SingleResponse` whose `certStatus` is `good`.
#[test]
fn responder_known_good_certificate() {
    let pki = test_pki();
    let config = responder_config(&pki, CertStatusVariant::Good);

    // Build a real OCSPRequest (full SEQUENCE wrap, not just TBSRequest) so
    // the responder can `OcspRequest::from_der` it.
    let request_der =
        encode_ocsp_request(pki.ca_cert.inner(), pki.ee_cert.inner()).expect("encode OCSP request");

    let response_der = build_ocsp_response(&request_der, &config).expect("build response");

    let status = decode_ocsp_response(&response_der, &pki.ee_cert.serial_number())
        .expect("decode OCSP response");
    assert_eq!(status, CertStatusResult::Good);
}

/// RFC 6960 §4.2.1: a serial marked `Revoked` MUST produce a `certStatus` of
/// `revoked` carrying the configured revocation time and reason.
#[test]
fn responder_revoked_certificate() {
    let pki = test_pki();
    // RFC 5280 §5.3.1 CrlReason keyCompromise (1).
    let revocation_time = Time::try_from(UNIX_EPOCH + Duration::from_secs(1_700_000_000))
        .expect("valid revocation time");
    let config = responder_config(
        &pki,
        CertStatusVariant::Revoked {
            revocation_time,
            revocation_reason: Some(1),
        },
    );

    let request_der =
        encode_ocsp_request(pki.ca_cert.inner(), pki.ee_cert.inner()).expect("encode OCSP request");

    let response_der = build_ocsp_response(&request_der, &config).expect("build response");

    let status = decode_ocsp_response(&response_der, &pki.ee_cert.serial_number())
        .expect("decode OCSP response");
    assert_eq!(status, CertStatusResult::Revoked);

    // Verify the revocation time is carried in the response (RFC 6960
    // §4.2.1 RevokedInfo.revocationTime).
    let response = OcspResponse::from_der(&response_der).expect("decode OcspResponse");
    assert_eq!(response.response_status, OcspResponseStatus::Successful);
    let response_bytes = response
        .response_bytes
        .as_ref()
        .expect("response has responseBytes");
    let basic =
        BasicOcspResponse::from_der(response_bytes.response.as_bytes()).expect("decode basic");
    let single = basic
        .tbs_response_data
        .responses
        .iter()
        .find(|r| r.cert_id.serial_number.as_bytes() == pki.ee_cert.serial_number().as_slice())
        .expect("find SingleResponse for EE cert");
    if let x509_ocsp::CertStatus::Revoked(info) = &single.cert_status {
        // OcspGeneralizedTime wraps der::asn1::GeneralizedTime; SystemTime
        // has a `From<GeneralizedTime>` impl via the `der` crate.
        let returned: SystemTime = info.revocation_time.0.into();
        let original: SystemTime = revocation_time.into();
        // RFC 6960 §4.2.1 stores revocation time as GeneralizedTime; the
        // round-trip through `OcspGeneralizedTime` may truncate sub-second
        // precision, so we allow up to 1s of drift.
        let diff_secs = if returned >= original {
            returned
                .duration_since(original)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        } else {
            original
                .duration_since(returned)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        };
        assert!(diff_secs <= 1, "revocation_time drifted by {diff_secs}s");
    } else {
        panic!("expected CertStatus::Revoked, got {:?}", single.cert_status);
    }
}

/// RFC 6960 §4.2.1: a serial absent from the status DB MUST be reported as
/// `unknown` (a status, not an error).
#[test]
fn responder_unknown_certificate() {
    let pki = test_pki();
    // Status DB only knows about ee_cert; query for ee_cert_other_serial.
    let config = responder_config(&pki, CertStatusVariant::Good);

    let request_der = encode_ocsp_request(pki.ca_cert.inner(), pki.ee_cert_other_serial.inner())
        .expect("encode OCSP request");

    let response_der = build_ocsp_response(&request_der, &config).expect("build response");

    let status = decode_ocsp_response(&response_der, &pki.ee_cert_other_serial.serial_number())
        .expect("decode OCSP response");
    assert_eq!(status, CertStatusResult::Unknown);
}

/// RFC 6960 §4.4.1: when the request carries an OCSP nonce extension, the
/// response MUST echo the same nonce in `responseExtensions`.
#[test]
fn responder_nonce_echo() {
    let pki = test_pki();
    let config = responder_config(&pki, CertStatusVariant::Good);

    // Build the request with `x509_ocsp::builder::OcspRequestBuilder` so we
    // can attach a nonce extension (RFC 6960 §4.4.1) and serialize the full
    // OCSPRequest — `encode_ocsp_request` does not currently support nonces.
    let request =
        Request::from_cert::<Sha1>(pki.ca_cert.inner(), pki.ee_cert.inner()).expect("build CertID");
    let nonce_bytes: Vec<u8> = (0u8..32).collect();
    let nonce = Nonce::new(nonce_bytes.clone()).expect("build Nonce");
    let ocsp_request = OcspRequestBuilder::default()
        .with_request(request)
        .with_extension(nonce)
        .expect("attach nonce extension")
        .build();
    let request_der = ocsp_request.to_der().expect("encode OCSP request");

    let response_der = build_ocsp_response(&request_der, &config).expect("build response");

    let response = OcspResponse::from_der(&response_der).expect("decode OcspResponse");
    assert_eq!(response.response_status, OcspResponseStatus::Successful);
    let response_bytes = response
        .response_bytes
        .as_ref()
        .expect("response has responseBytes");
    let basic =
        BasicOcspResponse::from_der(response_bytes.response.as_bytes()).expect("decode basic");

    let echoed = basic
        .nonce()
        .expect("response should echo the request nonce");
    assert_eq!(echoed.0.as_bytes(), nonce_bytes.as_slice());
}

/// RFC 6960 §4.2.1: a request that cannot be parsed as an `OCSPRequest` MUST
/// yield an `OCSPResponse` whose `responseStatus` is `malformedRequest` (1),
/// not an `Err` and not a panic. The malformed response is returned as
/// `Ok(..)` so a broken client still receives a well-formed reply.
#[test]
fn responder_malformed_request() {
    let pki = test_pki();
    let config = responder_config(&pki, CertStatusVariant::Good);

    // Bytes that are not valid DER for an OCSPRequest SEQUENCE.
    let garbage: &[u8] = b"this is not a valid OCSP request";

    let response_der = build_ocsp_response(garbage, &config).expect("malformed is Ok(..)");

    let response = OcspResponse::from_der(&response_der).expect("decode OcspResponse");
    assert_eq!(
        response.response_status,
        OcspResponseStatus::MalformedRequest
    );
    // RFC 6960 §4.2.1: malformed responses carry no responseBytes.
    assert!(response.response_bytes.is_none());
}

/// RFC 6960 §4.2.1: the response signature over `responseData` MUST verify
/// against the responder cert's public key.
#[test]
fn responder_signature_verifies() {
    let pki = test_pki();
    let config = responder_config(&pki, CertStatusVariant::Good);

    let request_der =
        encode_ocsp_request(pki.ca_cert.inner(), pki.ee_cert.inner()).expect("encode OCSP request");
    let response_der = build_ocsp_response(&request_der, &config).expect("build response");

    let response = OcspResponse::from_der(&response_der).expect("decode OcspResponse");
    assert_eq!(response.response_status, OcspResponseStatus::Successful);
    let response_bytes = response
        .response_bytes
        .as_ref()
        .expect("response has responseBytes");
    let basic =
        BasicOcspResponse::from_der(response_bytes.response.as_bytes()).expect("decode basic");

    // The to-be-signed bytes are the DER encoding of ResponseData, not
    // BasicOcspResponse (RFC 6960 §4.2.1).
    let tbs = basic
        .tbs_response_data
        .to_der()
        .expect("encode ResponseData");
    let signature = basic
        .signature
        .as_bytes()
        .expect("signature is octet-aligned");

    let signer_public_key = pki.ca_cert.public_key().expect("extract CA public key");
    let verified = signer_public_key
        .verify_sha256(&tbs, signature)
        .expect("verify_sha256 call should not error");
    assert!(
        verified,
        "OCSP response signature must verify with CA public key"
    );

    // Negative control: flipping a byte in the to-be-signed bytes MUST cause
    // verification to fail. Guards against accidental constant-time-equality
    // false positives or signatures that verify against any input.
    let mut tampered = tbs.clone();
    let flip_idx = tampered.len().saturating_sub(1);
    tampered[flip_idx] ^= 0xFF;
    let still_verified = signer_public_key
        .verify_sha256(&tampered, signature)
        .unwrap_or(false);
    assert!(
        !still_verified,
        "tampered ResponseData must NOT verify against the original signature"
    );
}
