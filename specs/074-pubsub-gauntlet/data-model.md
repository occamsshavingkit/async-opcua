# Data Model: PubSub Gauntlet Compliance

**Feature**: 074-pubsub-gauntlet
**Date**: 2026-07-12

No new entities. This feature extends existing PubSub types from specs 026/037.

## Existing Types Extended

### DataSetReaderMessageDataType (OPC-10000-14 §6.3)

Concrete subtypes already defined in `async-opcua-types`:
- `UadpDataSetReaderMessageDataType` — UADP-specific parameters
- `JsonDataSetReaderMessageDataType` — JSON-specific parameters (§6.3.2.4.3)

**Change**: Subscriber runtime now dispatches on this variant to select decode path.

### DataSetReaderTransportDataType (OPC-10000-14 §6.4)

Concrete subtypes:
- `DatagramDataSetReaderTransportDataType` — UDP (existing, wired in spec 037)
- `BrokerDataSetReaderTransportDataType` — MQTT/AMQP (§6.4.2, §9.3.2.4)

**Change**: `MqttSubscriber` variant added to transport module.

### PubSubStatusType (OPC-10000-14 §9.1.10.1)

State values:
- `Disabled` (0), `PreOperational` (1), `Operational` (2), `Error` (3)

**Change**: Expose per-DataSetReader Status Object on the information model.

## New Configuration Parameters

| Parameter | Type | Source | Purpose |
|-----------|------|--------|---------|
| `subscriber_datagram_queue_capacity` | usize | Config | Bound on incoming datagram channel |
