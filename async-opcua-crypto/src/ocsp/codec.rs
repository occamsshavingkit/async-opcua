use const_oid::ObjectIdentifier;
use der::{Decode, Encode};
use sha1::{Digest, Sha1};
use tracing::trace;
use x509_cert::certificate::Certificate;
use x509_cert::der::{
    asn1::{Null, OctetString},
    Sequence,
};
use x509_cert::serial_number::SerialNumber;
use x509_cert::spki::AlgorithmIdentifierOwned;
use x509_ocsp::builder::OcspRequestBuilder;
use x509_ocsp::{CertStatus, OcspResponse, OcspResponseStatus};

use crate::x509::X509;

use super::config::{CertStatusResult, OcspError};

/// SHA-1 algorithm OID (`1.3.14.3.2.26`), the mandatory-to-implement hash for
/// OCSP clients per RFC 5019 §2.2.1 and the algorithm used in the CertID
/// profile of RFC 6960 §4.1.1.
const ID_SHA1: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.14.3.2.26");

/// `CertID` per RFC 6960 §4.1.1.
///
/// ```text
/// CertID ::= SEQUENCE {
///     hashAlgorithm  AlgorithmIdentifier,
///     issuerNameHash OCTET STRING,
///     issuerKeyHash  OCTET STRING,
///     serialNumber   CertificateSerialNumber }
/// ```
#[derive(Sequence)]
struct CertId {
    hash_algorithm: AlgorithmIdentifierOwned,
    issuer_name_hash: OctetString,
    issuer_key_hash: OctetString,
    serial_number: SerialNumber,
}

/// `Request` per RFC 6960 §4.1.1, with the optional `singleRequestExtensions`
/// omitted.
///
/// ```text
/// Request ::= SEQUENCE {
///     reqCert                 CertID,
///     singleRequestExtensions [0] EXPLICIT Extensions OPTIONAL }
/// ```
#[derive(Sequence)]
struct SingleRequest {
    req_cert: CertId,
}

/// `TBSRequest` per RFC 6960 §4.1.1, with `version`, `requestorName` and
/// `requestExtensions` left at defaults; only the mandatory `requestList`
/// (a `SEQUENCE OF Request`) is populated.
#[derive(Sequence)]
struct TbsRequest {
    request_list: Vec<SingleRequest>,
}

/// `OCSPRequest` per RFC 6960 §4.1.1, with the optional `optionalSignature`
/// omitted.
///
/// ```text
/// OCSPRequest ::= SEQUENCE {
///     tbsRequest        TBSRequest,
///     optionalSignature [0] EXPLICIT Signature OPTIONAL }
/// ```
#[derive(Sequence)]
struct OcspRequest {
    tbs_request: TbsRequest,
}

/// Encode a DER-encoded `OCSPRequest` per RFC 6960 §4.1.1, querying the
/// revocation status of `cert` (issued by `issuer`).
///
/// The `CertID` is built using SHA-1 — the mandatory-to-implement hash for
/// OCSP clients conforming to RFC 5019 §2.2.1 and the algorithm named in the
/// CertID profile of RFC 6960 §4.1.1. The issuer name hash is the SHA-1 of
/// the DER encoding of the issuer's subject distinguished name, and the
/// issuer key hash is the SHA-1 of the issuer's `subjectPublicKey` BIT STRING
/// value (its raw bytes).
pub fn encode_ocsp_request(issuer: &Certificate, cert: &Certificate) -> Result<Vec<u8>, String> {
    let issuer_name_der = issuer
        .tbs_certificate
        .subject
        .to_der()
        .map_err(|e| format!("failed to DER-encode issuer subject DN for CertID: {e}"))?;
    let issuer_name_hash = Sha1::digest(&issuer_name_der);

    let issuer_key_bytes = issuer
        .tbs_certificate
        .subject_public_key_info
        .subject_public_key
        .raw_bytes();
    let issuer_key_hash = Sha1::digest(issuer_key_bytes);

    let cert_id = CertId {
        hash_algorithm: AlgorithmIdentifierOwned {
            oid: ID_SHA1,
            parameters: Some(Null.into()),
        },
        issuer_name_hash: OctetString::new(issuer_name_hash.to_vec())
            .map_err(|e| format!("failed to build issuerNameHash octet string: {e}"))?,
        issuer_key_hash: OctetString::new(issuer_key_hash.to_vec())
            .map_err(|e| format!("failed to build issuerKeyHash octet string: {e}"))?,
        serial_number: cert.tbs_certificate.serial_number.clone(),
    };

    let ocsp_request = OcspRequest {
        tbs_request: TbsRequest {
            request_list: vec![SingleRequest { req_cert: cert_id }],
        },
    };

    let der = ocsp_request
        .to_der()
        .map_err(|e| format!("failed to DER-encode OCSPRequest: {e}"))?;

    trace!(
        "Encoded OCSPRequest ({} bytes) for cert serial {:?}",
        der.len(),
        cert.tbs_certificate.serial_number.as_bytes()
    );
    Ok(der)
}

pub fn build_ocsp_request(end_entity: &X509, issuer: &X509) -> Result<Vec<u8>, OcspError> {
    let issuer_cert = issuer.inner();
    let ee_cert = end_entity.inner();

    let request = x509_ocsp::Request::from_cert::<Sha1>(issuer_cert, ee_cert)
        .map_err(|e| OcspError::InvalidResponse(format!("failed to build CertID: {e}")))?;

    let ocsp_req = OcspRequestBuilder::default().with_request(request).build();

    let der = ocsp_req
        .tbs_request
        .to_der()
        .map_err(|e| OcspError::InvalidResponse(format!("failed to encode OCSPRequest: {e}")))?;

    trace!(
        "Built OCSP request ({} bytes) for cert serial {:?}",
        der.len(),
        end_entity.serial_number()
    );
    Ok(der)
}

pub fn decode_ocsp_response(
    response_der: &[u8],
    expected_serial: &[u8],
) -> Result<CertStatusResult, OcspError> {
    let response = OcspResponse::from_der(response_der)
        .map_err(|e| OcspError::InvalidResponse(format!("failed to decode OCSPResponse: {e}")))?;

    if response.response_status != OcspResponseStatus::Successful {
        return Err(OcspError::InvalidResponse(format!(
            "OCSP response status is not successful: {:?}",
            response.response_status
        )));
    }

    let response_bytes = response
        .response_bytes
        .ok_or_else(|| OcspError::InvalidResponse("OCSP response has no responseBytes".into()))?;

    if response_bytes.response_type != const_oid::db::rfc6960::ID_PKIX_OCSP_BASIC {
        return Err(OcspError::InvalidResponse(
            "OCSP responseBytes type is not id-pkix-ocsp-basic".into(),
        ));
    }

    let basic = x509_ocsp::BasicOcspResponse::from_der(response_bytes.response.as_bytes())
        .map_err(|e| {
            OcspError::InvalidResponse(format!("failed to decode BasicOcspResponse: {e}"))
        })?;

    for single in &basic.tbs_response_data.responses {
        if single.cert_id.serial_number.as_bytes() != expected_serial {
            continue;
        }

        return match single.cert_status {
            CertStatus::Good(_) => Ok(CertStatusResult::Good),
            CertStatus::Revoked(_) => Ok(CertStatusResult::Revoked),
            CertStatus::Unknown(_) => Ok(CertStatusResult::Unknown),
        };
    }

    Err(OcspError::InvalidResponse(
        "no matching serial number in OCSP response".into(),
    ))
}
