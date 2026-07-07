# Feature Specification: Shrink Foundation Profile Footprints

**Feature Branch**: `066-shrink-footprints`  
**Created**: 2026-07-07  
**Status**: Draft  
**Input**: User description: "Shrink the nano, micro, and embedded foundation profile binary sizes."

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Nano profile is sub-5 MB (Priority: P1)

An embedded systems engineer wants to deploy an OPC UA server on a resource-constrained MCU with limited flash storage. The current 12 MB nano profile binary is too large. After optimization, the stripped release binary is under 5 MB.

**Why this priority**: The nano profile is the entry point — if it's bloated, every larger profile inherits the bloat.

**Independent Test**: `cargo build --release -p async-opcua-foundation-profile-nano-server && strip target/release/async-opcua-foundation-profile-nano-server && ls -lh` shows < 5 MB. All existing nano profile tests pass.

**Acceptance Scenarios**:

1. **Given** the nano profile built in release mode and stripped, **When** measured with `ls -lh`, **Then** the binary is under 5 MB.
2. **Given** the optimized nano server, **When** the profile smoke tests run, **Then** all tests pass (same OPC UA behavior, reduced size).

---

### User Story 2 — Micro profile is sub-7 MB (Priority: P2)

The micro profile adds data-change subscriptions. Currently 13 MB. After optimization, under 7 MB.

**Why this priority**: Micro is the smallest profile with real-time data capability — the most common embedded deployment target.

**Independent Test**: Stripped release binary under 7 MB. Micro profile smoke tests pass.

---

### User Story 3 — Embedded profile is sub-10 MB (Priority: P3)

The embedded profile adds standard subscription tier (deadband, triggering), events, alarms, and history. Currently 26 MB. After optimization, under 10 MB.

**Why this priority**: The jump from 13 MB (micro) to 26 MB (embedded) suggests a disproportionate dependency being pulled in.

**Independent Test**: Stripped release binary under 10 MB. Embedded profile smoke tests pass.

---

### Edge Cases

- **Optimization must not regress functionality**: All existing profile smoke tests must pass after each optimization.
- **Optimization must not regress standard profile**: The standard profile (full-featured) must continue to work and pass all tests.
- **Debug builds may be larger**: Only release + stripped sizes are gated.
- **CI footprint check**: The CI footprint job must still pass with the new sizes.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Nano profile binary (release, stripped) MUST be under 5 MB.
- **FR-002**: Micro profile binary (release, stripped) MUST be under 7 MB.
- **FR-003**: Embedded profile binary (release, stripped) MUST be under 10 MB.
- **FR-004**: All current profile smoke tests MUST continue to pass.
- **FR-005**: The standard profile and all other crates MUST continue to build and pass all tests.
- **FR-006**: CI footprint check MUST be updated with new size thresholds.

## Success Criteria *(mandatory)*

- **SC-001**: Nano profile stripped binary size reduced from 12 MB to under 5 MB (≥58% reduction).
- **SC-002**: Micro profile stripped binary size reduced from 13 MB to under 7 MB (≥46% reduction).
- **SC-003**: Embedded profile stripped binary size reduced from 26 MB to under 10 MB (≥61% reduction).
- **SC-004**: All 618+ existing tests continue to pass.
- **SC-005**: CI footprint job passes with updated size thresholds.

## Assumptions

- Binary size is measured as the stripped release ELF binary size on Linux x86_64.
- Most bloat comes from: (a) monomorphization of generic functions, (b) unused code pulled in by feature unification, (c) large vendored dependencies (rustls, aws-lc-rs), (d) debug symbol inclusion despite release build.
- `cargo bloat` and `cargo llvm-lines` can identify the largest contributors to binary size.
- LTO (Link-Time Optimization) and `opt-level = "s"` / `opt-level = "z"` can significantly reduce binary size.
- Some dependencies (rustls, aws-lc-rs for crypto) may need feature flagging to reduce their inclusion in smaller profiles.
- The `#[cfg(feature = ...)]` gates on the facade crate (`async-opcua`) already correctly select subsystem features; the bloat is from transitive deps and monomorphization within enabled subsystems.
