//! OAuth2 identity validation traits and claim profiles.

use opcua_types::status_code::StatusCode;

pub mod jwt_validator;
#[cfg(feature = "kerberos")]
pub mod kerberos_validator;
pub mod rsa_kem;
pub mod rsa_oaep;

pub use jwt_validator::LocalOAuth2Validator;
#[cfg(feature = "kerberos")]
pub use kerberos_validator::GssapiIdentityValidator;
pub use rsa_kem::decrypt_rsa_kem_secret;
pub use rsa_oaep::decrypt_rsa_oaep_secret;

/// Claims extracted from a validated OAuth2 identity token.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaimProfile {
    /// Stable username or subject for the authenticated identity.
    pub username: String,
    /// Role names granted to the authenticated identity.
    pub roles: Vec<String>,
    /// Permission names granted to the authenticated identity.
    pub permissions: Vec<String>,
}

/// Validates OAuth2 JWT issued identity tokens and maps them to local claims.
pub trait OAuth2IdentityValidator: Send + Sync {
    /// Validates a JWT and returns its claim profile.
    fn validate_token(&self, token_jwt: &str) -> Result<ClaimProfile, StatusCode>;
}
