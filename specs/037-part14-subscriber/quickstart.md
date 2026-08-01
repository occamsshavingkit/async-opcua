# Quickstart: Part 14 Subscriber Runtime

> This quickstart preserves the original brokerless UDP/UADP scenario. Spec 074
> later added MQTT UADP/JSON subscriber support and connection-scoped direct
> ingress. See `contracts/subscriber-runtime.md` for the current API contract.

## Goal

Validate that the PubSub crate can receive broker-less UADP over UDP, dispatch the DataSetMessage to a matching DataSetReader, and update target Variables safely.

## Expected workflow after implementation

1. Configure a `PubSubConnectionConfig` with one ReaderGroup and one DataSetReader.
2. Give the DataSetReader a PublisherId filter, WriterGroupId filter, DataSetWriterId filter, MessageReceiveTimeout, and target Variables.
3. Start the subscriber runtime through `PubSubEngine::start_subscribers`.
4. Send a matching UADP key-frame NetworkMessage to the configured UDP endpoint.
5. Read the target Variables from the AddressSpace.
6. Query `subscriber_status` for accepted count and Operational state.

## Focused validation commands

```bash
cargo test -p async-opcua-pubsub subscriber_plain_uadp
cargo test -p async-opcua-pubsub subscriber_security
cargo test -p async-opcua-pubsub subscriber_status
cargo test -p async-opcua-pubsub message_security
```

## Full crate validation

```bash
cargo test -p async-opcua-pubsub
```

## Supported in this feature

- Broker-less OPC UA UDP subscriber receive path.
- UADP NetworkMessages.
- Key-frame DataSetMessages with supported value encodings.
- DataSetReader filtering by PublisherId, WriterGroupId, NetworkMessageNumber, and DataSetWriterId.
- Field-to-target Variable mapping through FieldTargetDataType-equivalent configuration.
- ReaderGroup or DataSetReader secured UADP with fail-closed verification.
- DataSetReader status and diagnostics.

## Added by spec 074

- Brokered MQTT UADP and JSON key-frame subscribers.
- Connection-scoped reader identity through `DataSetReaderKey`.
- `process_datagram_for_connection`, `process_network_message_for_connection`, and `reader_status_by_key` for multi-connection runtimes.

## Explicitly unsupported in the current runtime

- AMQP and WebSocket subscriber transports.
- MQTT TLS through `mqtts://`.
- TSN hardware scheduling.
- Delta frame, event DataSetMessage, and RawData subscriber application.
- Non-Value target attributes and non-empty index ranges.
- Full Part 14 PubSub information-model method coverage.
- Custom UDP fragment reassembly that is not defined by OPC 10000-14.

## Current Capability Notes

- `SubscriberRuntime::process_datagram` and `process_datagram_for_connection`
  accept raw bytes only when every configured reader is effectively unsecured
  after applying its DataSetReader override. Connections with an effective
  `Sign` or `SignAndEncrypt` mode return `BadSecurityChecksFailed`, and secured
  ingress must use `PubSubEngine::process_subscriber_datagram`. The
  `process_network_message*` methods are the trusted boundary for already
  decoded, verified, decrypted, and replay-checked UADP messages.
- `MessageReceiveTimeout` transitions are evaluated only when
  `SubscriberRuntime::check_timeouts_at` is explicitly called. Transport
  receive loops do not independently schedule timeout checks.
- `DataSetReaderStatus` is available as in-memory snapshots. The
  information-model reflection exposes custom `ReaderState`, `AcceptedCount`,
  `FilteredCount`, and `DroppedCount` properties, but mandatory Part 14
  Status Object and State nodes are not yet live-synchronized.
