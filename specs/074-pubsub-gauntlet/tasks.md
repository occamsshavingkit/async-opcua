# Tasks: PubSub Gauntlet Compliance

**Feature**: 074-pubsub-gauntlet
**Branch**: `074-pubsub-gauntlet`
**Spec**: [spec.md](./spec.md) | **Plan**: [plan.md](./plan.md)

## Ledger Status

This ledger was reconciled on 2026-07-13 after issue #292 reported that all six PubSub
Gauntlet failures still reproduced at `08181297c` (193/219 overall). The implementation
tasks below track source/test work that exists in this repository; they do not imply the
Gauntlet PubSub criteria pass unless the task explicitly says so. A follow-up remediation
pass on branch `092-pubsub-ledger` fixed the local demo-server/Gauntlet data-flow gaps
identified during root-cause analysis; fresh external Gauntlet validation is still needed.

Evidence used for reconciliation:
- Demo server starts PubSub subscribers: `samples/demo-server/src/main.rs`.
- UDP and MQTT subscriber dispatch plus bounded datagram queue: `async-opcua-pubsub/src/engine.rs`.
- MQTT subscriber transport: `async-opcua-pubsub/src/transport/mqtt.rs`.
- JSON subscriber runtime tests: `async-opcua-pubsub/tests/subscriber_json_tests.rs`.
- MQTT handoff tests: `async-opcua-pubsub/tests/subscriber_mqtt_tests.rs`.
- Queue-full unit coverage: `async-opcua-pubsub/tests/pubsub_tests.rs`.
- Writable PubSub config updates now notify a runtime-owner task: `async-opcua-pubsub/src/config_methods.rs`, `samples/demo-server/src/main.rs`.
- `DataSetReaderDataType` conversion now preserves JSON mapping and `TargetVariablesDataType`: `async-opcua-pubsub/src/config_methods.rs`.
- Current failing Gauntlet inventory: GitHub issue #292.

> **Spec Reading Protocol**: Every task below that changes OPC UA behavior includes a directive to **read the referenced OPC-10000-14 section in full** before writing code. Use `opc-ua-reference_search_text` with the cited `docNumber` and section number to fetch the normative text, or open the linked reference URL directly. Do not implement based on the task summary alone.

## Phase 1: Setup

- [x] T001 Verify build baseline — `cargo build -p async-opcua-pubsub` and `cargo test -p async-opcua-pubsub` pass on branch `074-pubsub-gauntlet`
- [x] T002 Read and annotate the current subscriber dispatch path:
  - `async-opcua-pubsub/src/subscriber.rs` — `process_network_message`, `apply_reader`
  - `async-opcua-pubsub/src/engine.rs` — `start_subscribers`
  - `async-opcua-pubsub/src/transport/udp.rs` — `bind_subscriber_socket`

## Phase 2: Foundational — Demo Server Wiring

- [x] T003 Wire `PubSubEngine::start_subscribers()` into the demo server startup path. Check `samples/demo-server/src/main.rs` and `samples/demo-server/src/customs.rs` for where the server is constructed. Add `engine.start_subscribers()` call after server initialization if not already present. Before implementing, read OPC-10000-14 §5.4.6.2.2 Broker-less model with OPC UA UDP ([reference](https://reference.opcfoundation.org/specs/OPC-10000-14/5.4.6.2.2.md)).
- [x] T004 Verify `cargo build -p async-opcua-demo-server --features pubsub` succeeds

## Phase 3: User Story 1 — Broker-less UDP Subscriber (Priority: P1)

**Goal**: Demo server starts subscriber UDP receive loops and processes incoming UADP NetworkMessages.

**Independent Test**: Start demo server with a ReaderGroup + DataSetReader with UDP transport. Publish a plain UADP NetworkMessage. Verify reception and variable update.

- [x] T005 [US1] In `async-opcua-pubsub/src/engine.rs`, verify `start_subscribers()` iterates all `PubSubConnectionConfig` entries and starts UDP datagram readers for each DataSetReader whose transport is `DatagramDataSetReaderTransportDataType`. If gaps exist, implement them. Before implementing, read OPC-10000-14 §6.4.1.6.1 Address ([reference](https://reference.opcfoundation.org/specs/OPC-10000-14/6.4.1.6.1.md)) and §6.4.1.6.3 DatagramQos ([reference](https://reference.opcfoundation.org/specs/OPC-10000-14/6.4.1.6.3.md)).
- [x] T006 [P] [US1] In `async-opcua-pubsub/src/subscriber.rs`, ensure the DataSetReader Status Object state transitions per OPC-10000-14 §6.2.1 PubSubState state machine: PreOperational → Operational on first key-frame DataSetMessage, Operational → Error on `MessageReceiveTimeout` expiry (§6.2.9.6). Expose the state via the PubSubStatusType model (§9.1.10.1). Before implementing, read all three referenced sections.
- [x] T007 [P] [US1] Add integration test in `async-opcua-pubsub/tests/wired_subscriber_test.rs`. Configure a DataSetReader with UADP mapping and UDP transport. Publish a UADP NetworkMessage to the reader's multicast address. Assert the subscriber decodes it and writes the field value to the target variable. Also verify the Status Object transitions to Operational after first message.

## Phase 4: User Story 2 — JSON Subscriber Message Mapping (Priority: P2)

**Goal**: Subscriber runtime dispatches JSON NetworkMessages and decodes DataSet fields per OPC-10000-14 §7.2.5.

**Independent Test**: Configure DataSetReader with `JsonDataSetReaderMessageDataType`. Send JSON NetworkMessage. Verify decoded fields match.

- [x] T008 [US2] In `async-opcua-pubsub/src/subscriber.rs`, add a message-type dispatch point. When the DataSetReader's `DataSetReaderMessageDataType` is `JsonDataSetReaderMessageDataType`, route the payload to the JSON decode path instead of UADP. Before implementing, read OPC-10000-14 §6.3.2.4.3 JsonDataSetReaderMessageDataType structure ([reference](https://reference.opcfoundation.org/specs/OPC-10000-14/6.3.2.4.3.md)) and §7.2.5 JSON message mapping overview.
- [x] T009 [P] [US2] Enhance `async-opcua-pubsub/src/codec/json.rs`. Implement `decode_network_message(bytes) -> Result<JsonNetworkMessage>` that parses the JSON envelope: `MessageId`, `MessageType`, `PublisherId`, `DataSetClassId`, `Messages` array per OPC-10000-14 §7.2.5.4 / Table 187. Implement field value decoding per the DataSetMetaData field definitions. Before implementing, read §7.2.5.4 JSON NetworkMessage structure and §7.2.5.5.2 DataSetMetaData.
- [x] T010 [P] [US2] Add integration test `json_subscriber_receives_fields` in `async-opcua-pubsub/tests/subscriber_json_tests.rs`. Configure a DataSetReader with JSON mapping and DataSetMetaData defining 3 fields (Int32, Double, String). Publish a matching JSON NetworkMessage. Assert decoded field values match expected types and values.

## Phase 5: User Story 3 — UADP Broker Subscriber (Priority: P2)

**Goal**: Subscriber connects to MQTT broker and receives UADP NetworkMessages per OPC-10000-14 §6.4.2.

**Independent Test**: Start mosquitto. Configure MQTT PubSubConnection + UADP DataSetReader. Publish UADP to topic. Verify reception.

- [x] T011 [US3] Implement `MqttSubscriber` in `async-opcua-pubsub/src/transport/mqtt.rs`. Use the existing `rumqttc` dependency. Connect to the broker `NetworkAddressUrl` from `BrokerDataSetReaderTransportDataType`, subscribe to the configured topic filter (§6.4.1.6.4), and forward received payload bytes to the subscriber runtime via a channel. Honor `RequestedDeliveryGuarantee` for QoS (§6.4.2.6.4). Before implementing, read OPC-10000-14 §6.4.2.6 MQTT reader transport settings ([reference](https://reference.opcfoundation.org/specs/OPC-10000-14/6.4.2.6.md)).
  - Evidence: `async-opcua-pubsub/src/transport/mqtt/subscriber.rs` defines configurable subscriber startup, maps `RequestedDeliveryGuarantee` to `rumqttc::QoS`, forwards published payloads over `mpsc`, and reconnects with backoff. `async-opcua-pubsub/src/reader_transport.rs` preserves `BrokerDataSetReaderTransportDataType` settings from the OPC UA configuration model.
  - Local verification: `cargo test -p async-opcua-pubsub --test mqtt_reader_transport_config` passed all four topic and delivery-guarantee regression tests.
- [x] T012 [P] [US3] In `async-opcua-pubsub/src/engine.rs`, wire MQTT subscriber transport dispatch. When `start_subscribers()` encounters a PubSubConnection with `BrokerDataSetReaderTransportDataType`, spawn an MQTT subscriber task for each DataSetReader in its ReaderGroups. Before implementing, read §6.4.2 MQTT transport protocol mapping overview.
  - Evidence: `PubSubEngine::start_subscribers` dispatches `TransportKind::Mqtt` to `spawn_mqtt_subscribers`, which starts one cancellation-aware MQTT task and one runtime-forwarder task per DataSetReader using its configured `QueueName` topic filter and delivery guarantee.
- [x] T013 [P] [US3] Add integration test `mqtt_subscriber_receives_uadp` in `async-opcua-pubsub/tests/subscriber_mqtt_tests.rs`. Start a local mosquitto instance. Configure MQTT PubSubConnection + UADP DataSetReader. Publish a UADP NetworkMessage to the topic. Assert the subscriber receives and processes it.
  - Evidence: `async-opcua-pubsub/tests/subscriber_mqtt_tests.rs` now includes `mqtt_subscriber_receives_uadp_from_live_mosquitto`, which starts a local mosquitto broker when available, starts `PubSubEngine::start_subscribers`, publishes a UADP payload to `opcua/telemetry/7`, and asserts the target variable is updated through the real broker path.
  - Local verification: `cargo test -p async-opcua-pubsub --test subscriber_mqtt_tests -- --nocapture` passed, including `start_mqtt_subscriber_stops_when_cancelled`. The live broker case compiled and reported `skipping live MQTT broker test: mosquitto not found on PATH` in this environment because mosquitto is not installed.

## Phase 6: User Story 4 — PubSub Status Codes and Limits (Priority: P3)

**Goal**: Server returns `BadTooManyPublishRequests` when the subscriber datagram queue is full, per OPC-10000-14 §9.1.10.1.

**Independent Test**: Flood subscriber with datagrams beyond queue capacity. Verify limit status code.

- [x] T014 [US4] In `async-opcua-pubsub/src/engine.rs`, replace the unbounded datagram send path with a bounded `tokio::sync::mpsc` channel. Use `try_send` — on failure (channel full), return `StatusCode::BadTooManyPublishRequests`. Add a configuration parameter for queue capacity with a sensible default (e.g., 1024). Before implementing, read OPC-10000-14 §9.1.10.1 PubSubStatusType ([reference](https://reference.opcfoundation.org/specs/OPC-10000-14/9.1.10.1.md)) and §9.1.8.2 DataSetReaderType ([reference](https://reference.opcfoundation.org/specs/OPC-10000-14/9.1.8.2.md)).
  - Evidence: `async-opcua-pubsub/src/engine.rs` defines a bounded `DatagramQueue`, maps `TrySendError::Full` to `StatusCode::BadTooManyPublishRequests`, and uses that queue in UDP and MQTT subscriber paths.
  - Caveat: issue #292 still reports Gauntlet P14-S08-001 gets `Good`, so the internal queue-full behavior is not yet wired to the specific service/status path the Gauntlet checks.
- [x] T015 [P] [US4] Add unit test `datagram_queue_full_returns_limit` in `async-opcua-pubsub/tests/pubsub_tests.rs`. Create queue capacity 1, send two datagrams. Assert the second `try_send` returns `BadTooManyPublishRequests`.
  - Evidence: `async-opcua-pubsub/tests/pubsub_tests.rs` asserts queue-full returns `StatusCode::BadTooManyPublishRequests`.

## Phase 7: Polish & Verification

- [x] T016 Run `cargo fmt --check` and `cargo clippy --workspace` — fix any issues
  - Evidence: `cargo fmt --check` passed on branch `092-pubsub-ledger` after formatting; `tools/ci-playbook.sh --ci` completed successfully and covered the full local quality gate.
- [x] T017 Run `cargo test -p async-opcua-pubsub` — verify all existing subscriber and codec tests pass
  - Evidence: `cargo test -p async-opcua-pubsub` passed on branch `092-pubsub-ledger`.
- [x] T018 Run `cargo build -p async-opcua-demo-server --features pubsub` — verify demo server compiles with PubSub
  - Evidence: `cargo build -p async-opcua-demo-server` and `cargo build --release -p async-opcua-demo-server` passed with PubSub enabled by default.
- [x] T019 Run `tools/ci-playbook.sh --ci` — verify full local CI gate passes, including existing interop tests
  - Evidence: `tools/ci-playbook.sh --ci` completed with `CI gate complete.` on branch `092-pubsub-ledger`.
- [x] T020 Run the live MQTT broker test on a host with `mosquitto` installed — `cargo test -p async-opcua-pubsub --test subscriber_mqtt_tests mqtt_subscriber_receives_uadp_from_live_mosquitto -- --nocapture`
  - Evidence: command passed locally after `mosquitto` was installed; output reported `test mqtt_subscriber_receives_uadp_from_live_mosquitto ... ok` with 1 passed, 0 failed, and no skip message.

## Current Unresolved Conformance Gap

Issue #292 reported that all six PubSub Gauntlet tests failed at `08181297c`:

- P14-S05.6-001: JSON message mapping returns empty Publish results.
- P14-S05.7-001: UADP message mapping returns empty Publish results.
- P14-S06.2-001: broker-less PubSub returns empty Publish results.
- P14-S06.3-001: broker connection returns empty Publish results.
- P14-S06.3-002: broker QoS returns empty Publish results.
- P14-S08-001: expected `Bad_TooManyPublishRequests`, got `Good`.

The local remediation now addresses the identified demo-server/Gauntlet data-flow blockers:
default demo-server builds include PubSub wiring, writable PubSub config methods are registered,
config changes reach a running engine through snapshot updates, and DataSetReader snapshots carry
JSON mapping plus target variables. External Gauntlet still needs to be rerun, and P14-S08-001 may
still need a follow-up if the Gauntlet expects `Bad_TooManyPublishRequests` on a service path other
than the subscriber datagram queue.

## Dependencies

```
T001 ──> T002 ──> T003 ──> T004     (setup + foundational)
                            │
              ┌─────────────┤
              ▼             │
    T005 ──> T006,T007      │      (US1: UDP subscriber)
              │             │
              └──────┬──────┘
                     │
          ┌──────────┼──────────┐
          ▼          ▼          ▼
    T008 ──> T009  T011 ──> T012  T014
    (US2: JSON)    (US3: MQTT)   (US4: limits)
          │          │            │
          ▼          ▼            ▼
        T010        T013         T015
          │          │            │
          └──────────┴────────────┘
                     │
                     ▼
              T016..T019      (polish)
```

## Parallel Execution

- US2 (T008-T010), US3 (T011-T013), and US4 (T014-T015) are fully independent after US1 completes.
- Within US2: T009 is parallel with T008.
- Within US3: T012 is parallel with T011.
- T006 and T007 within US1 are parallel.

## Suggested MVP Scope

Phases 1-3 (T001-T007) — wires existing subscriber to demo server, 7 tasks.
