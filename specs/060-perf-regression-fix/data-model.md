# Data Model: Performance Regression Fix — Localhost Benchmark

This feature has no new persistent entities. All changes are optimization-level modifications to existing code:

- **US1 (Profiling)**: No code changes — profiling output is documentation
- **US2 (VIEW-03 Revert)**: Structural change to `BrowseNode` method layout (private implementation detail)
- **US3 (#[inline] Annotations)**: Compiler hint annotations on existing functions
- **US4 (Release Profile)**: Cargo.toml configuration values

No data model changes, no new types, no state transitions.
