# Phase 1 Data Model: Transport Asymmetric Crypto Offload

This feature adds no wire types and no persisted data. The "entities" are the **owned
value bundles** that cross the async→blocking-thread boundary, and the **crypto-core functions**
that consume them. Everything here is internal (`pub(crate)` at most); no public umbrella API.

## Entity: owned sign-and-encrypt input

Carries everything `asymmetric_sign_and_encrypt` needs, owned, so the closure is `Send + 'static`.

| Field | Type | Source | Notes |
|-------|------|--------|-------|
| `security_policy` | `SecurityPolicy` | channel | `Copy` |
| `signing_key` | `PrivateKey` | `self.private_key.clone()` | `Clone`; server/client own key |
| `encryption_key` | `Option<PublicKey>` | `self.remote_cert.public_key()` | `None` for ECC |
| `src` | `Vec<u8>` | the padded chunk plaintext (owned) | already sequence-stamped |
| `encrypted_range` | `Range<usize>` | computed pre-offload | plain-text region |

**Output**: `Result<Vec<u8>, StatusCode>` — the owned signed+encrypted chunk bytes (previously
written into a borrowed `dst`; now returned owned and copied into the send buffer by the caller).

## Entity: owned decrypt-and-verify input

Carries everything `asymmetric_decrypt_and_verify` needs, owned.

| Field | Type | Source | Notes |
|-------|------|--------|-------|
| `security_policy` | `SecurityPolicy` | channel | `Copy` |
| `our_private_key` | `PrivateKey` | `self.private_key.clone()` | decrypts the body |
| `our_cert` | `X509` | `self.cert.clone()` | identifies which cert the peer used |
| `verification_key` | `PublicKey` | derived from sender cert (async ctx) | verifies signature |
| `receiver_thumbprint` | `ByteString` | security header | `Clone` |
| `src` | `Vec<u8>` | inbound chunk bytes (owned) | |
| `encrypted_range` | `Range<usize>` | computed pre-offload | |

**Output**: `Result<Vec<u8>, Error>` — the owned decrypted bytes (length preserved by the
existing `update_message_size_and_truncate` logic in the caller).

> Implementation note: these may be passed as explicit function parameters or grouped into a
> small `pub(crate)` struct to keep signatures readable — a mechanical choice left to
> implementation, not a design decision. Either way the fields and ownership above are the
> contract.

## Function: `asymmetric_sign_and_encrypt_owned` (crypto core)

- **Location**: `async-opcua-core/src/comms/secure_channel.rs` (free fn or associated fn).
- **Purity**: no `&self`, no thread-local, no I/O — pure CPU. `Send + 'static`. Panic-free.
- **Body**: the current `asymmetric_sign_and_encrypt` logic, parameterized on owned inputs, using
  a local scratch `Vec` in place of `PADDING_AND_SIGNATURE_SCRATCH`.
- **Wrapper**: `SecureChannel::asymmetric_sign_and_encrypt(&self, …)` clones its material and
  delegates, preserving the existing signature for sync callers/tests.

## Function: `asymmetric_decrypt_and_verify_owned` (crypto core)

- **Location**: `secure_channel.rs`. Same purity/`Send`/panic-free constraints.
- **Body**: the current `asymmetric_decrypt_and_verify` logic on owned inputs.
- **Wrapper**: `SecureChannel::asymmetric_decrypt_and_verify(&self, …)` delegates.

## State & lifecycle (offload boundary)

```
async ctx: borrow &SecureChannel (or &mut) briefly
          → clone owned inputs (PrivateKey, cert/pubkey, chunk bytes, ranges)
          → spawn_blocking(move || core(owned…))        [runs on tokio blocking pool]
          → .await → Result<Vec<u8>, {StatusCode|Error}>  (inner crypto result, verbatim)
          → on JoinError (task panic): drop connection, do not mask inner fault
          → splice owned bytes into send buffer / continue decode
```

Invariants (see contracts): ordering preserved (single OSC chunk, seq stamped pre-crypto);
status codes preserved; symmetric path never enters this boundary.
