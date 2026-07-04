# Reverse Bridge

Connects to another OPC UA server, reads its variables, and mirrors them in its own address space. Demonstrates using both the server and client halves of async-opcua together.

## Running

```bash
# Start a source server (e.g., demo-server) first
cargo run -- --source opc.tcp://localhost:4855
```

## How it works

1. Connects to the source OPC UA server via `--source`
2. Recursively browses the source's Objects folder for Variable nodes
3. Reads each variable's current value
4. Creates mirrored Variable nodes in its own address space with the same values

Connect to the reverse bridge with any client to see the mirrored data.
