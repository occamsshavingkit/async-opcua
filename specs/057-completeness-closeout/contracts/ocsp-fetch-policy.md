# Contract: OCSP Fetch Policy

**Feature**: 057-completeness-closeout / US1
**Part of**: `async-opcua-crypto::CertificateStore`

## Public API

### OCSP Fetch Policy Enum

```rust
pub enum OcspFetchPolicy {
    /// No live fetching — only stapled/supplied OCSP responses (backward-compatible default).
    Off,
    /// Fetch live; fall back to CRL if unreachable.
    Soft,
    /// Fetch live; hard-fail if unreachable.
    Strict,
}
```

### OCSP Fetch Configuration

```rust
pub struct OcspFetchConfig {
    pub policy: OcspFetchPolicy,
    pub timeout: Duration,        // default: 5s
    pub max_response_size: usize, // default: 65536
}
```

### CertificateStore Integration

```rust
impl CertificateStore {
    /// Configure live OCSP fetching. Calling this method with `policy: Off`
    /// preserves the current behavior (stapled/supplied only).
    pub fn set_ocsp_fetch_config(&mut self, config: OcspFetchConfig);

    /// Fetch an OCSP response for a certificate from its AIA extension.
    /// Called internally during chain validation when policy is Soft or Strict.
    fn fetch_ocsp_response(&self, cert: &Certificate, issuer: &Certificate)
        -> Result<OcspResponse, OcspError>;
}
```

## Invariants

- **Default is Off**: `CertificateStore` default-constructs with `OcspFetchPolicy::Off`.
- **Fail-closed on Strict**: Unreachable responder → reject connection.
- **Fail-open on Soft**: Unreachable responder → proceed to CRL check.
- **Timeout honored**: HTTP fetch must not block longer than `timeout`; the caller uses `std::thread::spawn` + `thread::join(timeout)` or equivalent to bound the sync IO.
- **Response size bounded**: Responses exceeding `max_response_size` are rejected before full buffering.

## OCSP Request Format

Per RFC 6960 §4.1.1:
```
OCSPRequest ::= SEQUENCE {
    tbsRequest    TBSRequest
}

TBSRequest ::= SEQUENCE {
    version       [0] EXPLICIT Version DEFAULT v1,
    requestorName [1] EXPLICIT GeneralName OPTIONAL,
    requestList   SEQUENCE OF Request,
    requestExtensions [2] EXPLICIT Extensions OPTIONAL
}

Request ::= SEQUENCE {
    reqCert CertID
}

CertID ::= SEQUENCE {
    hashAlgorithm   AlgorithmIdentifier,
    issuerNameHash  OCTET STRING,    -- SHA-1 of issuer DN
    issuerKeyHash   OCTET STRING,    -- SHA-1 of issuer public key
    serialNumber    CertificateSerialNumber
}
```

## OCSP Response Validation

Per RFC 6960 §4.2.2.3 and OPC UA Part 4 §6.1.3:
1. Response status must be `successful` (0)
2. Response signature must verify against the responder certificate
3. Responder certificate must chain to a trusted root
4. `thisUpdate` ≤ now ≤ `nextUpdate` (or within configurable skew)
5. `producedAt` must be recent (configurable max age)
6. Nonce match if request included a nonce (optional, recommended)

## Error Handling

| Condition | Off mode | Soft mode | Strict mode |
|-----------|----------|-----------|-------------|
| No AIA extension | Continue | Continue | Continue |
| Unreachable responder | N/A | Continue (CRL) | Reject |
| Invalid OCSP response | N/A | Continue (CRL) | Reject |
| Response: REVOKED | N/A | Reject | Reject |
| Response: UNKNOWN | N/A | Continue (CRL) | Reject |
