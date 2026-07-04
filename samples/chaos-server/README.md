# Chaos Server

An OPC UA server that deliberately misbehaves — nodes randomly change type, value, and status code to exercise client error-handling paths.

## Running

```bash
cargo run
```

## What it does

A background task continuously mutates a set of Variable nodes under `Objects/ChaosFolder`:
- Randomly changes values to different variant types
- Randomly flips status codes between Good, BadUnexpectedError, BadOutOfService, and BadNoData

Use this server to test how your OPC UA client handles unexpected data changes, type mismatches, and error statuses.
