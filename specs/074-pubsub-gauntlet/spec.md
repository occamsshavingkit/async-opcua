# Feature Specification: PubSub Gauntlet Compliance

**Feature Branch**: `074-pubsub-gauntlet`
**Created**: 2026-07-12
**Status**: Draft
**Input**: User description: "Implement PubSub fully per OPC-10000-14 to pass the 6 remaining Gauntlet PubSub compliance tests."

## Context

The OPC UA Gauntlet compliance test tool reported 6 PubSub failures against `demo-server`.
The OPC UA specification OPC-10000-14 defines the normative requirements for
Publish-Subscribe. This feature closes the gaps so the server passes all 6 tests.

Prior specs 026 (message security) and 037 (subscriber runtime) have already implemented
the UADP codec, secured UADP subscriber, DataSetReader state machine, and UDP transport.
What remains is wiring these implementations to the demo server, adding JSON subscriber
support, and implementing broker subscriber transports.

### Specification Grounding

| Gauntlet Test | OPC-10000-14 Section | What |
|---|---|---|
| P14-S05.6-001 | §6.3.2.4.3 JsonDataSetReaderMessageDataType | JSON subscriber message mapping |
| P14-S05.7-001 | §6.3.1.4.10 UadpDataSetReaderMessageDataType | UADP subscriber message mapping |
| P14-S06.2-001 | §5.4.6.2.2 Broker-less model with OPC UA UDP | Broker-less UDP subscriber |
| P14-S06.3-001 | §6.4.2.6 MQTT reader transport | Broker subscriber (MQTT/AMQP) |
| P14-S06.3-002 | §6.4.2.6 MQTT reader transport | Broker subscriber (MQTT/AMQP) |
| P14-S08-001 | §9.1.10.1 PubSubStatusType / §9.1.8.2 DataSetReaderType | PubSub status codes and limits |

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Broker-less UDP Subscriber (Priority: P1)

As an OPC UA compliance test tool, when I configure a PubSub connection with datagram
transport and a DataSetReader with UADP message mapping, the server receives NetworkMessages
on the configured UDP multicast address and delivers DataSet values to target variables.

**Why this priority**: The subscriber runtime code is already implemented and tested
(spec 037). The remaining work is wiring the demo server to start subscriber loops
automatically on startup and expose the PubSub information model. This is the
highest-impact, lowest-risk fix — unblocks 1 Gauntlet test directly and provides
the infrastructure for the remaining tests.

**Independent Test**: Configure a PubSubConnection with `DatagramDataSetReaderTransport`
and a DataSetReader with UADP message mapping via the server's configuration methods.
Publish a UADP NetworkMessage to the configured multicast address. Verify the server
receives it and updates the target variable.

**Acceptance Scenarios**:

1. **Demo server starts subscriber loops**: **Given** a demo server is started with a
   PubSub configuration containing ReaderGroups and DataSetReaders, **When** the server
   completes startup, **Then** subscriber UDP receive loops are active and the
   DataSetReader Status Object shows `State = Operational` after receiving the first
   message (OPC-10000-14 §6.2.1, §9.1.10.1).

2. **Subscriber receives plain UADP NetworkMessage**: **Given** a running demo server
   with a configured DataSetReader for PublisherId X, **When** a UADP NetworkMessage
   with PublisherId X is sent to the configured multicast address, **Then** the server
   decodes the DataSetMessage and applies field values to the configured target variables
   (OPC-10000-14 §5.4.6.2.2, §7.2.4).

### User Story 2 — JSON Subscriber Message Mapping (Priority: P2)

As an OPC UA compliance test tool, when I configure a DataSetReader with JSON message
mapping, the server receives JSON NetworkMessages and delivers the decoded DataSet to
target variables.

**Why this priority**: The JSON codec exists in `async-opcua-pubsub/src/codec/json.rs`
(66 lines) but the subscriber runtime does not dispatch JSON messages. This is a
self-contained addition to the subscriber pipeline.

**Independent Test**: Configure a DataSetReader with `JsonDataSetReaderMessage` mapping.
Send a JSON NetworkMessage matching the reader's filter criteria. Verify the data is
decoded and applied to target variables.

**Acceptance Scenarios**:

1. **Subscriber dispatches JSON NetworkMessages**: **Given** a DataSetReader with
   `JsonDataSetReaderMessageDataType` configuration, **When** a JSON NetworkMessage
   arrives on the reader's transport, **Then** the subscriber runtime identifies
   the message type as JSON and routes it to the JSON decode path
   (OPC-10000-14 §6.3.2.4.3).

2. **JSON DataSet fields are decoded**: **Given** a JSON NetworkMessage containing
   a DataSetMessage with typed fields (Int32, Double, String), **When** the subscriber
   processes it, **Then** each field is decoded to the correct OPC UA Variant type
   per the DataSetMetaData field definitions (OPC-10000-14 §7.2.5.4).

### User Story 3 — UADP Broker Subscriber (Priority: P2)

As an OPC UA compliance test tool, when I configure a PubSubConnection with MQTT
broker transport and a DataSetReader with UADP message mapping, the server subscribes
to the MQTT topic and processes received UADP NetworkMessages.

**Why this priority**: The MQTT publisher exists (`transport/mqtt.rs`) but the subscriber
side rejects broker transports. Adding MQTT subscriber support completes the
broker-based UADP transport profile. This unblocks P14-S06.3-001 and P14-S05.7-001.

**Independent Test**: Start an MQTT broker locally. Configure the demo server with a
PubSubConnection pointing to the MQTT broker and a DataSetReader with UADP message
mapping. Publish a UADP NetworkMessage to the configured topic. Verify the server
receives and processes it.

**Acceptance Scenarios**:

1. **Subscriber connects to MQTT broker**: **Given** a PubSubConnection with MQTT
   broker transport and a ReaderGroup, **When** the server starts the subscriber,
   **Then** the subscriber connects to the MQTT broker address and subscribes to the
   configured topic filter (OPC-10000-14 §6.4.2.6).

2. **Subscriber receives UADP over MQTT**: **Given** an MQTT-connected subscriber
   with a configured DataSetReader, **When** a UADP NetworkMessage is published to
   the topic, **Then** the subscriber decodes and applies the DataSet per the
   reader's field mapping (OPC-10000-14 §6.4.2.6.1, §7.2.4).

### User Story 4 — PubSub Status Codes and Limits (Priority: P3)

As an OPC UA compliance test tool, when I send a request that exceeds the server's
PubSub processing capacity, the server returns `BadTooManyPublishRequests` or an
equivalent limit-enforcing status code.

**Why this priority**: The Gauntlet expects the server to enforce operational limits
on PubSub subscriber processing. This is a status-code addition to the existing
subscriber runtime.

**Independent Test**: Configure the server with a subscriber and flood it with
NetworkMessages at a rate that exceeds processing capacity; verify the server returns
a limit-enforcing status code or transitions the DataSetReader to Error state.

**Acceptance Scenarios**:

1. **Subscriber enforces processing limits**: **Given** a subscriber receiving
   NetworkMessages at a rate exceeding its configured capacity, **When** the
   subscriber's internal queue reaches its limit, **Then** the server returns
   `BadTooManyPublishRequests` or transitions the DataSetReader Status to `Error`
   (OPC-10000-14 §9.1.10.1).

### Edge Cases

- **Empty NetworkMessage**: A datagram containing zero DataSetMessages must not crash
  or panic; the subscriber silently discards it.
- **Malformed JSON**: A JSON payload that fails to parse as a NetworkMessage must be
  dropped and logged; the subscriber must not enter Error state from a single malformed
  message.
- **Missing DataSetMetaData**: If a DataSetReader is configured without DataSetMetaData,
  it must not crash; it stays in PreOperational state until metadata is received or
  configured.
- **Rapid reconnect**: If the MQTT broker connection drops, the subscriber must attempt
  reconnection with backoff rather than entering a tight reconnect loop.
- **Security downgrade**: A DataSetReader configured with security must reject unsecured
  NetworkMessages; a plaintext message on a secured reader must not be processed.

## Requirements *(mandatory)*

### Functional Requirements

#### Demo Server Wiring

- **FR-001**: The demo server MUST start PubSub subscriber UDP receive loops on startup
  when the `pubsub` feature is enabled and the PubSub configuration contains active
  DataSetReaders.
- **FR-002**: The demo server MUST expose PubSub configuration methods (AddConnection,
  AddReaderGroup, AddDataSetReader, etc.) through the standard Part 14 information model
  under `Server.PublishSubscribe`.
- **FR-003**: The PubSub engine MUST coordinate publisher and subscriber lifecycle,
  starting subscriber loops after the server is fully initialized.

#### JSON Subscriber

- **FR-004**: The subscriber runtime MUST dispatch received NetworkMessages to the JSON
  decode path when the DataSetReader's `DataSetReaderMessageDataType` is
  `JsonDataSetReaderMessageDataType` (OPC-10000-14 §6.3.2.4.3).
- **FR-005**: The JSON decode path MUST parse a JSON NetworkMessage envelope, extract
  DataSetMessage payloads, and decode field values per the DataSetMetaData field
  definitions (OPC-10000-14 §7.2.5.4).
- **FR-006**: The JSON subscriber MUST support the standard JSON NetworkMessage fields:
  MessageId, MessageType, PublisherId, DataSetClassId, Messages array, and optional
  DataSetWriterId/SequenceNumber per the JsonNetworkMessageContentMask
  (OPC-10000-14 §6.3.2.3.1).

#### Broker Subscriber

- **FR-007**: The subscriber runtime MUST support MQTT broker transport for receiving
  UADP NetworkMessages (OPC-10000-14 §6.4.2.6).
- **FR-008**: The MQTT subscriber MUST connect using the configured `NetworkAddressUrl`
  and subscribe to the topic specified by the DataSetReader's address parameters
  (OPC-10000-14 §6.4.1.6.1, §6.4.1.6.4).
- **FR-009**: The MQTT subscriber MUST support QoS level configuration per the
  `RequestedDeliveryGuarantee` parameter (OPC-10000-14 §6.4.2.6.4).

#### PubSub Status Codes

- **FR-010**: The subscriber runtime MUST enforce a bounded queue for incoming
  NetworkMessages and return `BadTooManyPublishRequests` when the queue is full
  or set the DataSetReader Status to `Error` (OPC-10000-14 §9.1.10.1, §9.1.8.2).
- **FR-011**: The subscriber MUST expose per-DataSetReader Status objects with
  `State` property indicating Disabled, PreOperational, Operational, or Error
  as defined by the PubSub state machine (OPC-10000-14 §6.2.1).

### Key Entities

- **PubSubConnection**: Groups Publisher/Subscriber transport settings for a
  communication middleware (OPC-10000-14 §9.1.5.2).
- **DataSetReader**: Subscriber-side component that filters and decodes incoming
  DataSetMessages (OPC-10000-14 §6.2.9, §9.1.8.2).
- **ReaderGroup**: Groups DataSetReaders sharing a transport connection
  (OPC-10000-14 §9.1.6.9).
- **NetworkMessage**: Wire-format message carrying one or more DataSetMessages
  (OPC-10000-14 §7.2.4, §7.2.5).
- **DataSetMetaData**: Defines field names, types, and layout for a PublishedDataSet
  (OPC-10000-14 §6.2.3.2.3).

## Success Criteria *(mandatory)*

- **SC-001**: Gauntlet test P14-S06.2-001 (broker-less PubSub) passes — the demo server
  receives and processes UADP NetworkMessages over UDP.
- **SC-002**: Gauntlet test P14-S05.6-001 (JSON message mapping) passes — the subscriber
  decodes JSON NetworkMessages and applies DataSet values.
- **SC-003**: Gauntlet test P14-S05.7-001 (UADP message mapping) passes — the subscriber
  decodes UADP NetworkMessages delivered over broker transport.
- **SC-004**: Gauntlet tests P14-S06.3-001 and P14-S06.3-002 (broker connection/QoS) pass —
  the subscriber connects to an MQTT broker and negotiates QoS.
- **SC-005**: Gauntlet test P14-S08-001 (PubSub StatusCodes) passes — the server returns
  `BadTooManyPublishRequests` or appropriate status when limits are exceeded.
- **SC-006**: No existing integration tests regress — all existing PubSub tests
  (`async-opcua-pubsub/tests/*.rs`, `async-opcua/tests/integration/pubsub.rs`)
  continue to pass.
- **SC-007**: Existing asyncua interop and .NET interop tests continue to pass.

## Assumptions

- The Gauntlet provides its own PubSub configuration via the server's configuration
  Methods (AddConnection, AddReaderGroup, AddDataSetReader). The demo server does not
  need a hardcoded default PubSub configuration.
- The MQTT broker for broker-based tests is provided by the test environment
  (e.g., mosquitto running locally). The demo server does not embed a broker.
- Security (Sign/SignAndEncrypt) for JSON subscriber is out of scope; plain JSON
  subscriber is sufficient for the Gauntlet tests.
- AMQP broker subscriber is deferred — only MQTT is required to pass the Gauntlet
  broker tests.
- Configuration method support for PubSub already exists via `config_methods.rs`;
  the remaining work is wiring and subscriber transport dispatch.
