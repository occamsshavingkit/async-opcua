//! OCSP responder: build signed OCSP responses per RFC 6960.

use std::collections::HashMap;
use std::time::Duration;
use std::time::SystemTime;

use der::asn1::{BitString, Null};
use der::{Decode, Encode};
use x509_cert::ext::pkix::CrlReason;
use x509_cert::ext::{AsExtension, Extension};
use x509_cert::name::Name;
use x509_cert::spki::AlgorithmIdentifierOwned;
use x509_cert::time::Time;
use x509_ocsp::{
    BasicOcspResponse, CertStatus, OcspGeneralizedTime, OcspRequest, OcspResponse, ResponderId,
    ResponseData, RevokedInfo, SingleResponse,
};

use crate::x509::X509;
use crate::{KeySize, PrivateKey};

use super::config::OcspError;

/// Configuration for an OCSP responder instance.
pub struct OcspResponderConfig {
    /// The certificate whose key signs responses (the CA certificate).
    pub signer_cert: X509,
    /// The private key for signing responses.
    pub signer_key: PrivateKey,
    /// How long produced responses are valid for (next_update = now + validity).
    pub response_validity: Duration,
    /// Certificate status database: serial_number (DER bytes) → status.
    pub status_db: HashMap<Vec<u8>, CertStatusVariant>,
}

/// The status of a certificate known to the responder.
pub enum CertStatusVariant {
    Good,
    Revoked {
        /// Time of revocation (generalized time or UTC time per RFC 5280).
        revocation_time: Time,
        /// Optional CRL reason code (RFC 5280 §5.3.1).
        revocation_reason: Option<u32>,
    },
    Unknown,
}

/// Maps a raw RFC 5280 §5.3.1 CRL reason code to its `CrlReason` enum. Values
/// outside the defined set (including the reserved `(7)`) collapse to
/// `Unspecified`, which is the safest conveyable reason per RFC 5280.
fn crl_reason_from_code(code: u32) -> CrlReason {
    match code {
        1 => CrlReason::KeyCompromise,
        2 => CrlReason::CaCompromise,
        3 => CrlReason::AffiliationChanged,
        4 => CrlReason::Superseded,
        5 => CrlReason::CessationOfOperation,
        6 => CrlReason::CertificateHold,
        8 => CrlReason::RemoveFromCRL,
        9 => CrlReason::PrivilegeWithdrawn,
        10 => CrlReason::AaCompromise,
        _ => CrlReason::Unspecified,
    }
}

/// Builds a DER-encoded, signed `OCSPResponse` (RFC 6960 §4.2.1) for the
/// certificates queried by `request_der` (an `OCSPRequest`, RFC 6960 §4.1.1).
///
/// For each requested certificate the responder looks up the serial number
/// (the DER bytes of the `CertID.serialNumber`) in `config.status_db` and emits
/// a `SingleResponse`. A serial not present in the database is reported as
/// `unknown` (RFC 6960 §4.2.1) — this is a status, not an error.
///
/// `thisUpdate` is the current time and `nextUpdate` is the current time plus
/// `config.response_validity`, following RFC 6960 §4.2.3. The response is
/// signed with RSA-SHA256 (PKCS#1 v1.5) using `config.signer_key`, and
/// `config.signer_cert` is embedded in the `certs` field so clients can verify
/// without an out-of-band trust anchor.
///
/// A malformed request yields an `OCSPResponse` with status `malformedRequest`
/// (RFC 6960 §4.2.1) returned as `Ok(..)`; only encoding or signing failures
/// are reported via `Err`.
pub fn build_ocsp_response(
    request_der: &[u8],
    config: &OcspResponderConfig,
) -> Result<Vec<u8>, OcspError> {
    // RFC 6960 §4.1.1: decode the OCSPRequest. A request we cannot parse is,
    // by §4.2.1, a `malformedRequest` — it is signalled in-band, not as an
    // error, so that a broken client still receives a well-formed reply.
    let request = match OcspRequest::from_der(request_der) {
        Ok(req) => req,
        Err(_) => {
            let malformed = OcspResponse::malformed_request();
            let der = malformed.to_der().map_err(|e| {
                OcspError::InvalidResponse(format!("failed to encode malformed OCSPResponse: {e}"))
            })?;
            return Ok(der);
        }
    };

    // RFC 6960 §4.4.1: if the request carried an OCSP nonce extension in
    // `tbsRequest.requestExtensions`, the response MUST echo it back in
    // `responseData.responseExtensions`. The nonce cryptographically binds
    // this response to the request, preventing replay by a man-in-the-middle.
    // A request without a nonce yields no response extension; a request whose
    // nonce fails to decode is treated as having none (the rest of the
    // response is still well-formed and useful to compliant clients).
    let response_extensions: Option<Vec<Extension>> = request
        .nonce()
        .map(|nonce| {
            nonce
                .to_extension(&Name::default(), &[])
                .map(|ext| vec![ext])
        })
        .transpose()
        .map_err(|e| {
            OcspError::InvalidResponse(format!("failed to encode OCSP nonce extension: {e}"))
        })?;

    // RFC 6960 §4.2.3: thisUpdate is now; nextUpdate is now + validity.
    let now = SystemTime::now();
    let this_update = OcspGeneralizedTime::try_from(now)
        .map_err(|e| OcspError::InvalidResponse(format!("failed to encode thisUpdate: {e}")))?;
    let next_update = OcspGeneralizedTime::try_from(now + config.response_validity)
        .map_err(|e| OcspError::InvalidResponse(format!("failed to encode nextUpdate: {e}")))?;

    // One SingleResponse per requested certificate (RFC 6960 §4.2.1).
    let request_list = &request.tbs_request.request_list;
    let mut responses = Vec::with_capacity(request_list.len());
    for req in request_list {
        let serial = req.req_cert.serial_number.as_bytes();
        let cert_status = match config.status_db.get(serial) {
            Some(CertStatusVariant::Good) => CertStatus::good(),
            Some(CertStatusVariant::Revoked {
                revocation_time,
                revocation_reason,
            }) => CertStatus::revoked(RevokedInfo {
                revocation_time: OcspGeneralizedTime::from(*revocation_time),
                revocation_reason: (*revocation_reason).map(crl_reason_from_code),
            }),
            // RFC 6960 §4.2.1: unknown covers both the explicit Unknown
            // variant and serials absent from the status database.
            Some(CertStatusVariant::Unknown) | None => CertStatus::unknown(),
        };

        let single = SingleResponse::new(req.req_cert.clone(), cert_status, this_update)
            .with_next_update(next_update);
        responses.push(single);
    }

    // RFC 6960 §4.2.1: ResponseData. producedAt is the time of signing; the
    // responder is identified by its certificate subject name (byName).
    let signer_cert = config.signer_cert.inner();
    let responder_id = ResponderId::ByName(signer_cert.tbs_certificate.subject.clone());
    let response_data = ResponseData {
        version: x509_ocsp::Version::V1,
        responder_id,
        produced_at: this_update,
        responses,
        response_extensions,
    };

    // The to-be-signed bytes are the DER encoding of ResponseData.
    let tbs = response_data
        .to_der()
        .map_err(|e| OcspError::InvalidResponse(format!("failed to encode ResponseData: {e}")))?;

    // Sign with RSA-SHA256 (PKCS#1 v1.5).
    let signature_algorithm = AlgorithmIdentifierOwned {
        oid: const_oid::db::rfc5912::SHA_256_WITH_RSA_ENCRYPTION,
        parameters: Some(Null.into()),
    };
    let mut sig_buf = vec![0u8; config.signer_key.size()];
    let signed = config
        .signer_key
        .sign_sha256(&tbs, &mut sig_buf)
        .map_err(|e| OcspError::InvalidResponse(format!("failed to sign OCSP response: {e}")))?;
    sig_buf.truncate(signed);
    let signature = BitString::from_bytes(&sig_buf).map_err(|e| {
        OcspError::InvalidResponse(format!("failed to build signature BitString: {e}"))
    })?;

    let basic = BasicOcspResponse {
        tbs_response_data: response_data,
        signature_algorithm,
        signature,
        certs: Some(vec![signer_cert.clone()]),
    };

    let response = OcspResponse::successful(basic).map_err(|e| {
        OcspError::InvalidResponse(format!("failed to wrap BasicOcspResponse: {e}"))
    })?;

    response
        .to_der()
        .map_err(|e| OcspError::InvalidResponse(format!("failed to encode OCSPResponse: {e}")))
}
