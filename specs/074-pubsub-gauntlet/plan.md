# Implementation Plan: PubSub Gauntlet Compliance

**Branch**: `074-pubsub-gauntlet` | **Date**: 2026-07-12 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/074-pubsub-gauntlet/spec.md`

## Summary

Wire the existing PubSub subscriber runtime (specs 026, 037) to the demo server and add
JSON and MQTT broker subscriber support to pass the 6 remaining OPC-10000-14 Gauntlet tests.

## Technical Context

**Language/Version**: Rust (stable)
**Primary Dependencies**: async-opcua-pubsub (existing), rumqttc (MQTT), tokio (async runtime)
**Storage**: In-memory PubSub configuration
**Testing**: `cargo test` with existing subscriber test suites
**Target Platform**: Linux server
**Project Type**: Library + demo server binary
**Performance Goals**: No regression on existing throughput
**Constraints**: Must not break existing UADP subscriber or publisher functionality
**Scale/Scope**: 6 Gauntlet tests, ~4 sub-features

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Check | Status |
|-----------|-------|--------|
| I. Correctness Over Completion | Each requirement mapped to an OPC-10000-14 section | PASS |
| II. Do It Right Once | Builds on existing tested subscriber implementation | PASS |
| III. Individual Task Discipline | Tasks organized by independent user story | PASS |
| IV. Security Is Paramount | Security downgrade rejection enforced; fail-closed defaults | PASS |
| V. Leave It Better Than You Found It | Existing subscriber runtime reused, not rewritten | PASS |

**Gate result**: All principles pass.

## Project Structure

### Documentation

```text
specs/074-pubsub-gauntlet/
├── plan.md, research.md, data-model.md, quickstart.md
├── contracts/
└── tasks.md
```

### Source Code

```text
async-opcua-pubsub/src/
├── subscriber.rs          # PRINCIPAL: dispatch JSON, broker transport start
├── codec/json.rs          # Enhance JSON decode path
├── transport/mqtt.rs      # Add MQTT subscriber (subscribe + receive)
├── engine.rs              # Wire subscriber start to demo server
└── config.rs              # Accept broker + JSON configs

samples/demo-server/
└── src/main.rs or customs.rs  # Wire pubsub engine subscriber start

async-opcua-pubsub/tests/
├── subscriber_json_tests.rs    # NEW: JSON subscriber tests
└── subscriber_mqtt_tests.rs    # NEW: MQTT broker subscriber tests
```

### Complexity Tracking

No violations to justify.
