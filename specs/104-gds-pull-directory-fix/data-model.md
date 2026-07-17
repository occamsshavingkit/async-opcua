# Data Model: GDS Pull Directory Singleton Correction (Run 1 rework)

No new entities. This feature changes how one existing entity's fields are populated, not its shape.

## DirectoryInstanceNodeIds (unchanged public shape, corrected values)

`async-opcua-server/src/gds/directory_instance.rs`

| Field | Before (feature 103) | After (this feature) |
|---|---|---|
| `directory_object_id` | `NodeId::new(gds_ns, "Directory")` (hand-built) | `NodeId::new(gds_ns, 141u32)` (real object) |
| `start_signing_request_id` | `NodeId::new(gds_ns, "Directory.StartSigningRequest")` | `NodeId::new(gds_ns, 157u32)` |
| `start_new_key_pair_request_id` | `NodeId::new(gds_ns, "Directory.StartNewKeyPairRequest")` | `NodeId::new(gds_ns, 154u32)` |
| `finish_request_id` | `NodeId::new(gds_ns, "Directory.FinishRequest")` | `NodeId::new(gds_ns, 163u32)` |
| `get_certificate_groups_id` | `NodeId::new(gds_ns, "Directory.GetCertificateGroups")` | `NodeId::new(gds_ns, 508u32)` |
| `get_trust_list_id` | `NodeId::new(gds_ns, "Directory.GetTrustList")` | `NodeId::new(gds_ns, 204u32)` |
| `get_certificate_status_id` | `NodeId::new(gds_ns, "Directory.GetCertificateStatus")` | `NodeId::new(gds_ns, 225u32)` |
| `default_application_group_id` | hand-built `"Directory.CertificateGroups.DefaultApplicationGroup"` | `NodeId::new(gds_ns, 615u32)` |
| `default_application_group_trust_list_id` | hand-built `"...TrustList"` | `NodeId::new(gds_ns, 616u32)` |

All fields keep their existing types (`NodeId`) and existing consumers (`pull_methods/mod.rs`'s
`register_pull_method_callbacks`, the six handler functions, and the two test suites). No signature
changes propagate outward from `directory_instance.rs`.

## Removed internal helpers (no longer needed)

- `insert_method(address_space, gds_ns, parent_id, name, input_args, output_args) -> NodeId` — was
  used to hand-build each method node; the real methods already exist with their own real
  `InputArguments`/`OutputArguments` from the XML import.
- `argument(name, data_type) -> Argument` / `array_argument(name, data_type) -> Argument` — were used
  only to author the hand-built methods' argument lists; no longer needed for the same reason.
- `ObjectBuilder`/`MethodBuilder` usage throughout the file — replaced by `AddressSpace::find`
  existence checks against the real, verified NodeIds.
