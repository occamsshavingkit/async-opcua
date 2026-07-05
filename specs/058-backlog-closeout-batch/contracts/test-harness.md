# Contract: Test Harness Extensions

## Scope

Enhance the two foundation-profile test harnesses (`embedded` and `standard`) with the helpers needed to un-ignore deferred tests.

## Embedded Profile Harness (`samples/foundation-profile-embedded-server/tests/common/mod.rs`)

### New Helper: `connect_secure_two_phase`

```rust
/// Two-phase secure connect: fetch server cert via None, then connect with Sign&Encrypt.
///
/// Phase 1: Connect with policy None to get endpoints (includes server cert).
/// Phase 2: Extract the server cert from the endpoint description.
/// Phase 3: Reconnect with Sign&Encrypt using the discovered server cert.
pub async fn connect_secure_two_phase(tester: &EmbeddedTester) -> Arc<Session>;
```

**Behavior**:
1. Creates a client with `create_sample_keypair(true)` and `trust_server_certs(true)`
2. Calls `client.get_endpoints(tester.url.as_str())` over a None-policy channel
3. Extracts the server certificate from the returned endpoint descriptions
4. Closes the None session
5. Reconnects with `SecurityPolicy::Basic256Sha256` + `MessageSecurityMode::SignAndEncrypt` using the discovered cert

**Error handling**: Panics (test-fail) if the server has no endpoints or no certificate.

### Modified Test: `secure_channel_basic256sha256_sign_encrypt`

Changes from current (`#[ignore]`d):
1. Replace `connect_secure(&tester)` with `connect_secure_two_phase(&tester)`
2. Remove `#[ignore]` attribute
3. Keep the rest of the test identical (Read of ServerStatus)

## Standard Profile Harness (`samples/foundation-profile-standard-server/tests/common/mod.rs`)

### New Helper: `connect_secure_two_phase`

Same as embedded version, using `StandardTester` instead of `EmbeddedTester`.

### New Helper: `spawn_lds_peer`

```rust
/// Spawn an in-process LDS (Local Discovery Server) peer on an ephemeral port.
/// Returns the LDS URL so the standard server can register with it.
pub async fn spawn_lds_peer() -> LdsPeer;
```

**`LdsPeer` struct**:
- `url: String` — the LDS endpoint URL
- `handle: ServerHandle` — for cleanup on drop

**Behavior**:
1. Creates a server with `discovery-mdns` feature enabled
2. Configures it as an LDS (sets `is_discovery_server: true`)
3. Starts on an ephemeral port

### Modified Test: `x509_user_token_activation`

Changes from current (`#[ignore]`d):
1. Add `connect_secure_two_phase` call instead of `connect`
2. Create `IdentityToken::X509` using existing test cert paths (`tests/x509/user_cert.der`, `tests/x509/user_private_key.pem`)
3. Assign the X509 token to the session activation
4. Remove `#[ignore]` attribute
5. Assert that the session activates successfully (Read of ServerStatus returns Good)

### Modified Test: `register_server2_flow`

Changes from current (`#[ignore]`d):
1. Call `spawn_lds_peer()` to create an in-process LDS
2. Configure the standard server with the LDS URL in its discovery configuration
3. Start the standard server
4. Poll the LDS for registered servers over a timeout (e.g., 10 seconds)
5. Assert that the standard server appears in the LDS registry
6. Remove `#[ignore]` attribute

## Non-goals

- Modifying the `Tester` harness in `async-opcua/tests/utils/tester.rs` — the profile tests use their own lightweight harnesses
- Adding two-phase connect to the main client library (`connect_to_matching_endpoint`) — scoped to test helpers only
- Full LDS-ME mDNS testing — the RegisterServer2 test uses in-process discovery, not network mDNS
