//! GSSAPI/Kerberos identity token validator (OPC 10000-6 §6.4 IssuedToken).
//!
//! Validates Kerberos service tickets carried as OPC UA IssuedToken data,
//! using the GSSAPI `gss_accept_sec_context` flow.

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use opcua_types::status_code::StatusCode;
use tracing::{debug, warn};

use super::{ClaimProfile, OAuth2IdentityValidator};

/// Maximum GSSAPI token size (64 KB) before rejection.
const MAX_TOKEN_SIZE: usize = 64 * 1024;

/// Timeout for each GSSAPI context step (5 seconds).
const GSSAPI_STEP_TIMEOUT: Duration = Duration::from_secs(5);

/// GSSAPI-backed Kerberos identity validator.
///
/// Validates base64-encoded GSSAPI tokens against a configured service
/// principal and keytab, extracting the client principal name as the
/// authenticated identity.
pub struct GssapiIdentityValidator {
    spn: String,
    keytab_path: Option<PathBuf>,
}

impl GssapiIdentityValidator {
    /// Create a new GSSAPI Kerberos validator.
    #[must_use]
    pub fn new(spn: String, keytab_path: Option<PathBuf>) -> Self {
        Self { spn, keytab_path }
    }

    /// Probe whether the GSSAPI library is available.
    pub fn probe_library() -> Result<(), String> {
        let result = std::panic::catch_unwind(|| {
            let _ = libgssapi::name::Name::new(b"probe", None::<&libgssapi::oid::Oid>);
        });
        match result {
            Ok(Ok(())) | Ok(Err(_)) => Ok(()),
            Err(_) => Err("GSSAPI/Kerberos library not available (libkrb5 or libgssapi)".into()),
        }
    }
}

impl OAuth2IdentityValidator for GssapiIdentityValidator {
    fn validate_token(&self, token: &str) -> Result<ClaimProfile, StatusCode> {
        // Strip "GSSAPI " prefix if present
        let encoded = token
            .strip_prefix("GSSAPI ")
            .unwrap_or(token)
            .trim();

        // Decode base64
        use base64::Engine;
        let token_bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| {
                warn!("Kerberos token is not valid base64");
                StatusCode::BadIdentityTokenRejected
            })?;

        // Enforce size limit
        if token_bytes.len() > MAX_TOKEN_SIZE {
            warn!(
                "Kerberos token too large: {} bytes (max {})",
                token_bytes.len(),
                MAX_TOKEN_SIZE
            );
            return Err(StatusCode::BadIdentityTokenRejected);
        }

        let spn = self.spn.clone();
        let start = Instant::now();

        // Run GSSAPI in blocking context
        let principal =
            std::thread::Builder::new()
                .name("kerberos-gssapi".into())
                .spawn(move || -> Result<String, StatusCode> {
                    use libgssapi::context::{SecurityContext, ServerCtx};
                    use libgssapi::credential::{Cred, CredUsage};
                    use libgssapi::name::Name;
                    use libgssapi::oid::{OidSet, GSS_MECH_KRB5, GSS_NT_HOSTBASED_SERVICE};

                    let mut mechs = OidSet::new();
                    mechs
                        .add(GSS_MECH_KRB5.to_owned())
                        .map_err(|_| StatusCode::BadIdentityTokenRejected)?;

                    let name = Name::new(spn.as_bytes(), Some(&GSS_NT_HOSTBASED_SERVICE))
                        .map_err(|_| StatusCode::BadIdentityTokenRejected)?;
                    let canonical = name
                        .canonicalize(Some(&GSS_MECH_KRB5))
                        .map_err(|_| StatusCode::BadIdentityTokenRejected)?;

                    let cred = Cred::acquire(
                        Some(&canonical),
                        None,
                        CredUsage::Accept,
                        Some(&mechs),
                    )
                    .map_err(|_| StatusCode::BadIdentityTokenRejected)?;

                    let mut server = ServerCtx::new(Some(cred));
                    let result = server
                        .step(&token_bytes, None)
                        .map_err(|_| StatusCode::BadIdentityTokenRejected)?;

                    match result {
                        Some(_response) => {
                            debug!("GSSAPI multi-step handshake initiated");
                            let deadline = Instant::now() + GSSAPI_STEP_TIMEOUT;
                            loop {
                                if Instant::now() > deadline {
                                    return Err(StatusCode::BadTimeout);
                                }
                                let result = server
                                    .step(b"", None)
                                    .map_err(|_| StatusCode::BadIdentityTokenRejected)?;
                                if result.is_none() {
                                    break;
                                }
                            }
                        }
                        None => {}
                    }

                    let sender = server
                        .source_name()
                        .map_err(|_| StatusCode::BadIdentityTokenRejected)?;
                    let display = sender
                        .display_name()
                        .map_err(|_| StatusCode::BadIdentityTokenRejected)?;
                    String::from_utf8(display.to_vec())
                        .map_err(|_| StatusCode::BadIdentityTokenRejected)
                })
                .map_err(|_| StatusCode::BadIdentityTokenRejected)?
                .join()
                .map_err(|_| StatusCode::BadIdentityTokenRejected)??;

        let elapsed = start.elapsed();
        debug!(
            "Kerberos GSSAPI validation complete for {} in {}ms",
            principal,
            elapsed.as_millis()
        );

        Ok(ClaimProfile {
            username: principal,
            roles: vec![],
            permissions: vec![],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_base64() {
        let validator = GssapiIdentityValidator::new("OPCUA/test@PLANT.LOCAL".into(), None);
        assert_eq!(
            validator.validate_token("!!!not-base64!!!"),
            Err(StatusCode::BadIdentityTokenRejected)
        );
    }

    #[test]
    fn rejects_empty_token() {
        let validator = GssapiIdentityValidator::new("OPCUA/test@PLANT.LOCAL".into(), None);
        assert_eq!(
            validator.validate_token(""),
            Err(StatusCode::BadIdentityTokenRejected)
        );
    }

    #[test]
    fn rejects_oversized_token() {
        let validator = GssapiIdentityValidator::new("OPCUA/test@PLANT.LOCAL".into(), None);
        let big = base64::engine::general_purpose::STANDARD.encode(&vec![0u8; MAX_TOKEN_SIZE + 1]);
        assert_eq!(
            validator.validate_token(&big),
            Err(StatusCode::BadIdentityTokenRejected)
        );
    }
}
