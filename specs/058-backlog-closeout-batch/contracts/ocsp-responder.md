# Contract: OCSP Responder

## Module

`async-opcua-crypto::ocsp::responder`

## Public API

### Types

```rust
/// The status of a certificate known to the responder.
pub enum CertStatusVariant {
    Good,
    Revoked {
        /// Time of revocation (generalized time or UTC time per RFC 5280).
        revocation_time: x509_cert::time::Time,
        /// Optional CRL reason code (RFC 5280 §5.3.1).
        revocation_reason: Option<u32>,
    },
    Unknown,
}

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
```

### Functions

```rust
/// Build and sign an OCSP response for the given DER-encoded OCSP request.
///
/// # Arguments
/// * `request_der` - DER-encoded OCSPRequest per RFC 6960 §4.1.1
/// * `config` - Responder configuration (signer, validity, status DB)
///
/// # Returns
/// * `Ok(Vec<u8>)` - DER-encoded OCSPResponse with status 0 (successful)
/// * `Err(OcspError)` - On malformed request, unknown issuer, or signing failure
///
/// # Behavior
/// - If the request contains a nonce extension, the response echoes it.
/// - If the request is malformed, returns an OCSPResponse with status
///   `malformedRequest` (1), not an Err.
/// - If a requested certificate is in the status DB, returns its status.
/// - If a requested certificate is NOT in the status DB, returns "unknown".
/// - Signs the response using config.signer_key and config.signer_cert.
pub fn build_ocsp_response(
    request_der: &[u8],
    config: &OcspResponderConfig,
) -> Result<Vec<u8>, OcspError>;
```

### Error Handling

- Malformed OCSP request → `OcspResponse` DER with status `malformedRequest` returned as Ok (not Err)
- Signing failure → `Err(OcspError::CryptoError(...))`
- Empty request list → `OcspResponse` DER with status `malformedRequest`

### Security Properties

- The responder MUST NOT panic on any input, including malformed or oversized DER
- The response signing key MUST NOT be logged or exposed in error messages
- Response size is bounded by the request size plus a fixed overhead (certificate + signature)
- Nonce extension is echoed verbatim to prevent replay attacks (RFC 6960 §4.4.1)

## Dependencies

- `x509-ocsp::OcspResponseBuilder`, `BasicOcspResponse`, `ResponseData`, `SingleResponse`
- `x509-ocsp::CertStatus` (Good, Revoked, Unknown)
- `crate::x509::X509` (for signer certificate)
- `crate::PrivateKey` (for signing)
- Existing `crate::ocsp::codec` module (reused, not modified)

## Non-goals

- HTTP transport layer — the function produces a DER response; the caller handles transport
- CRL-based status derivation — the status DB is caller-managed
- OCSP response caching — the caller decides cache strategy
- Multi-CA support — one config per responder instance
