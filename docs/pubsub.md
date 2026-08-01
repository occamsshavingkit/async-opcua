# OPC UA PubSub Configuration & Usage

The `async-opcua-pubsub` crate implements the OPC UA PubSub (Part 14) specifications, supporting both brokered and brokerless communication protocols.

## 1. Supported Transport Protocols

1. **UDP Multicast (UADP)**: Publisher and subscriber support for brokerless,
   low-latency factory-floor communication.
2. **MQTT (UADP/JSON)**: Publisher and subscriber support for brokered cloud and
   IoT integration.
3. **AMQP (JSON, publisher only)**: Enterprise messaging integration using brokers
   such as RabbitMQ.
4. **WebSockets (JSON, publisher only)**: Web-based telemetry streaming.

## 2. Configuration Structures

The main configuration is defined via `PubSubConnectionConfig`:

```rust
pub struct PubSubConnectionConfig {
    pub connection_id: String,
    pub name: String,
    pub address: String, // e.g. "udp://239.0.0.1:4840" or "mqtt://localhost:1883"
    pub writer_groups: Vec<WriterGroupConfig>,
    pub reader_groups: Vec<ReaderGroupConfig>,
}

pub struct WriterGroupConfig {
    pub writer_group_id: u16,
    pub publishing_interval: u64, // millisecond interval
    pub encoding: MessageEncoding, // MessageEncoding::Uadp or MessageEncoding::Json
    pub dataset_writers: Vec<DataSetWriterConfig>,
}

pub struct DataSetWriterConfig {
    pub dataset_writer_id: u16,
    pub dataset_name: String,
    pub published_dataset: PublishedDataSetConfig,
}

pub struct PublishedDataSetConfig {
    pub published_variables: Vec<NodeId>,
}

pub struct ReaderGroupConfig {
    pub reader_group_id: u16,
    pub security_mode: Option<MessageSecurityMode>,
    pub security_policy_uri: Option<String>,
    pub security_group_id: Option<String>,
    pub dataset_readers: Vec<DataSetReaderConfig>,
}

pub struct DataSetReaderConfig {
    pub dataset_reader_id: u16,
    pub dataset_writer_id: u16,
    pub publisher_id: Option<PublisherId>,
    pub writer_group_id: Option<u16>,
    pub network_message_number: Option<u16>,
    pub target_variables: Vec<FieldTargetConfig>,
}
```

## 3. Running the PubSub Bridge

To bridge data from an OPC UA server's AddressSpace to a PubSub broker/multicast endpoint:

```rust
use async_opcua_pubsub::{PubSubConnectionConfig, WriterGroupConfig, DataSetWriterConfig, PublishedDataSetConfig, MessageEncoding};
use opcua_types::NodeId;
use std::sync::Arc;

// 1. Define configuration
let config = PubSubConnectionConfig {
    connection_id: "conn-1".to_string(),
    name: "FactorySensors".to_string(),
    address: "udp://239.0.0.1:4840".to_string(),
    writer_groups: vec![WriterGroupConfig {
        writer_group_id: 101,
        publishing_interval: 1000,
        encoding: MessageEncoding::Uadp,
        dataset_writers: vec![DataSetWriterConfig {
            dataset_writer_id: 1,
            dataset_name: "TemperatureDataSet".to_string(),
            published_dataset: PublishedDataSetConfig {
                published_variables: vec![NodeId::new(2, "TemperatureSensor")],
            },
        }],
    }],
    reader_groups: Vec::new(),
};

// 2. Start the PubSub bridge with an OPC UA Server instance
let server = Arc::new(server_instance);
let _bridge = async_opcua_pubsub::start_pubsub_bridge(config, server).await.unwrap();
```

## 4. Running a UADP Subscriber

The subscriber runtime applies matching UADP key-frame DataSetMessages to configured target Variables. Matching uses the configured PublisherId, WriterGroupId, NetworkMessageNumber, and DataSetWriterId filters; omitted PublisherId/WriterGroupId/NetworkMessageNumber and a DataSetWriterId of `0` act as wildcards.

```rust
use std::sync::Arc;

use opcua_core::sync::RwLock;
use opcua_pubsub::{
    DataSetReaderConfig, DataSetReaderKey, FieldTargetConfig, PubSubConnectionConfig,
    ReaderGroupConfig, SubscriberRuntime,
};
use opcua_server::address_space::AddressSpace;
use opcua_types::{ContextOwned, NodeId};

let address_space = Arc::new(RwLock::new(AddressSpace::new()));
let target = NodeId::new(2, "TemperatureTarget");

let config = PubSubConnectionConfig {
    connection_id: "subscriber-1".to_string(),
    name: "LineSubscriber".to_string(),
    address: "udp://239.0.0.1:4840".to_string(),
    writer_groups: Vec::new(),
    reader_groups: vec![ReaderGroupConfig {
        reader_group_id: 1,
        dataset_readers: vec![DataSetReaderConfig {
            dataset_reader_id: 1,
            dataset_writer_id: 10,
            target_variables: vec![FieldTargetConfig::value(0, target)],
            ..DataSetReaderConfig::default()
        }],
        ..ReaderGroupConfig::default()
    }],
};

let mut runtime = SubscriberRuntime::with_connections(address_space, vec![config])?;
let ctx_owned = ContextOwned::default();
let ctx = ctx_owned.context();
runtime.process_datagram_for_connection("subscriber-1", &udp_payload, &ctx)?;
let status = runtime.reader_status_by_key(&DataSetReaderKey::new("subscriber-1", 1));
```

## 5. Engine-Level Subscriber Startup

`PubSubEngine::start_subscribers` starts all configured subscriber receive loops
from the engine. It is async and returns `Result<(), StatusCode>`, so callers
must `.await` it:

```rust
let mut engine = PubSubEngine::new(address_space);
engine.add_connection(config);
engine.start_subscribers().await?;
```

The method validates every subscriber configuration against supported transports
(OPC 10000-14 §6.4) and binds all UDP sockets before committing any subscriber
task or running state. If validation or UDP preparation fails, the engine returns
an error and no subscriber is left partially running.

Unsupported subscriber transports (AMQP, WebSocket, TSN, `mqtts://`) fail closed
with `StatusCode::BadNotSupported` during the same pre-start validation.

## Limitations and experimental features

- **Subscriber scope**: the reader side supports brokerless UDP UADP and brokered
  MQTT UADP/JSON key-frame DataSetMessages with Variant/DataValue-compatible
  fields and Value-attribute target writes. The public `start_mqtt_subscriber*`
  helpers return `MqttBrokerAddressError` for invalid broker addresses before task
  spawn. Engine dispatch returns `BadNotSupported` for `mqtts://` TLS, AMQP,
  WebSocket, and TSN subscribers. MQTT TLS subscriber support remains unimplemented.
  RawData payloads, delta frames, event DataSetMessages, non-Value target
  attributes, index ranges, and the crate's legacy publisher fragmentation header
  are rejected with `BadNotSupported`.
- **Security boundary and ingress**: `SubscriberRuntime::process_datagram` and
  `process_datagram_for_connection` accept raw bytes only when every configured
  reader is effectively unsecured after applying its DataSetReader override.
  If any reader requires signing or signing and encryption, the runtime returns
  `BadSecurityChecksFailed` without decoding the payload.
  Secured bytes must enter through `PubSubEngine::process_subscriber_datagram`,
  which owns the security verification, decryption, and anti-replay pipeline.
  The legacy `decode_and_apply` helper enforces the same raw-byte restriction.
  `apply_network_message`, `process_network_message`, and
  `process_network_message_for_connection` accept already decoded, verified,
  decrypted, and replay-checked UADP NetworkMessages and are the trusted boundary
  for the variable-apply path.
- **Multi-connection direct ingress**: use `process_datagram_for_connection` or
  `process_network_message_for_connection` to select the owning connection.
  Unscoped direct ingress returns `BadInvalidArgument` when more than one
  connection is configured; unknown scoped connection ids return `BadNotFound`.
  Status is keyed by `DataSetReaderKey` through `reader_status_by_key`;
  numeric-only `reader_status(u16)` returns no result when the id is ambiguous
  across connections.
- **MessageReceiveTimeout**: timeout transitions are evaluated only when
  `SubscriberRuntime::check_timeouts_at` is explicitly called. Current
  transport receive loops do not independently schedule timeout checks.
- **DataSetReader status**: `DataSetReaderStatus` snapshots are available
  in-memory through `reader_status_by_key`. The information-model reflection
  exposes custom `ReaderState`, `AcceptedCount`, `FilteredCount`, and
  `DroppedCount` properties, but mandatory Part 14 Status Object and State
  nodes are not yet materialized or live-synchronized with the runtime.
- **Message security**: secured UADP NetworkMessages use the OPC UA Part 14
  SecurityHeader, SecurityTokenId, MessageNonce, AES-CTR payload encryption,
  HMAC-SHA256 signing, and subscriber anti-replay checks before target Variables
  are updated. Secure subscriber processing requires a registered
  `SecurityGroup` and matching ReaderGroup/DataSetReader security settings.
- **TSN is a simulated stub**: the `tsn://` transport is gated behind the
  off-by-default `tsn` feature of `async-opcua-pubsub`. Its AF_XDP socket is
  a simulated loopback and scheduling shells out to `tc taprio`; it has not
  been validated on real TSN hardware (spec 004 T046). The 2026-06-28 T046
  closeout found no PHC device, no NIC hardware timestamp modes, no local
  PTP/cyclictest tooling, and no effective raw-socket capability in this
  workspace, so no sub-millisecond jitter claim is made.
