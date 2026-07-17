# Quickstart: Address Space Write Enforcement Completion

## Verify CU 2820 (WriteFullArrayOnly enforcement)

1. Build a Variable with an array value (e.g. `Variant::from(vec![1i32, 2, 3])`)
   and `.access_level_ex(AccessLevelExType::CurrentRead | AccessLevelExType::CurrentWrite | AccessLevelExType::WriteFullArrayOnly)`.
2. Send a `Write` with a non-empty `IndexRange` (e.g. `"1"`) targeting that
   Variable's `Value` attribute.
3. Expect the Write result status to be `BadWriteNotSupported`, and a
   subsequent `Read` of the Value to show the array unchanged.
4. Send a `Write` with no `IndexRange` (full array replace) to the same
   Variable. Expect `Good`, and the new value to read back correctly.
5. Repeat steps 2-3 against an equivalent Variable WITHOUT
   `WriteFullArrayOnly` set — expect the IndexRange Write to succeed
   (regression check — this server's existing IndexRange write support,
   CU 3147, must be untouched).

## Verify CU 2936 (StatusCode & Timestamp write)

1. Build a writable scalar Variable.
2. Send a `Write` with `status: Some(StatusCode::Uncertain)`,
   `source_timestamp` and `server_timestamp` both set to distinct,
   deliberately-chosen values (not "now").
3. `Read` the Variable with `TimestampsToReturn::Both`.
4. Expect the returned value, status, source_timestamp, and
   server_timestamp to all match what was written.

## Verify CU 4237 (NonVolatile / Constant)

1. Build a Variable with
   `.access_level_ex(AccessLevelExType::CurrentRead | AccessLevelExType::NonVolatile | AccessLevelExType::Constant)`.
2. Read the `AccessLevelEx` attribute.
3. Expect both the `NonVolatile` (bit 12) and `Constant` (bit 13) flags to
   be present in the returned bitmask.

## Full verification

```bash
cargo test -p async-opcua-server address_level_ex   # or the specific new unit test name
cargo test -p async-opcua --test integration_tests -- integration::write::
cargo clippy --all-targets --all-features
cargo fmt --all -- --check
tools/ci-playbook.sh --ci
```
