# Implementation Plan: Bundle Companion Specifications

**Branch**: `069-companion-specs` | **Date**: 2026-07-07

## Summary

Download companion spec NodeSet XMLs from `OPCFoundation/UA-Nodeset`, run them through the existing `async-opcua-codegen` pipeline, and register the generated types behind Cargo feature flags in `async-opcua-server`.

## Technical Context

**Language/Version**: Rust (edition 2021)
**Primary Dependencies**: Existing codegen tool (`async-opcua-codegen`)
**Source**: `https://github.com/OPCFoundation/UA-Nodeset`

## Strategy

1. Clone `UA-Nodeset` as a submodule into `schemas/companion/`
2. Create a companion codegen config that generates one module per spec
3. Run codegen to generate Rust types + node registrations
4. Add Cargo feature flags for each companion spec
5. Register generated namespaces in `async-opcua-server`
