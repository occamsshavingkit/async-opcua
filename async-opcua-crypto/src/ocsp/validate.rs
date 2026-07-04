use chrono::{DateTime, Utc};
use der::{Decode, Encode};
use tracing::trace;
use x509_ocsp::{BasicOcspResponse, CertStatus, OcspResponse, OcspResponseStatus};

use crate::PublicKey;

use super::config::{CertStatusResult, OcspError};

pub fn validate_ocsp_response(
    response_der: &[u8],
    expected_serial: &[u8],
    issuer_public_key: &PublicKey,
    now: &DateTime<Utc>,
) -> Result<CertStatusResult, OcspError> {
    let response = OcspResponse::from_der(response_der)
        .map_err(|e| OcspError::InvalidResponse(format!("failed to decode OcspResponse: {e}")))?;

    if response.response_status != OcspResponseStatus::Successful {
        return Err(OcspError::InvalidResponse(format!(
            "OCSP response status is {:?}, expected Successful",
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

    let basic = BasicOcspResponse::from_der(response_bytes.response.as_bytes()).map_err(|e| {
        OcspError::InvalidResponse(format!("failed to decode BasicOcspResponse: {e}"))
    })?;

    verify_ocsp_signature(&basic, issuer_public_key)?;

    for single in &basic.tbs_response_data.responses {
        if single.cert_id.serial_number.as_bytes() != expected_serial {
            continue;
        }

        if !is_fresh(single, now) {
            return Err(OcspError::InvalidResponse(
                "OCSP response is outside its validity window".into(),
            ));
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

fn verify_ocsp_signature(
    basic: &BasicOcspResponse,
    issuer_public_key: &PublicKey,
) -> Result<(), OcspError> {
    let tbs = basic
        .tbs_response_data
        .to_der()
        .map_err(|e| OcspError::SignatureInvalid(format!("failed to encode ResponseData: {e}")))?;

    let signature = basic.signature.as_bytes().ok_or_else(|| {
        OcspError::SignatureInvalid("OCSP response signature is not octet-aligned".into())
    })?;

    let algorithm_oid = basic.signature_algorithm.oid;

    let verified = if algorithm_oid == const_oid::db::rfc5912::SHA_256_WITH_RSA_ENCRYPTION {
        issuer_public_key
            .verify_sha256(&tbs, signature)
            .is_ok_and(|ok| ok)
    } else if algorithm_oid == const_oid::db::rfc5912::ID_RSASSA_PSS {
        issuer_public_key
            .verify_sha256_pss(&tbs, signature)
            .is_ok_and(|ok| ok)
    } else if algorithm_oid == const_oid::db::rfc5912::SHA_1_WITH_RSA_ENCRYPTION {
        issuer_public_key
            .verify_sha1(&tbs, signature)
            .is_ok_and(|ok| ok)
    } else if algorithm_oid == const_oid::db::rfc5912::ECDSA_WITH_SHA_256
        || algorithm_oid == const_oid::db::rfc5912::ECDSA_WITH_SHA_384
    {
        #[cfg(feature = "ecc")]
        {
            issuer_public_key.ecc_key().is_some_and(|ecc_key| {
                crate::ecc::ecdsa_verify_der(ecc_key, &tbs, signature).is_ok()
            })
        }
        #[cfg(not(feature = "ecc"))]
        {
            false
        }
    } else {
        false
    };

    if verified {
        trace!("OCSP response signature verified");
        Ok(())
    } else {
        Err(OcspError::SignatureInvalid(
            "OCSP response signature verification failed".into(),
        ))
    }
}

fn is_fresh(single: &x509_ocsp::SingleResponse, now: &DateTime<Utc>) -> bool {
    let now_sys: std::time::SystemTime = (*now).into();
    let this_ok = single.this_update.0.to_system_time() <= now_sys;
    let next_ok = single
        .next_update
        .as_ref()
        .is_none_or(|next| next.0.to_system_time() >= now_sys);
    this_ok && next_ok
}
