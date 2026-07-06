# Implementation Plan: Shrink Foundation Profile Footprints

**Branch**: `066-shrink-footprints` | **Date**: 2026-07-07

## Summary

Reduce binary sizes of nano (12 MB → <5 MB), micro (13 MB → <7 MB), and embedded (26 MB → <10 MB) foundation profile servers using Rust binary size optimization techniques: LTO, `opt-level = "s"/"z"`, `strip = true`, `codegen-units = 1`, and feature-gate analysis.

## Technical Context

**Language/Version**: Rust (edition 2021)
**Primary Dependencies**: tokio, aws-lc-rs, rustls, dashmap, parking_lot, serde
**Target Platform**: Linux x86_64 (CI measurement)
**Performance Goals**: <5/7/10 MB stripped binary sizes for nano/micro/embedded
**Constraints**: Must not regress OPC UA behavior or existing tests

## Key Optimization Strategies

1. **LTO + codegen-units**: `lto = true`, `codegen-units = 1` in release profile — typically 10-30% reduction
2. **opt-level = "s"**: Size-optimized code generation — 5-15% reduction
3. **strip = true**: Automatic symbol stripping — 30-50% of binary is debug symbols
4. **panic = "abort"**: Remove unwind tables — 5-10% reduction
5. **Feature gate crypto**: Use `aws-lc-rs` only for profiles that need it (nano may only need ring or no TLS)
6. **Dependency audit**: Check if `aws-lc-rs` or other large deps are pulled into nano/micro unnecessarily
7. **Remove unused code**: `cargo bloat` to identify large functions that shouldn't be in smaller profiles

## Project Structure

```text
Cargo.toml                    # + release profile optimizations (LTO, opt-level, strip)
async-opcua/Cargo.toml        # Check nano/micro feature deps (avoid pulling aws-lc-rs)
async-opcua-crypto/Cargo.toml # Feature-gate aws-lc-rs behind non-nano profiles
samples/foundation-profile-*/ # Update Cargo.toml with profile-specific optimizations
specs/066-shrink-footprints/  # Spec, plan, tasks
```
