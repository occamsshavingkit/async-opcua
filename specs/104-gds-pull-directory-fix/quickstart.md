# Quickstart: GDS Pull Directory Singleton Correction (Run 1 rework)

Manual verification steps for this fix, assuming `schemas/companion/GDS/Opc.Ua.Gds.NodeSet2.xml` is
present locally (per `schemas/companion/README.md`).

## 1. Confirm exactly one Directory object after import

```sh
cargo test -p async-opcua-server --no-default-features --features companion-gds,gds \
  gds::directory_instance:: -- --nocapture
```

Expect: the existing `instantiates_a_real_directory_object_when_companion_xml_is_present` test now
resolves `directory_object_id` as `NodeId::new(gds_ns, 141)` (not a string identifier), and every
other field resolves to the real integer NodeIds documented in `data-model.md`.

## 2. Confirm the Pull-model unit suite still passes unchanged in outcome

```sh
cargo test -p async-opcua-server --no-default-features --features companion-gds,gds --lib gds::
```

Expect: all pre-existing tests pass (same count as before this fix) — externally observable
certificate-issuance/discovery behavior is unchanged, only the underlying NodeIds differ.

## 3. Confirm the end-to-end Call dispatch still reaches the real object

```sh
cargo test -p async-opcua-server --features companion-gds,method-call,generated-address-space \
  --test gds_pull_companion_integration -- --nocapture
```

Expect: `start_new_key_pair_request_call_reaches_the_pull_method_callback` passes, now dispatching
against `(NodeId::new(gds_ns, 141), NodeId::new(gds_ns, 154))` instead of the previous string-based
pair.

## 4. Confirm zero regression to GDS Push (features 101/102) and to `companion-gds`-disabled builds

```sh
cargo test -p async-opcua-server --all-features
cargo build -p async-opcua-server --no-default-features --features gds
```

Expect: full green; the disabled-feature build has zero warnings.

## 5. (Optional, manual) Browse-confirm there is exactly one "Directory" object

Run a server with `companion-gds` enabled and the Pull-model wiring invoked, connect any OPC UA
browser client, and Browse the `ObjectsFolder`. Expect exactly one "Directory" object of type
`CertificateDirectoryType` in the GDS namespace — not two.
