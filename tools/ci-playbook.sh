#!/usr/bin/env bash
# Local CI playbook — mirrors the GitHub Actions workflows as closely as
# practical without external infrastructure (no .NET, Node, Python interop).
#
# Usage:
#   ./tools/ci-playbook.sh            # full suite
#   ./tools/ci-playbook.sh --fast     # skip slow steps (coverage, footprint)
#   ./tools/ci-playbook.sh --ci       # CI-gate subset (fmt, clippy, check, tests)
#   ./tools/ci-playbook.sh --list     # list available steps
#
# Dependencies: cargo-deny, cargo-llvm-cov (optional), libelf-dev

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

PASS="${GREEN}PASS${NC}"
FAIL="${RED}FAIL${NC}"
SKIP="${YELLOW}SKIP${NC}"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# ---------------------------------------------------------------------------
# Parse arguments
# ---------------------------------------------------------------------------
FAST=false
CI_ONLY=false
while [[ $# -gt 0 ]]; do
    case "$1" in
        --fast)   FAST=true ;;
        --ci)     CI_ONLY=true ;;
        --list)
            echo "Steps:"
            echo "  cargo-fmt          cargo fmt --all -- --check"
            echo "  cargo-deny         cargo deny check advisories bans sources"
            echo "  cargo-clippy       clippy --workspace --all-targets --all-features"
            echo "  cargo-clippy-nf    clippy --no-default-features + xml-only + json-only"
            echo "  cargo-check        RUSTFLAGS=-Dwarnings cargo check --workspace"
            echo "  cargo-build-matrix  3 build configs: default, all-features, no-default-features"
            echo "  cargo-test         cargo test --locked --all-features"
            echo "  cargo-test-lib     cargo test -p async-opcua-{crypto,core,server,nodes} --lib"
            echo "  codegen-verify     regenerate codegen, verify clean"
            echo "  footprint          minimal server + foundation profiles + feature lattice"
            echo "  code-coverage      cargo llvm-cov (excludes crypto)"
            echo "  interop           .NET + Node.js + Python + C interop (skipped locally)"
            exit 0
            ;;
        *) echo "Unknown flag: $1"; exit 1 ;;
    esac
    shift
done

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
step()  { echo -e "\n${YELLOW}[$(date +%H:%M:%S)] $*${NC}"; }
pass()  { echo -e "  $PASS"; }
fail()  { echo -e "  $FAIL"; return 1; }
skip_msg() { echo -e "  $SKIP — $*"; }

# ---------------------------------------------------------------------------
# Steps
# ---------------------------------------------------------------------------

# 1. Formatting (always run)
run_cargo_fmt() {
    step "cargo fmt"
    cargo fmt --all -- --check && pass || fail
}

# 2. Dependency audit
run_cargo_deny() {
    [[ "$CI_ONLY" == "true" ]] && { skip_msg "ci-only mode"; return 0; }
    step "cargo deny"
    if command -v cargo-deny &>/dev/null; then
        cargo deny check advisories bans sources && pass || fail
    else
        skip_msg "cargo-deny not installed (cargo install cargo-deny)"
    fi
}

# 3. Clippy — full workspace
run_cargo_clippy() {
    step "clippy (workspace --all-targets --all-features)"
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings && pass || fail
}

# 3b. Clippy — no-default-features, xml-only, json-only
run_cargo_clippy_nf() {
    [[ "$CI_ONLY" == "true" ]] && { skip_msg "ci-only mode"; return 0; }
    step "clippy (--no-default-features)"
    cargo clippy --locked -p async-opcua --no-default-features -- -D warnings || fail
    step "clippy (--no-default-features --features xml)"
    cargo clippy --locked -p async-opcua --no-default-features --features xml -- -D warnings || fail
    step "clippy (--no-default-features --features json)"
    cargo clippy --locked -p async-opcua --no-default-features --features json -- -D warnings || fail
    pass
}

# 4. cargo check with -D warnings
run_cargo_check() {
    step "cargo check (-D warnings)"
    RUSTFLAGS="-D warnings" cargo check --workspace && pass || fail
}

# 5. Build matrix (3 feature configs)
run_cargo_build_matrix() {
    [[ "$CI_ONLY" == "true" ]] && { skip_msg "ci-only mode"; return 0; }
    step "build-matrix (default)"
    RUSTFLAGS="-D warnings" cargo build --locked --workspace && pass || fail
    step "build-matrix (all-features)"
    RUSTFLAGS="-D warnings" cargo build --locked --workspace --all-features && pass || fail
    step "build-matrix (no-default-features)"
    RUSTFLAGS="-D warnings" cargo build --locked --no-default-features \
        -p async-opcua -p async-opcua-types -p async-opcua-core \
        -p async-opcua-crypto -p async-opcua-client -p async-opcua-server \
        -p async-opcua-nodes && pass || fail
}

# 6. Full test suite (--all-features)
run_cargo_test() {
    [[ "$CI_ONLY" == "true" ]] && { skip_msg "ci-only mode"; return 0; }
    step "cargo test (locked --all-features)"
    # libelf-dev required for TSN feature
    cargo test --verbose --locked --all-features && pass || fail
}

# 6b. Library-only test suite (fast)
run_cargo_test_lib() {
    step "cargo test (-p ... --lib)"
    cargo test -p async-opcua-crypto --lib \
        && cargo test -p async-opcua-core --lib \
        && cargo test -p async-opcua-server --lib \
        && cargo test -p async-opcua-nodes --lib \
        && pass || fail
}

# 7. Codegen verification
run_codegen_verify() {
    [[ "$CI_ONLY" == "true" ]] && { skip_msg "ci-only mode"; return 0; }
    step "codegen verify — types"
    cargo run --locked --bin async-opcua-codegen code_gen_config.yml \
        && cargo fmt -- async-opcua-types/src/generated/ \
        && git diff --exit-code -- async-opcua-types/src/generated/ \
        && pass || fail

    step "codegen verify — custom-codegen sample"
    cargo run --locked --bin async-opcua-codegen samples/custom-codegen/code_gen_config.yml \
        && cargo fmt -- samples/custom-codegen/src/generated/ \
        && git diff --exit-code -- samples/custom-codegen/src/generated/ \
        && pass || fail

    step "codegen verify — FX data"
    cargo run --locked --bin async-opcua-codegen async-opcua-fx/code_gen_config.yml \
        && cargo fmt -- async-opcua-fx/src/generated/ \
        && git diff --exit-code -- async-opcua-fx/src/generated/ \
        && pass || fail
}

# 8. Footprint
run_footprint() {
    [[ "$FAST" == "true" ]] && { skip_msg "fast mode"; return 0; }
    [[ "$CI_ONLY" == "true" ]] && { skip_msg "ci-only mode"; return 0; }

    step "footprint — minimal embedded server"
    cargo build --locked --profile embedded -p async-opcua-minimal-server || fail
    ls -lh target/embedded/async-opcua-minimal-server
    pass

    step "footprint — foundation profiles (nano/micro/embedded/standard)"
    for profile in nano micro embedded standard; do
        pkg="async-opcua-foundation-profile-${profile}-server"
        echo "  Building $profile ($pkg)..."
        cargo build --locked --profile embedded -p "$pkg" || fail
        ls -lh "target/embedded/$pkg"
    done
    pass

    step "footprint — profile absence checks"
    tools/check-profile-absence.sh \
        async-opcua-foundation-profile-nano-server \
        "subscriptions,subscriptions-standard,events,alarms,method-call,history,history-aggregates,query,node-management,diagnostics,rbac,gds,fota,programs,lds" \
        "opcua_server::subscriptions::,opcua_server::alarms::,opcua_server::history::,opcua_server::gds::,opcua_server::rbac::role_management,opcua_server::rbac::defaults,opcua_server::programs::,opcua_server::fota::" \
        && pass || fail

    step "footprint — feature lattice"
    tools/check-feature-lattice.sh && pass || fail
}

# 9. Code coverage (requires cargo-llvm-cov)
run_code_coverage() {
    [[ "$FAST" == "true" ]] && { skip_msg "fast mode"; return 0; }
    [[ "$CI_ONLY" == "true" ]] && { skip_msg "ci-only mode"; return 0; }
    step "code coverage (cargo llvm-cov)"
    if command -v cargo-llvm-cov &>/dev/null; then
        cargo llvm-cov --workspace --exclude async-opcua-crypto --codecov \
            --output-path codecov.json --locked && pass || fail
    else
        skip_msg "cargo-llvm-cov not installed (cargo install cargo-llvm-cov)"
    fi
}

# 10. Interop (skipped — requires .NET, Node.js, Python, C toolchain)
run_interop() {
    step "interop harnesses"
    skip_msg "requires .NET SDK 8, Node.js, Python venv, open62541 build deps"
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
echo -e "${YELLOW}=== Local CI Playbook ===${NC}"
echo "Started at $(date)"
echo

# CI gate — always run these
run_cargo_fmt
run_cargo_check
run_cargo_clippy
run_cargo_clippy_nf
run_cargo_test_lib

[[ "$CI_ONLY" == "true" ]] && { echo -e "\n${GREEN}CI gate subset complete.${NC}"; exit 0; }

# Non-gate steps
run_cargo_deny
run_cargo_build_matrix
run_cargo_test
run_codegen_verify
run_footprint
run_code_coverage
run_interop

echo
echo -e "${GREEN}=== All done ===${NC}"
echo "Finished at $(date)"
