# Quickstart: PubSub Gauntlet Compliance

**Feature**: 074-pubsub-gauntlet
**Date**: 2026-07-12

## Prerequisites

- Rust toolchain (stable)
- MQTT broker for broker tests (e.g., `mosquitto` running on localhost:1883)
- Working directory: `/home/quackdcs/async-opcua`
- Branch: `074-pubsub-gauntlet`

## Verify Current State

```bash
git branch --show-current
# Should output: 074-pubsub-gauntlet

cargo build --workspace
cargo test -p async-opcua-pubsub
```

## Key Files

| Area | File | What Changes |
|------|------|-------------|
| Subscriber dispatch | `async-opcua-pubsub/src/subscriber.rs` | JSON message dispatch |
| JSON codec | `async-opcua-pubsub/src/codec/json.rs` | Enhanced decode |
| MQTT subscriber | `async-opcua-pubsub/src/transport/mqtt.rs` | New subscriber variant |
| Engine wiring | `async-opcua-pubsub/src/engine.rs` | Start subscriber loops |
| Demo server | `samples/demo-server/src/` | Wire engine startup |
| Config | `async-opcua-pubsub/src/config.rs` | Accept broker/JSON configs |

## Build & Test

```bash
cargo build -p async-opcua-pubsub
cargo test -p async-opcua-pubsub

# Integration tests with pubsub feature
cargo test -p async-opcua --features pubsub --test integration_tests

# Full CI gate
tools/ci-playbook.sh --ci
```
