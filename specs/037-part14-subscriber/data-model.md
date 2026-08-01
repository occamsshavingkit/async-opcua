# Data Model: Part 14 Subscriber Runtime

> **Current implementation**: Spec 074 extended this model with MQTT UADP/JSON
> subscribers and connection-scoped direct ingress. The sections below describe
> the resulting runtime where they supersede the original spec 037 boundary.

## ReaderGroupConfig

Represents a subscriber-side grouping of DataSetReaders.

Fields to preserve or extend:

- `reader_group_id`: local identifier for the ReaderGroup.
- `dataset_readers`: list of `DataSetReaderConfig` entries.
- `security_mode`: optional shared security mode for received NetworkMessages.
- `security_policy_uri`: optional shared security policy URI.
- `security_group_id`: optional shared security group id.

Validation:

- DataSetReader names must be unique within the group. (OPC 10000-14 Section 6.2.9.13.1)
- Shared security settings must be complete when security mode requires signing or encryption. (OPC 10000-14 Section 6.2.5.2)

## DataSetReaderConfig

Represents the Part 14 subscriber filter, decode, security, timeout, and target-apply settings for one received stream.

Fields to preserve or extend:

- `name`: optional human-readable name, unique within a ReaderGroup.
- `dataset_reader_id`: local identifier.
- `publisher_id`: optional subscriber-side PublisherId filter. (OPC 10000-14 Section 6.2.9.1)
- `writer_group_id`: optional UADP WriterGroupId filter. (OPC 10000-14 Section 6.2.9.2)
- `network_message_number`: optional UADP NetworkMessageNumber filter. (OPC 10000-14 Section 7.2.4.4.2)
- `dataset_writer_id`: DataSetWriterId filter; zero means ignore this filter. (OPC 10000-14 Section 6.2.9.3)
- `message_receive_timeout`: optional timeout from first Operational state. (OPC 10000-14 Section 6.2.9.6)
- `dataset_metadata`: configured metadata needed to decode field order and validate versions. (OPC 10000-14 Section 5.4.2.2)
- `security_mode`: optional DataSetReader security override. (OPC 10000-14 Section 6.2.9.9)
- `security_policy_uri`: optional DataSetReader security override.
- `security_group_id`: optional DataSetReader security override.
- `target_variables`: list of `FieldTargetConfig`.
- `subscribed_variables`: legacy shorthand mapped to `FieldTargetConfig` entries using Value attribute and field index order.

Validation:

- The runtime must reject missing target mappings for supported receive mode.
- The runtime must reject duplicate target NodeIds within one target list. (OPC 10000-14 Section 6.2.10.2.1)
- The runtime supports UADP and JSON key-frame readers over supported transports.
- The runtime must reject RawData, delta frame, event, AMQP, WebSocket, `mqtts://`, and TSN subscriber settings with explicit errors.
- The runtime must validate security override completeness before starting a receive loop.

## FieldTargetConfig

Local representation of the Part 14 FieldTargetDataType relation between a DataSet field and a target Variable.

Fields:

- `dataset_field_index`: zero-based field index for UADP field order.
- `dataset_field_id`: optional Guid when metadata supplies stable field ids.
- `target_node_id`: target Variable NodeId.
- `attribute_id`: target AttributeId; first implementation supports Value.
- `index_range`: optional NumericRange placeholder; unsupported ranges are rejected until implemented.
- `override_value_handling`: configured handling for non-Operational state or Bad field status; unsupported modes fail closed until implemented.

Validation:

- `target_node_id` must resolve to a Variable node before any update is applied.
- `attribute_id` must be Value for the first implementation.
- `dataset_field_index` must be within the decoded DataSetMessage field count.

## SubscriberRuntime

Coordinates UADP/JSON decode, DataSetReader dispatch, target apply, timeout
evaluation, and diagnostics. Transport tasks and the security registry are
owned by `PubSubEngine`, not by the runtime.

Fields:

- `address_space`: shared AddressSpace handle used during bounded apply
  operations.
- `connection_ids`: set of all configured connection ids.
- `secured_connection_ids`: set of connection ids whose validated effective
  reader security resolves to `Sign` or `SignAndEncrypt` after DataSetReader
  overrides are applied.
- `readers`: all configured `BoundDataSetReader` instances.
- `reader_records`: runtime status and timeout records keyed by
  `DataSetReaderKey(connection_id, dataset_reader_id)`.

Identity and ingress rules:

- Connection ids are unique across a runtime. DataSetReader ids are unique within one connection, across its ReaderGroups. Duplicate identities fail with `BadConfigurationError`.
- `reader_status_by_key` is the authoritative status lookup. Numeric-only `reader_status(u16)` returns a value only when that id is unambiguous across all connections.
- `process_datagram_for_connection` and `process_network_message_for_connection` select readers belonging to the named connection. Raw-byte methods fail closed with `BadSecurityChecksFailed` for secured connections; secured ingress must use `PubSubEngine::process_subscriber_datagram`.
- Unscoped `process_datagram` and `process_network_message` are compatibility APIs for runtimes with at most one connection; they return `BadInvalidArgument` for multi-connection runtimes.

Invariants:

- No payload decode or target mutation occurs until required security checks pass.
- Target mutation for one DataSetReader update is all-or-nothing.
- Engine cancellation stops receive loops without leaking tasks.

## DataSetReaderStatus

Observable per-reader status snapshot.

Fields:

- `state`: Disabled, PreOperational, Operational, or Error.
- `last_sequence_number`: optional last accepted sequence number.
- `last_receive_time`: optional monotonic receive timestamp.
- `last_error`: optional structured error code.
- `accepted_count`: accepted DataSetMessages.
- `filtered_count`: messages filtered by reader criteria.
- `dropped_count`: malformed or unsupported datagrams.
- `sequence_gap_count`: missing sequence observations.
- `duplicate_count`: duplicate sequence observations.
- `out_of_order_count`: out-of-order sequence observations.
- `timeout_count`: MessageReceiveTimeout expirations.
- `security_failure_count`: failed signature, encryption, token, nonce, or replay checks.

State rules:

- Enabled reader starts PreOperational until first accepted key-frame DataSetMessage. (OPC 10000-14 Section 6.2.1)
- Operational reader moves to Error after MessageReceiveTimeout without a new DataSetMessage. (OPC 10000-14 Section 6.2.9.6)
- Error reader caused by timeout returns to Operational on the next valid new DataSetMessage.
- Metadata major-version mismatch moves the reader to Error if updated metadata is unavailable within MessageReceiveTimeout. (OPC 10000-14 Section 6.2.9.4)

## SubscriberApplyOutcome

Result returned by one datagram process operation.

Fields:

- `matched_readers`: count of readers whose filters matched the message.
- `applied_readers`: count of readers whose target Variables were updated.
- `filtered_readers`: count of readers that rejected the message by filters.
- `dropped_reason`: optional structured failure for datagram-level drop.

Usage:

- Tests can assert outcomes without inspecting logs.
- Engine receive loops can update diagnostics from one structured result.
