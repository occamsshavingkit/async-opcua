# Contracts: PubSub Gauntlet Compliance

**Feature**: 074-pubsub-gauntlet
**Date**: 2026-07-12

No new external interfaces. Internal changes to subscriber runtime dispatch and transport.

## Internal Service Contracts

### Subscriber Message Dispatch

**Before**: `process_network_message` only handles UADP decode; JSON messages are rejected.

**After**: Inspect `DataSetReaderMessageDataType` variant:
- `UadpDataSetReaderMessage` → existing UADP decode path
- `JsonDataSetReaderMessage` → new JSON decode path (OPC-10000-14 §7.2.5)

### Transport Dispatch

**Before**: Only UDP datagram transport (`DatagramDataSetReaderTransport`). Broker transports reject.

**After**: Match on concrete transport type:
- `DatagramDataSetReaderTransport` → existing UDP subscriber
- `BrokerDataSetReaderTransport` → new MQTT subscriber (OPC-10000-14 §6.4.2)

### Demo Server Wiring

**Before**: `PubSubEngine` created but subscriber loops not started by demo server.

**After**: Demo server calls `engine.start_subscribers()` after server initialization completes.
