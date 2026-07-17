# Quickstart: Base Info Conformance Completion

```bash
cargo test -p async-opcua --test integration_tests -- integration::base_info::
```

Each test proves one user story:

- `ordered_list_*` (US1): browse `HasOrderedComponent` children, confirm
  `NumberInList` uniqueness/order and `HasInterface` → `IOrderedObjectType`.
- `selection_list_*` (US2): read `Selections`/`SelectionDescriptions`/`RestrictToList`.
- `option_set_*` (US3): read `OptionSetValues`/`BitMask`.
- `value_as_text_*` (US4): write different enum values, confirm `ValueAsText` tracks each.
- `reference_description_*` (US5): read the attached `ReferenceDescriptionDataType` Value.
- `currency_unit_*` (US6): read the `CurrencyUnit` property's 4 fields.
- `estimated_return_time_*` (US7): schedule a shutdown with a return time, read it back; confirm null otherwise.

## Full gate

```bash
tools/ci-playbook.sh --ci    # launch detached per this repo's established gotcha
```
