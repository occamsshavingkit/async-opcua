# Research: Address Space Write Enforcement Completion

## CU 2820 — Address Space Full Array Only (WriteFullArrayOnly)

**Official CU description** (from the OPC UA Foundation normalized profile
snapshot): "Support setting the WriteFullArrayOnly flag in the AccessLevelEx
Attribute for Variable Nodes of non-scalar data types to indicate whether
write operations for an array can be performed with an IndexRange."

**Decision**: Enforce the flag in
`async-opcua-server/src/address_space/write_validation.rs`,
`validate_node_write_inner`, in the `NodeType::Variable(var)` arm — the
single function called (via `AddressSpace::validate_node_write` /
`validate_node_write_in_address_space`) by both `SimpleNodeManagerImpl`'s
and `TestNodeManagerImpl`'s Write dispatch, BEFORE `write_node_value`
applies the change. When the write targets `AttributeId::Value`,
`has_index_range` is true, and
`var.access_level_ex() & AccessLevelExType::WriteFullArrayOnly.bits() as u32 != 0`,
return `Err(StatusCode::BadWriteNotSupported)`.

**Rationale**:
- Part 3 (Address Space Model) v1.05.06 §8.58, Table 42 — AccessLevelExType
  Definition: "WriteFullArrayOnly, Bit 10: Indicates if Write of IndexRange
  is supported (0 means Write of IndexRange is supported)." So bit=1 means
  IndexRange writes are NOT supported for that Variable.
- Part 4 (Services) v1.05.07 §5.11.4, Table 53 (Write Service Parameters,
  `indexRange` row): "A Server shall return a Bad_WriteNotSupported error
  if an indexRange is provided and writing of indexRange is not possible
  for the Node." This pins down both the trigger condition and the exact
  status code.
- `StatusCode::BadWriteNotSupported` is already used elsewhere in this
  codebase for a structurally similar case (`variant/mod.rs:1766`, a
  multi-dim range write to a non-array target), so this is consistent with
  existing conventions, not a new status-code choice.

**Where NOT to add the check** (and why): `Variable::set_value_range`
(`async-opcua-nodes/src/variable.rs:727`) and
`NodeManagerInner::set_values` (`async-opcua-server/src/node_manager/memory/mod.rs:256`)
both call the same underlying range-write logic, but they are also the
mechanism servers use to update their OWN Variables programmatically —
e.g. an alarm's source-value sampler, or a server author pushing sensor
readings. `WriteFullArrayOnly` per spec gates the OPC UA **Write service**
(a client-facing request), not a server's internal state management. Adding
the check inside the shared low-level setter would incorrectly block
legitimate internal updates.

**Alternatives considered**: Adding the check inside
`Variable::set_value_range` directly was considered and rejected for the
above reason — it's called from both the Write-service path and
internal/programmatic paths, and only the former should be gated.

## CU 2936 — Attribute Write StatusCode & Timestamp

**Official CU description**: "Supports writing of StatusCode and
Timestamps along with the Value."

**Investigation finding**: The AUDIT_TABLE's prior "Partial" entry described
this as needing "a test that reads value back post-Write" — but
`async-opcua/tests/integration/write.rs` already has a `write_then_read()`
helper and a `write_variable()` test that DOES write-then-read the `Value`
field (asserting `read.value == write.value.value`). So the description
behind the "Partial" status was stale/imprecise. The real, narrower gap:
the existing `write_value()` test helper (write.rs:22-37) always sends
`status: Some(StatusCode::Good)` and a freshly-generated
`source_timestamp`, and `write_then_read()` only ever asserts the Variant
payload equality — never the StatusCode or the timestamps. No existing
test proves a client-supplied *non-Good* StatusCode plus explicit,
distinct Source/Server Timestamps survive a Write and come back unchanged
on a Read with `TimestampsToReturn::Both`.

**Decision**: Add a new integration test in `write.rs` that writes a
Variable's Value with `status: Some(StatusCode::Uncertain)` and explicit,
distinct `source_timestamp`/`server_timestamp` values, then reads it back
with `TimestampsToReturn::Both` and asserts the value, status, and both
timestamps all match. No production code change needed — `write_node_value`
(`address_space/utils.rs:473`) already passes the client's status and
source timestamp straight through to `set_value_range`/`set_value_direct`.

**Rationale**: Part 4 §5.11.4, Table 53 (`value` row): "If the
SourceTimestamp or the ServerTimestamp is specified, the Server shall use
these values." This is a direct, testable spec requirement independent of
whether the value itself changed.

## CU 4237 — Address Space NonVolatile and Constant

**Official CU description**: "Support setting the NonVolatile and Constant
flags in the AccessLevelEx Attribute for Variable Nodes to indicate whether
persistent storage is supported."

**Rationale for scope (Part 3 §8.58, Table 42)**:
- Bit 12, NonVolatile: "0 means it is volatile or not known to be, 1 means
  non-volatile."
- Bit 13, Constant: "0 means the Value is not constant, 1 means the Value
  is constant."
- Table 41 (Use Cases of Constant and NonVolatile Fields) documents this is
  purely a descriptive/informational pair of flags — the CU is about being
  able to correctly SET and READ these bits, not about actually enforcing
  persistence (this server has no persistent-storage backend for arbitrary
  Variables to enforce against in the first place, and the CU text itself
  only asks for the flag-setting capability).

**Investigation finding**: `Variable::access_level_ex()` /
`set_access_level_ex_attribute()` (`async-opcua-nodes/src/variable.rs:826-838`)
already implement a fully generic bitmask pass-through for the entire
`AccessLevelEx` attribute — every bit including 12 and 13 already works
correctly as a mechanical consequence of the feature-053 AccessLevelEx
work. The existing `access_level_ex_tests` module (variable.rs:937+) only
happens to exercise other bits.

**Decision**: Add a targeted test — either a unit test in the
`access_level_ex_tests` module (`variable.rs`) or an integration test in
`write.rs`/`read.rs` using `VariableBuilder::access_level_ex(...)` — that
sets `AccessLevelExType::NonVolatile | AccessLevelExType::Constant` and
confirms a Read of the `AccessLevelEx` attribute returns both bits. No
production code change needed; this closes the CU via test evidence only.

## Status code confirmation

`StatusCode::BadWriteNotSupported` (`async-opcua-types/src/status_code.rs:704`):
"The server does not support writing the combination of value, status and
timestamps provided." — generic enough to cover the IndexRange-not-
supported case per Part 4 Table 53's explicit instruction to use
`Bad_WriteNotSupported` for this exact scenario.
