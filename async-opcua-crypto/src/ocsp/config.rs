use std::fmt;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OcspFetchPolicy {
    Off,
    Soft,
    Strict,
}

#[derive(Clone, Debug)]
pub struct OcspFetchConfig {
    pub policy: OcspFetchPolicy,
    pub timeout: Duration,
    pub max_response_size: usize,
}

impl Default for OcspFetchConfig {
    fn default() -> Self {
        Self {
            policy: OcspFetchPolicy::Off,
            timeout: Duration::from_secs(5),
            max_response_size: 65536,
        }
    }
}

#[derive(Debug)]
pub enum OcspError {
    NoAiaExtension,
    NoOcspUrl,
    NoIssuerCert,
    FetchFailed(String),
    InvalidResponse(String),
    SignatureInvalid(String),
}

impl fmt::Display for OcspError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAiaExtension => write!(f, "certificate has no AIA extension"),
            Self::NoOcspUrl => write!(f, "AIA extension contains no OCSP URL"),
            Self::NoIssuerCert => write!(f, "issuer certificate not available"),
            Self::FetchFailed(msg) => write!(f, "OCSP fetch failed: {msg}"),
            Self::InvalidResponse(msg) => write!(f, "invalid OCSP response: {msg}"),
            Self::SignatureInvalid(msg) => write!(f, "OCSP signature invalid: {msg}"),
        }
    }
}

impl std::error::Error for OcspError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CertStatusResult {
    Good,
    Revoked,
    Unknown,
}
