# Contract: Part 14 Subscriber Runtime

This contract describes the current public Rust-facing behavior. Spec 037
introduced brokerless UDP/UADP subscriber processing; spec 074 later added MQTT
UADP/JSON subscribers and connection-scoped direct ingress.

## Identity And Construction

```rust
pub struct DataSetReaderKey {
    pub connection_id: String,
    pub dataset_reader_id: u16,
}

impl SubscriberRuntime {
    pub fn with_connections(
        address_space: Arc<RwLock<AddressSpace>>,
        connections: Vec<PubSubConnectionConfig>,
    ) -> Result<Self, StatusCode>;
}
```

Required behavior:

- `DataSetReaderKey(connection_id, dataset_reader_id)` is the canonical runtime
  reader identity. `reader_group_id` is not part of this key.
- Connection ids are unique across the runtime. DataSetReader ids are unique
  within one connection, across all of its ReaderGroups.
- Duplicate identities fail construction with `BadConfigurationError`.
- DataSetReader names remain unique within their ReaderGroup.

## Direct Ingress

```rust
impl SubscriberRuntime {
    pub fn process_datagram(
        &mut self,
        payload: &[u8],
        ctx: &Context<'_>,
    ) -> Result<SubscriberApplyOutcome, StatusCode>;

    pub fn process_datagram_for_connection(
        &mut self,
        connection_id: &str,
        payload: &[u8],
        ctx: &Context<'_>,
    ) -> Result<SubscriberApplyOutcome, StatusCode>;

    pub fn process_network_message(
        &mut self,
        message: &UadpNetworkMessage,
    ) -> Result<SubscriberApplyOutcome, StatusCode>;

    pub fn process_network_message_for_connection(
        &mut self,
        connection_id: &str,
        message: &UadpNetworkMessage,
    ) -> Result<SubscriberApplyOutcome, StatusCode>;
}
```

Required behavior:

- Connection-scoped methods select only readers belonging to the named
  connection and return `BadNotFound` for an unknown connection id.
- Unscoped methods are compatibility APIs for runtimes containing at most one
  connection. They return `BadInvalidArgument` for multi-connection runtimes.
- `process_datagram` and `process_datagram_for_connection` accept raw bytes
  only when every configured reader is effectively unsecured after applying
  its DataSetReader override. If any reader requires signing or signing and
  encryption, these methods fail closed with `BadSecurityChecksFailed`.
  Secured byte ingress must use
  `PubSubEngine::process_subscriber_datagram`, which owns the security
  verification, decryption, and anti-replay pipeline.
- `process_network_message` and `process_network_message_for_connection`
  accept already decoded, verified, decrypted, and replay-checked UADP
  NetworkMessages. They are the trusted boundary for the variable-apply path.
- UADP and JSON key-frame messages are dispatched only to readers configured for
  the matching encoding.
- Security verification, decryption, and replay checks complete before payload
  application or target Variable mutation.
- Accepted target updates are all-or-nothing per DataSetReader.
- Malformed or unsupported input never panics and cannot mutate target Variables.

## Status

```rust
impl SubscriberRuntime {
    pub fn reader_status_by_key(
        &self,
        key: &DataSetReaderKey,
    ) -> Option<DataSetReaderStatus>;

    pub fn reader_status(&self, reader_id: u16) -> Option<DataSetReaderStatus>;
}
```

Required behavior:

- `reader_status_by_key` is the authoritative lookup.
- Numeric-only `reader_status` is retained for compatibility and returns a
  snapshot only when the numeric id is unambiguous across all connections.
- First accepted data moves a reader from PreOperational to Operational.
  (OPC 10000-14 Section 6.2.1)
- MessageReceiveTimeout moves an Operational reader to Error; the next valid new
  message returns it to Operational. (OPC 10000-14 Section 6.2.9.6)
  Timeout evaluation is explicit: `SubscriberRuntime::check_timeouts_at` must
  be called; transport receive loops do not independently schedule timeout
  checks.
- `DataSetReaderStatus` snapshots are in-memory counters. The
  information-model reflection exposes custom `ReaderState`, `AcceptedCount`,
  `FilteredCount`, and `DroppedCount` properties, but mandatory Part 14 Status
  Object and State nodes are not yet materialized or live-synchronized.
- Security failures increment diagnostics without target mutation.

## Engine Integration

```rust
impl PubSubEngine {
    pub fn process_subscriber_datagram(
        &mut self,
        connection_id: &str,
        payload: &[u8],
        ctx: &Context<'_>,
    ) -> Result<SubscriberApplyOutcome, StatusCode>;

    pub fn subscriber_status_by_key(
        &self,
        key: &DataSetReaderKey,
    ) -> Option<DataSetReaderStatus>;

    pub fn subscriber_status(&self, reader_id: u16) -> Option<DataSetReaderStatus>;

    pub async fn start_subscribers(&mut self) -> Result<(), StatusCode>;
    pub async fn stop_subscribers(&mut self);
}
```

Required behavior:

- `start_subscribers` validates every configured reader before committing any
  receive task or running state.
- UDP sockets are prepared before tasks are committed; MQTT starts one broker
  subscriber task per configured DataSetReader.
- A running engine whose subscriber configuration changed rejects direct
  processing with `BadInvalidState` until the runtime is restarted.
- `stop_subscribers` cancels receive loops and awaits task shutdown.

## Current Capability Boundary

Supported subscriber paths:

- Brokerless UDP UADP key-frame DataSetMessages.
- Brokered MQTT UADP and JSON key-frame DataSetMessages.
- ReaderGroup/DataSetReader secured UADP with fail-closed verification.
- Value-attribute target writes with empty index ranges.

Unsupported subscriber paths return `BadNotSupported` during validation or
processing:

- AMQP and WebSocket subscribers.
- MQTT TLS through `mqtts://`.
- TSN hardware scheduling.
- RawData, delta frame, and event DataSetMessages.
- Non-Value target attributes and non-empty index ranges.
- Legacy custom UDP fragmentation headers not defined by OPC 10000-14.
- Incomplete Part 14 information-model method surfaces.
