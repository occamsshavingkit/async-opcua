# Quickstart: Data Access Conformance Completion

## Verify a discrete-state type (e.g. MultiStateValueDiscreteType, CU 2831)

1. Build the Variable via `create_multi_state_value_discrete_variable`
   with a non-contiguous `EnumValues` table (e.g. 1, 4, 8).
2. Read the Value, `EnumValues`, and `ValueAsText` — confirm `ValueAsText`
   matches the current Value's display name.
3. Call `update_multi_state_value_discrete` with a value that IS in the
   table — confirm `ValueAsText` updates.
4. Call it again with a value NOT in the table — confirm `ValueAsText`
   no longer shows the previous (stale) text.

## Verify an array-shaped type (e.g. ImageItemType, CU 3325)

1. Build the Variable via `create_image_item_variable` with a 2-D
   dimension pair (columns, rows), the shared `ArrayItemBaseProperties`,
   and both `XAxisDefinition`/`YAxisDefinition`.
2. Read the Value and confirm it's the expected 2-D array.
3. Read the `ArrayDimensions` attribute and confirm `[columns, rows]`.
4. Read both axis-definition Properties and confirm they decode back to
   the `AxisInformation` values supplied.

## Full verification

```bash
cargo test -p async-opcua --test integration_tests -- integration::data_access::
cargo test -p async-opcua-server --lib --all-features
cargo clippy --all-targets --all-features
cargo fmt --all -- --check
tools/ci-playbook.sh --ci
```
