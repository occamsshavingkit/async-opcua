# Research: PubSub Gauntlet Compliance

**Feature**: 074-pubsub-gauntlet
**Date**: 2026-07-12

## Decision 1: Subscriber message dispatch architecture

**Decision**: Extend the subscriber runtime's `process_network_message` to inspect the
message type header and dispatch to UADP or JSON decode paths based on the configured
DataSetReader's `DataSetReaderMessageDataType`.

**Rationale**:
- The subscriber runtime already has `process_network_message` in `subscriber.rs`
  that handles UADP decode. Adding a dispatch point keeps changes local.
- The DataSetReader config already carries `DataSetReaderMessageDataType` —
  we match on the concrete variant (UadpDataSetReaderMessage vs JsonDataSetReaderMessage).
- JSON codec exists at `codec/json.rs` with `decode_network_message` and `decode_data_set`.

**Alternatives considered**:
- Create separate subscriber runtimes per transport/message-type: Rejected — duplicates
  filter matching, state machine, and target variable application logic.
- Pure runtime dispatch by content-type header: Rejected — OPC-10000-14 requires
  DataSetReader config to declare the expected message mapping; sampling the wire is
  unreliable and violates the spec.

## Decision 2: Demo server subscriber wiring

**Decision**: Wire subscriber loop start in `PubSubEngine::start()` (or a new `start_subscribers`),
called from the demo server's startup sequence after the server is initialized.

**Rationale**:
- The engine already has `start_subscribers()` (line 369 in `engine.rs`) that creates
  UDP receive tasks. It just needs to be called.
- The demo server creates the PubSub engine in its constructor; calling
  `engine.start_subscribers()` after server start completes the wiring.
- No new API surface needed — the engine methods exist.

**Alternatives considered**:
- Start subscriber loops lazily on first configuration change: Rejected — adds complexity
  without benefit; the Gauntlet configures via Methods at startup.

## Decision 3: MQTT subscriber transport

**Decision**: Implement `MqttSubscriber` in `transport/mqtt.rs` following the pattern of
`UdpSubscriberEndpoint` in `transport/udp.rs`. Use the existing `rumqttc` crate dependency.

**Rationale**:
- `rumqttc` is already a dependency for the MQTT publisher.
- The subscriber needs: connect to broker, subscribe to topic filter, receive messages,
  forward payload bytes to the subscriber runtime.
- MQTT subscriber uses `EventLoop::poll` + channel to deliver received messages to
  the subscriber runtime.

**Alternatives considered**:
- Implement as a separate subscriber runtime: Rejected — the existing runtime handles
  filter matching, state machine, and variable application regardless of transport.
- Use `paho-mqtt` instead of `rumqttc`: Rejected — adds a new dependency; `rumqttc` is
  already in the dependency tree.

## Decision 4: PubSub processing limits

**Decision**: Add a bounded `tokio::sync::mpsc` channel for incoming datagrams in the
subscriber engine, and return `StatusCode::BadTooManyPublishRequests` when the channel
is full (try_send fails).

**Rationale**:
- The subscriber runtime currently uses unbounded channels; adding a bound prevents
  memory exhaustion under flood.
- `BadTooManyPublishRequests` is the OPC UA Part 4 code for publish request overflow;
  OPC-10000-14 defers to Part 4 for generic service status codes.
- This is a minimal change: add a capacity config, wrap the send in try_send, return
  error on full.

**Alternatives considered**:
- Per-DataSetWriter rate limiting: Rejected — overspecified for Gauntlet; a global
  datagram queue bound is sufficient.
- Drop oldest datagram silently: Rejected — violates OPC-10000-14 §9.1.10.1 which
  requires status indication on limit exceeded.
