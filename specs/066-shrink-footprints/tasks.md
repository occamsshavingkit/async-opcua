# Tasks: Shrink Foundation Profile Footprints

**Input**: Design documents from `/specs/066-shrink-footprints/`
**Prerequisites**: plan.md, spec.md

## Format: `[ID] [P?] [Story] Description`

---

## Phase 1: Benchmark Baseline

- [ ] T001 Record current stripped binary sizes for nano, micro, embedded, standard profiles (release + strip, `ls -lh`)

---

## Phase 2: Release Profile Optimization (US1-3 shared)

**Goal**: Apply global Rust binary size optimization settings to all foundation profiles.

- [ ] T002 Add `[profile.release]` settings to workspace `Cargo.toml`: `opt-level = "s"`, `lto = true`, `codegen-units = 1`, `strip = true`, `panic = "abort"`
- [ ] T003 Build and test `cargo test --all-features` to verify no regressions from profile changes
- [ ] T004 Record new stripped binary sizes. Verify each profile is below its target. If not, proceed to Phase 3.

---

## Phase 3: Dependency Audit & Feature Gating (per-story)

**Goal**: Identify and remove unnecessary transitive dependencies from each profile.

- [ ] T005 [US1] Audit nano profile deps with `cargo bloat --release -p async-opcua-foundation-profile-nano-server --crates`. Check if `aws-lc-rs` (large crypto library) is pulled into nano. If so, feature-gate it to only load on profiles that need it.
- [ ] T006 [US2] Audit micro profile deps — same check as T005. Micro adds subscriptions; check if `moka` or `dashmap` size is disproportionate.
- [ ] T007 [US3] Audit embedded profile deps — the jump from 13 MB (micro) to 26 MB (embedded) suggests a large dep being pulled. Check `history`, `alarms`, `events` subsystems for size impact.
- [ ] T008 [P] [US1] Switch nano crypto backend from `aws-lc-rs` to `ring` if `aws-lc-rs` is the bloat cause. Ring is typically smaller.
- [ ] T009 [P] [US2] Add `default-features = false` to large deps in the facade crate's feature definitions to avoid pulling unused transitive deps.

---

## Phase 4: Post-Optimization Verification

- [ ] T010 Record final stripped binary sizes. Verify nano < 5 MB, micro < 7 MB, embedded < 10 MB.
- [ ] T011 Run existing profile smoke tests: `cargo test -p async-opcua-foundation-profile-nano-server --features profile-tests` (repeat for micro, embedded)
- [ ] T012 Run full test suite `cargo test --all-features`
- [ ] T013 Run `tools/ci-playbook.sh --ci` — footprint job must pass with new size thresholds
- [ ] T014 Update TODO.md

---

## Dependencies & Execution Order

- Phase 1 → Phase 2 → Phase 3 → Phase 4
- T005, T006, T007 are independent audits (different profiles)
- T008 depends on T005; T009 depends on T005/T006
