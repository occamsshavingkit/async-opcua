# Contract: Chat Server Information Model

**Feature**: 057-completeness-closeout / US4
**Source**: `cactuaroid/OpcUaChatServer` — ChatServerDesign.xml
**Namespace**: `https://github.com/cactuaroid/OpcUaChatServer`

## Node Hierarchy

```
ObjectsFolder (ns=0, i=85)
  └── ChatLogs (Object, ChatLogsType)
        ├── Post (Method)
        │     └── InputArguments (Variable) — 2 args
        └── PostCount (Variable, UInt32)
```

## Type Definitions

### ChatLogsType (ObjectType)

| Attribute | Value |
|-----------|-------|
| NodeId | `ns=1;i=1` |
| BaseType | BaseObjectType (ns=0;i=58) |
| SupportsEvents | true |
| HasNotifier | → Server (ns=0;i=2253) |

### PostMethodType (Method)

| Attribute | Value |
|-----------|-------|
| NodeId | `ns=1;i=5` |
| AlwaysGeneratesEvent | → ChatLogEventType |

**Input Arguments**:

| # | Name | DataType | Description |
|---|------|----------|-------------|
| 0 | Name | String | Sender display name |
| 1 | Content | String | Message text |

### ChatLogEventType (ObjectType)

| Attribute | Value |
|-----------|-------|
| NodeId | `ns=1;i=7` |
| BaseType | BaseEventType (ns=0;i=2041) |

**Properties**:

| Name | Type | Description |
|------|------|-------------|
| ChatLog | ChatLogType (VariableType), DataType=ChatLog | The chat log entry |

### ChatLogType (VariableType)

| Attribute | Value |
|-----------|-------|
| NodeId | `ns=1;i=18` |
| BaseType | BaseDataVariableType |
| DataType | ChatLog |
| ValueRank | Scalar (-1) |

### ChatLog (DataType / Structure)

| Field | DataType | Description |
|-------|----------|-------------|
| At | DateTime | Timestamp when message was posted |
| Name | String | Sender display name |
| Content | String | Message text |

## Runtime Behavior

### Post Method Handler

```
1. Receive (Name: String, Content: String)
2. Create ChatLog { At = now(), Name, Content }
3. Increment PostCount
4. Fire ChatLogEventType event with ChatLog property set
5. Return Good status
```

### Event Subscription

Clients subscribe to `ChatLogs` as the event source, filtering for `ChatLogEventType`. Each call to `Post` generates one event delivered to all subscribed clients.

### Event Filter (Select Clause)

To receive only the ChatLog property: `SelectClause: ChatLogEventType/ChatLog`

## Implementation in async-opcua

### Custom Type Registration

The `ChatLog` structure is registered at startup using the existing type system. Since it cannot be generated from a NodeSet2 (no in-tree codegen), it is hand-registered:

1. Register `ChatLog` as a custom DataType using the type loader or manual node construction
2. Register `ChatLogType` as a VariableType with DataType=ChatLog
3. Register `ChatLogEventType` as an ObjectType extending BaseEventType with a ChatLog property
4. Register `ChatLogsType` as an ObjectType with Post Method, PostCount Variable, SupportsEvents, and HasNotifier→Server
5. Instantiate `ChatLogs` under ObjectsFolder
6. Register the Post method callback

### Namespace Management

The server uses namespace index 1 (after the OPC UA namespace 0) for the chat model. All type NodeIds use `ns=1`. Instance NodeIds (ChatLogs, Post, PostCount) use `ns=1` with application-specific numeric IDs.

### Interop Testability

A client written against the same model (e.g., the C# `OpcUaChatServer` client, or a Python asyncua client) should be able to:
1. Browse ChatLogs and discover Post/PostCount
2. Call Post("Alice", "Hello")
3. See PostCount increment
4. Subscribe to ChatLogEventType and receive the event with ChatLog{At, Name="Alice", Content="Hello"}
