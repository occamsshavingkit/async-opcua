# OPC UA Chat Server

Demonstrates an OPC UA-based chat system using an information model with Events, Methods, and structured data types.

## Model

- **ChatLogs** object (SupportsEvents) under ObjectsFolder
- **Post** method (inputs: Name/String, Content/String) — fires a ChatLogEventType event
- **PostCount** variable (UInt32) — increments on each Post

Clients "send" messages by calling Post; they "receive" by subscribing to ChatLogEventType.

## Running

```bash
cargo run
```

## Testing with a client

Connect with any OPC UA client (e.g., UaExpert):
1. Browse to `Objects/ChatLogs`
2. Subscribe to ChatLogEventType events from ChatLogs
3. Call Post("Alice", "Hello world")
4. Observe the event delivery and PostCount increment
