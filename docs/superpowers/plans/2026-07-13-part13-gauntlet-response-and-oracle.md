# Part 13 Gauntlet Response + Oracle Probe Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix or verify the OPC UA Gauntlet Part 13 `HistoryData.DataValues` response-shape failure, then add a focused oracle probe that identifies whether bounded aggregate math needs a follow-up correction.

**Architecture:** Keep coding work intentionally narrow. First prove the `ReadProcessed` service path returns `HistoryData { data_values: Some(non_empty_vec) }` for aggregate reads. Then add one spec-grounded aggregate-engine oracle test around bounded aggregate behavior, fixing only a localized mismatch if the failing test points to an obvious implementation error.

**Tech Stack:** Rust, async-opcua server/client integration tests, `async-opcua-server` aggregate unit tests, Tokio tests, OPC UA Part 13.

## Global Constraints

- Preserve async architecture. Do not add locks, mutexes, semaphores, blocking waits, or blocking I/O.
- Do not weaken HistoryRead or aggregate error semantics to satisfy a test harness.
- Ground Part 13 claims in OPC-10000-13 before changing aggregate math.
- Keep each implementation task atomic and independently reviewable.
- Do not update issue `#288` based on assumptions. Only update it after a fresh Gauntlet run or direct reproducer evidence.
- Use `apply_patch` for manual edits.
- Run targeted tests before reporting task completion.

---

## Current Evidence

- Issue `#288` reports Part 13 failures `P13-S05.5-001`, `P13-S05.5-002`, and `P13-S05.5-003`: expected `Results[0].HistoryData.DataValues` present and non-empty; got `HistoryData` with missing/null `DataValues`.
- Current `async-opcua-server/src/aggregates/middleware.rs:95-100` already wraps backend processed values with `HistoryData { data_values: Some(processed_values) }` and sets node status `Good`.
- Current `async-opcua-server/src/history/backend.rs:127-130` and `async-opcua-history-sqlite/src/backend.rs:687-690` already call `read_raw_modified(..., return_bounds=true, ...)`.
- Current `async-opcua-server/src/aggregates/engine.rs:273-289` already has `AggregateInput.prior`, `AggregateInput.next`, `AggregateInput.config`, and `AggregateInput.stepped`.
- Current `async-opcua-server/src/aggregates/engine.rs:46-82` advertises 35 aggregate ids, and `async-opcua/tests/integration/browse.rs:801-829` verifies advertised aggregate functions.
- OPC-10000-13 reference search confirms `Total` uses interpolated bounding values and `StartBound`, `EndBound`, and `DeltaBounds` use simple bounding values.

---

## File Map

- `async-opcua/tests/integration/read.rs`: Add end-to-end processed HistoryRead response-shape test. This is the Gauntlet-symptom guard.
- `async-opcua-server/src/aggregates/middleware.rs`: Only modify if the response-shape test proves `HistoryData.data_values` can still be null or empty on a valid aggregate read.
- `async-opcua-server/tests/aggregates_tests.rs`: Add the focused oracle probe for bounded aggregate behavior.
- `async-opcua-server/src/aggregates/engine.rs`: Only modify if the oracle probe fails and the root cause is localized to one aggregate or bound helper.
- `async-opcua-server/src/history/backend.rs`: Only modify if backend output is empty despite valid raw history and valid aggregate request.
- `async-opcua-history-sqlite/src/backend.rs`: Do not touch unless the in-memory backend fix reveals the same bug in the SQLite override.

---

## Task 1: Add Gauntlet Response-Shape Reproducer

**Assigned to:** `software-engineer-zai`

**Purpose:** Prove whether `HistoryRead(ReadProcessed)` currently returns `HistoryData.DataValues = Some(non_empty)` through the public client/server path.

**Files:**
- Modify: `async-opcua/tests/integration/read.rs`

**Interfaces:**
- Consumes: existing `setup().await` helper in `async-opcua/tests/integration/read.rs`.
- Consumes: existing memory node manager `add_history` helper.
- Produces: one ignored-by-nothing Tokio integration test named `history_read_processed_returns_history_data_values`.

- [ ] **Step 1: Locate imports**

Open `async-opcua/tests/integration/read.rs` and confirm it already imports `HistoryReadAction`
from `opcua::client` and these processed-history types from `opcua::types`:

```rust
use chrono::TimeDelta;
use opcua::client::HistoryReadAction;
use opcua::types::{
    AggregateConfiguration, DataTypeId, DataValue, DateTime, HistoryData,
    HistoryReadValueId, NodeId, ReadProcessedDetails, StatusCode,
    TimestampsToReturn, Variant,
};
```

If any names are missing, add only the missing names to the existing import list. Do not duplicate imports.

- [ ] **Step 2: Add the failing test**

Append this test near the other history read tests in `async-opcua/tests/integration/read.rs`:

```rust
#[tokio::test]
async fn history_read_processed_returns_history_data_values() {
    let (tester, nm, session) = setup().await;

    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&id, "ProcessedVar", "ProcessedVar")
            .historizing(true)
            .value(0.0)
            .description("Processed history value")
            .data_type(DataTypeId::Double)
            .access_level(AccessLevel::CURRENT_READ | AccessLevel::HISTORY_READ)
            .user_access_level(AccessLevel::CURRENT_READ | AccessLevel::HISTORY_READ)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );

    let start = DateTime::now() - TimeDelta::try_seconds(30).unwrap();
    nm.inner().add_history(
        &id,
        [0.0, 10.0, 20.0].into_iter().enumerate().map(|(idx, value)| {
            let timestamp = start + TimeDelta::try_seconds((idx as i64) * 10).unwrap();
            DataValue {
                value: Some(Variant::Double(value)),
                status: Some(StatusCode::Good),
                source_timestamp: Some(timestamp),
                server_timestamp: Some(timestamp),
                ..Default::default()
            }
        }),
    );

    let results = session
        .history_read(
            HistoryReadAction::ReadProcessedDetails(ReadProcessedDetails {
                start_time: start,
                end_time: start + TimeDelta::try_seconds(20).unwrap(),
                processing_interval: 20_000.0,
                aggregate_type: Some(vec![NodeId::new(0u16, 2343u32)]),
                aggregate_configuration: AggregateConfiguration::default(),
            }),
            TimestampsToReturn::Both,
            false,
            &[HistoryReadValueId {
                node_id: id,
                index_range: Default::default(),
                data_encoding: Default::default(),
                continuation_point: Default::default(),
            }],
        )
        .await
        .unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status_code, StatusCode::Good);
    let history_data = results[0]
        .history_data
        .inner_as::<HistoryData>()
        .expect("processed HistoryRead should return HistoryData");
    let data_values = history_data
        .data_values
        .as_ref()
        .expect("processed HistoryRead should include DataValues");
    assert_eq!(data_values.len(), 1);
    assert_eq!(data_values[0].status, Some(StatusCode::Good));
    assert!(data_values[0].value.is_some());
}
```

- [ ] **Step 3: Run the test and capture the result**

Run:

```bash
cargo test -p async-opcua --test integration_tests integration::read::history_read_processed_returns_history_data_values -- --nocapture
```

Expected if already fixed: PASS.

Expected if the Gauntlet symptom still reproduces: FAIL at one of these assertions:

```text
processed HistoryRead should return HistoryData
processed HistoryRead should include DataValues
assertion `left == right` failed ... data_values.len()
```

- [ ] **Step 4: Report evidence to the planner**

Return:

```text
Task 1 result:
- Test command:
- PASS/FAIL:
- Failure message if any:
- Files changed:
```

Do not fix code in Task 1 unless explicitly instructed after review.

---

## Task 2: Fix Response Shape Only If Task 1 Fails

**Assigned to:** `software-engineer-zai`

**Purpose:** Make a valid processed HistoryRead return `HistoryData.data_values = Some(non_empty_vec)` without changing aggregate math.

**Files:**
- Modify only one of these if needed: `async-opcua-server/src/aggregates/middleware.rs`, `async-opcua-server/src/history/backend.rs`, `async-opcua-server/src/node_manager/memory/simple.rs`
- Test: `async-opcua/tests/integration/read.rs`

**Interfaces:**
- Consumes: Task 1 failing test `history_read_processed_returns_history_data_values`.
- Produces: passing Task 1 test.

- [ ] **Step 1: Re-run the failing test**

Run:

```bash
cargo test -p async-opcua --test integration_tests integration::read::history_read_processed_returns_history_data_values -- --nocapture
```

Expected: FAIL. If it passes, stop and report that no response-shape fix is needed.

- [ ] **Step 2: Inspect backend output before changing code**

Add temporary local logging only if needed. Prefer reading the call path first:

```rust
// async-opcua-server/src/aggregates/middleware.rs
Ok((processed_values, _continuation_point)) => {
    hn.set_result(HistoryData {
        data_values: Some(processed_values),
    });
    hn.set_status(StatusCode::Good);
}
```

If `processed_values` is empty for a valid raw series, inspect `read_processed` and `compute_processed_intervals`. If `processed_values` is non-empty but the client sees missing `DataValues`, inspect `HistoryNode::set_result` and response encoding.

- [ ] **Step 3: Apply the minimal fix**

Use the smallest code change matching the actual root cause:

```rust
// If the backend returns an empty Vec because a valid aggregate produced no value,
// fix the aggregate/backend path. Do not fake a DataValue in middleware.
```

or:

```rust
// If the node result is being dropped after hn.set_result(...), fix that result propagation.
// Preserve hn.set_status(StatusCode::Good) only for successful backend reads.
```

Do not add this anti-pattern:

```rust
// Do NOT do this.
if processed_values.is_empty() {
    processed_values.push(DataValue::default());
}
```

- [ ] **Step 4: Verify the response-shape test passes**

Run:

```bash
cargo test -p async-opcua --test integration_tests integration::read::history_read_processed_returns_history_data_values -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Verify adjacent history read tests**

Run:

```bash
cargo test -p async-opcua --test integration_tests integration::read -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Report evidence to the planner**

Return:

```text
Task 2 result:
- Root cause:
- Minimal fix:
- Test commands:
- PASS/FAIL:
- Files changed:
```

Do not commit unless the planner asks for a commit.

---

## Task 3: Add Focused Aggregate Oracle Probe

**Assigned to:** `software-engineer-zai`

**Purpose:** Add a small aggregate-engine test that checks current bounded aggregate behavior. This identifies whether a follow-up math correction is needed without bundling a broad rewrite.

**Files:**
- Modify: `async-opcua-server/tests/aggregates_tests.rs`

**Interfaces:**
- Consumes: `dispatch_aggregate`, `AggregateInput`, `AggregateConfiguration`, `NodeId`, `DataValue`, `DateTime`, `StatusCode`, `Variant`.
- Produces: one test named `part13_start_end_and_delta_bounds_use_simple_bounds`.

- [ ] **Step 1: Add helper functions in the test file**

Add these near the existing `calculate_aggregate` helper in `async-opcua-server/tests/aggregates_tests.rs`:

```rust
fn good_double(timestamp: DateTime, value: f64) -> DataValue {
    DataValue {
        value: Some(Variant::Double(value)),
        status: Some(StatusCode::Good),
        source_timestamp: Some(timestamp),
        server_timestamp: Some(timestamp),
        ..Default::default()
    }
}

fn bounded_aggregate(
    values: &[&DataValue],
    prior: Option<&DataValue>,
    next: Option<&DataValue>,
    aggregate_type: &NodeId,
    start: DateTime,
    end: DateTime,
) -> DataValue {
    let config = AggregateConfiguration::default();
    dispatch_aggregate(
        aggregate_type,
        &AggregateInput {
            values,
            annotations: &[],
            prior,
            next,
            interval_start: start,
            interval_end: end,
            config: &config,
            stepped: true,
        },
    )
}
```

- [ ] **Step 2: Add the oracle test**

Add this test in `async-opcua-server/tests/aggregates_tests.rs`:

```rust
#[test]
fn part13_start_end_and_delta_bounds_use_simple_bounds() {
    let before = DateTime::from((2026, 7, 13, 12, 0, 0));
    let start = DateTime::from((2026, 7, 13, 12, 0, 10));
    let inside = DateTime::from((2026, 7, 13, 12, 0, 15));
    let end = DateTime::from((2026, 7, 13, 12, 0, 20));
    let after = DateTime::from((2026, 7, 13, 12, 0, 30));

    let prior = good_double(before, 5.0);
    let in_interval = good_double(inside, 11.0);
    let next = good_double(after, 23.0);

    let start_bound = bounded_aggregate(
        &[&in_interval],
        Some(&prior),
        Some(&next),
        &NodeId::new(0u16, 11505u32),
        start,
        end,
    );
    assert_eq!(start_bound.status, Some(StatusCode::Good));
    assert_eq!(start_bound.value, Some(Variant::Double(5.0)));

    let end_bound = bounded_aggregate(
        &[&in_interval],
        Some(&prior),
        Some(&next),
        &NodeId::new(0u16, 11506u32),
        start,
        end,
    );
    assert_eq!(end_bound.status, Some(StatusCode::Good));
    assert_eq!(end_bound.value, Some(Variant::Double(11.0)));

    let delta_bounds = bounded_aggregate(
        &[&in_interval],
        Some(&prior),
        Some(&next),
        &NodeId::new(0u16, 11507u32),
        start,
        end,
    );
    assert_eq!(delta_bounds.status, Some(StatusCode::Good));
    assert_eq!(delta_bounds.value, Some(Variant::Double(6.0)));
}
```

The expected values encode simple bounds: `StartBound = prior`, `EndBound = last value at or before end`, and `DeltaBounds = EndBound - StartBound`.

- [ ] **Step 3: Run the oracle test**

Run:

```bash
cargo test -p async-opcua-server --test aggregates_tests part13_start_end_and_delta_bounds_use_simple_bounds -- --nocapture
```

Expected if already correct: PASS.

Expected if current simple-bound behavior is wrong: FAIL with a concrete value/status mismatch.

- [ ] **Step 4: Report evidence to the planner**

Return:

```text
Task 3 result:
- Test command:
- PASS/FAIL:
- Observed values if failed:
- Files changed:
```

Do not fix aggregate math in Task 3 unless explicitly instructed after review.

---

## Task 4: Fix One Localized Bounded-Aggregate Mismatch If Task 3 Fails

**Assigned to:** `software-engineer-zai`

**Purpose:** Correct only the simple-bound aggregate behavior exposed by Task 3, if it fails. Do not broaden into TimeAverage/Total unless the planner assigns a separate task.

**Files:**
- Modify: `async-opcua-server/src/aggregates/engine.rs`
- Test: `async-opcua-server/tests/aggregates_tests.rs`

**Interfaces:**
- Consumes: Task 3 failing test.
- Produces: passing Task 3 test and no aggregate regression.

- [ ] **Step 1: Re-run the failing oracle test**

Run:

```bash
cargo test -p async-opcua-server --test aggregates_tests part13_start_end_and_delta_bounds_use_simple_bounds -- --nocapture
```

Expected: FAIL. If it passes, stop and report that no localized simple-bound fix is needed.

- [ ] **Step 2: Inspect current simple-bound helpers**

Search within `async-opcua-server/src/aggregates/engine.rs` for these functions:

```rust
fn simple_bounded_points
fn agg_start_bound
fn agg_end_bound
fn agg_delta_bounds
```

Read the full bodies before editing. Identify whether the mismatch is in boundary source selection, empty bound handling, or delta calculation.

- [ ] **Step 3: Apply the minimal fix**

Implement only the behavior needed for Task 3:

```rust
// StartBound: value at interval_start using Simple Bounding Values.
// EndBound: value at interval_end using Simple Bounding Values.
// DeltaBounds: EndBound - StartBound.
```

Do not change `agg_time_average`, `agg_total`, `agg_interpolative`, or interpolated-bound helpers in this task.

- [ ] **Step 4: Run the focused test**

Run:

```bash
cargo test -p async-opcua-server --test aggregates_tests part13_start_end_and_delta_bounds_use_simple_bounds -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Run the aggregate suite**

Run:

```bash
cargo test -p async-opcua-server --test aggregates_tests
```

Expected: PASS.

- [ ] **Step 6: Report evidence to the planner**

Return:

```text
Task 4 result:
- Root cause:
- Minimal fix:
- Test commands:
- PASS/FAIL:
- Files changed:
```

Do not commit unless the planner asks for a commit.

---

## Task 5: Planner Review, Integration Gate, and Commit

**Assigned to:** main session, not `software-engineer-zai`

**Purpose:** Review the coding-agent changes, run integration checks, and commit only reviewed work.

- [ ] **Step 1: Inspect changed files**

Run:

```bash
git add -N docs/superpowers/plans/2026-07-13-part13-gauntlet-response-and-oracle.md
git status --short
git diff -- docs/superpowers/plans/2026-07-13-part13-gauntlet-response-and-oracle.md async-opcua/tests/integration/read.rs async-opcua-server/tests/aggregates_tests.rs async-opcua-server/src/aggregates/engine.rs async-opcua-server/src/aggregates/middleware.rs async-opcua-server/src/history/backend.rs async-opcua-history-sqlite/src/backend.rs
```

Expected: only files assigned by completed tasks are modified.

- [ ] **Step 2: Verify no blocking primitives were added**

Run:

```bash
rg "Mutex|RwLock|Semaphore|thread::sleep|std::sync|parking_lot" async-opcua/tests/integration/read.rs async-opcua-server/tests/aggregates_tests.rs async-opcua-server/src/aggregates/engine.rs async-opcua-server/src/aggregates/middleware.rs async-opcua-server/src/history/backend.rs async-opcua-history-sqlite/src/backend.rs
```

Expected: no new lock/blocking additions beyond existing imports/usages.

- [ ] **Step 3: Run formatting**

Run:

```bash
cargo fmt --check
```

Expected: PASS.

- [ ] **Step 4: Run targeted tests**

Run:

```bash
cargo test -p async-opcua-server --test aggregates_tests
cargo test -p async-opcua --test integration_tests integration::read::history_read_processed_returns_history_data_values -- --nocapture
```

Expected: PASS for both commands.

- [ ] **Step 5: Run broader history/read tests if response path changed**

Run this if any production file outside tests changed:

```bash
cargo test -p async-opcua --test integration_tests integration::read -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit reviewed changes**

Commit message if only tests were added:

```bash
git add docs/superpowers/plans/2026-07-13-part13-gauntlet-response-and-oracle.md async-opcua/tests/integration/read.rs async-opcua-server/tests/aggregates_tests.rs
git commit -m "test(history): Cover processed aggregate DataValues" -m "Add Part 13 coverage for processed HistoryRead response shape and focused bounded aggregate behavior." -m "Refs #288" -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```

Commit message if production code was fixed:

```bash
git add docs/superpowers/plans/2026-07-13-part13-gauntlet-response-and-oracle.md async-opcua/tests/integration/read.rs async-opcua-server/tests/aggregates_tests.rs async-opcua-server/src/aggregates/engine.rs async-opcua-server/src/aggregates/middleware.rs async-opcua-server/src/history/backend.rs async-opcua-history-sqlite/src/backend.rs
git commit -m "fix(history): Preserve processed aggregate DataValues" -m "Ensure valid ReadProcessed aggregate reads return HistoryData with present DataValues and add focused Part 13 bounded aggregate coverage." -m "Refs #288" -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 6: PR Gate

**Assigned to:** main session, not `software-engineer-zai`

**Purpose:** Run repository-required CI gate before any PR.

- [ ] **Step 1: Run local CI playbook**

Run:

```bash
tools/ci-playbook.sh --ci
```

Expected: ends with CI success output.

- [ ] **Step 2: Push and open PR only after green local CI**

Use the repository PR workflow, targeting `occamsshavingkit/async-opcua:master`.

---

## Self-Review

- Spec coverage: The plan covers the Gauntlet Part 13 response-shape symptom and one bounded aggregate oracle probe. It intentionally does not claim to complete all Part 13 aggregate semantics.
- Placeholder scan: No `TBD`, `TODO`, or unspecified implementation steps remain.
- Type consistency: Test snippets use existing `DataValue`, `DateTime`, `Variant`, `AggregateInput`, `AggregateConfiguration`, and `dispatch_aggregate` names from current code.
- Scope check: Each coding task has a single deliverable and can be delegated independently. Broad TimeAverage/Total correction is deferred unless the oracle/probe exposes a localized defect.
