# CU Coverage Report Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Generate an in-repo, CU-indexed Foundation profile coverage report from the normalized OPC UA Foundation profile snapshot.

**Architecture:** Add a small Rust workspace tool under `tools/cu-coverage-report`. The library parses the normalized profile snapshot, classifies CU IDs with explicit evidence rules, and renders a Markdown report; the binary wires CLI paths to the library.

**Tech Stack:** Rust 2021, `serde`, `serde_json`, workspace `cargo test`, Markdown output.

## Global Constraints

- Keep the implementation local and deterministic; tests use inline fixture JSON, not `/home/quackdcs/micro-opcua`.
- Preserve conservative status language: `implemented`, `partial`, `gap`, `needs-proof`, and `source-issue` are evidence labels, not certification claims.
- Do not add blocking locks or async synchronization.
- Use TDD for behavior changes.

---

### Task 1: CU Coverage Report Tool

**Files:**
- Create: `tools/cu-coverage-report/Cargo.toml`
- Create: `tools/cu-coverage-report/src/lib.rs`
- Create: `tools/cu-coverage-report/src/main.rs`
- Modify: `Cargo.toml`

**Interfaces:**
- Consumes: normalized snapshot JSON containing `canonical_profiles`, `conformance_units`, and `relationships.transitive_cu_closure`.
- Produces: `generate_markdown_report(snapshot: &NormalizedSnapshot) -> String` and CLI output to stdout or a file.

- [x] **Step 1: Write the failing tests**

Create `tools/cu-coverage-report/src/lib.rs` with tests that construct a minimal normalized snapshot fixture. The tests assert that:

```rust
let snapshot = parse_snapshot(FIXTURE).expect("fixture parses");
let report = generate_markdown_report(&snapshot);
assert!(report.contains("Nano Embedded Device 2025 Server Profile"));
assert!(report.contains("| 2478 | Time Sync - OS based support | gap |"));
assert!(report.contains("| 3912 | Base Info Server Capabilities 2 | partial |"));
assert!(report.contains("| 5592 | Missing from normalized CU list | source-issue |"));
```

- [x] **Step 2: Run test to verify it fails**

Run: `cargo test -p async-opcua-cu-coverage-report --lib -- --nocapture`

Expected: FAIL because the new crate/functions do not exist yet.

- [x] **Step 3: Write minimal implementation**

Implement:

```rust
pub fn parse_snapshot(input: &str) -> Result<NormalizedSnapshot, serde_json::Error>;
pub fn generate_markdown_report(snapshot: &NormalizedSnapshot) -> String;
```

Use explicit CU evidence rules copied from `docs/opcua-foundation-profile-roadmap.md`.

- [x] **Step 4: Run test to verify it passes**

Run: `cargo test -p async-opcua-cu-coverage-report --lib -- --nocapture`

Expected: PASS.

- [x] **Step 5: Validate the real snapshot path**

Run: `cargo run -p async-opcua-cu-coverage-report -- /home/quackdcs/micro-opcua/profiles/opcua-profile-normalized-snapshot.json /tmp/opcua-cu-report.md`

Expected: command exits 0 and `/tmp/opcua-cu-report.md` contains all four canonical profile sections.

- [x] **Step 6: Run formatting and diff checks**

Run: `cargo fmt --check && git diff --check`

Expected: PASS.

## Self-Review

- Spec coverage: Implements the roadmap Bucket 1 first deliverable: normalized CU registry/report from the Foundation snapshot.
- Placeholder scan: No placeholder task text is left; deferred CU closures remain explicit TODO entries in `TODO.md`.
- Type consistency: `NormalizedSnapshot`, `parse_snapshot`, and `generate_markdown_report` are the only cross-file tool interfaces.
