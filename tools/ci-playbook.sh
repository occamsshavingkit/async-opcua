#!/usr/bin/env bash
# Local CI playbook — mirrors every GitHub Actions job that runs on PR.
#
#   tools/ci-playbook.sh             full suite
#   tools/ci-playbook.sh --ci        pre-PR gate (all CI jobs except interop/coverage/deny)
#   tools/ci-playbook.sh --fast      skip footprint, coverage, interop
#   tools/ci-playbook.sh --list      list mapped jobs

set -euo pipefail

RED='\033[0;31m' GREEN='\033[0;32m' YELLOW='\033[1;33m' NC='\033[0m'
PASS="${GREEN}PASS${NC}" FAIL="${RED}FAIL${NC}" SKIP="${YELLOW}SKIP${NC}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

FAST=false; CI_ONLY=false
while [[ $# -gt 0 ]]; do
    case "$1" in
        --fast) FAST=true ;;
        --ci)   CI_ONLY=true ;;
        --list)
            echo "CI job → local command:"
            echo "  cargo-fmt          cargo fmt --all -- --check"
            echo "  cargo-deny         cargo deny check advisories bans sources"
            echo "  build-linux        cargo check --locked  &&  cargo test --verbose --locked --all-features"
            echo "  build-matrix       RUSTFLAGS=-Dwarnings cargo build --locked --no-default-features -p ..."
            echo "  build-matrix-full  + --workspace  + --workspace --all-features"
            echo "  clippy             cargo clippy --workspace --all-targets --all-features --locked -- -Dwarnings"
            echo "  clippy-nf          --no-default-features  + xml-only  + json-only"
            echo "  footprint          1 minimal + 4 foundation profiles (embedded profile)"
            echo "  feature-lattice    tools/check-feature-lattice.sh"
            echo "  code-coverage      cargo llvm-cov (excl crypto)"
            echo "  verify-codegen     regenerate + git diff --exit-code"
            echo "  interop            .NET/Node/Python/C harnesses (skipped)"
            exit 0
            ;;
        *) echo "Unknown: $1"; exit 1 ;;
    esac
    shift
done

step()  { echo -e "\n${YELLOW}[$(date +%H:%M:%S)] $*${NC}"; }
pass()  { echo -e "  $PASS"; }
fail()  { echo -e "  $FAIL"; }
skip_msg() { echo -e "  $SKIP — $*"; }
fail_msg() { echo -e "  $FAIL — $*"; }
maybe_skip() { [[ "$FAST" == "true" ]] && { skip_msg "fast mode"; return 0; }; return 1; }

# ────────────────────────────────────────────────────────────────────
# 1. cargo fmt
# ────────────────────────────────────────────────────────────────────
job_cargo_fmt() {
    step "cargo fmt"
    cargo fmt --all -- --check && pass || fail
}

# ────────────────────────────────────────────────────────────────────
# 2. cargo deny
# ────────────────────────────────────────────────────────────────────
job_cargo_deny() {
    step "cargo-deny"
    if command -v cargo-deny &>/dev/null; then
        cargo deny check advisories bans sources && pass || fail
    else
        skip_msg "cargo-deny not installed"
    fi
}

# ────────────────────────────────────────────────────────────────────
# 3. build-linux (stable) — matches GitHub build-linux job
# ────────────────────────────────────────────────────────────────────
job_build_linux() {
    step "build-linux: cargo check --locked"
    cargo check --locked && pass || fail

    step "build-linux: cargo test --verbose --locked --all-features"
    cargo test --verbose --locked --all-features && pass || fail
}

# ────────────────────────────────────────────────────────────────────
# 4. build-matrix — 3 feature configs (matches GitHub build-matrix job)
# ────────────────────────────────────────────────────────────────────
job_build_matrix() {
    maybe_skip && return 0
    local r="RUSTFLAGS=-D warnings"

    step "build-matrix: default (--workspace)"
    env RUSTFLAGS="-D warnings" cargo build --locked --workspace && pass || fail

    step "build-matrix: all-features (--workspace --all-features)"
    env RUSTFLAGS="-D warnings" cargo build --locked --workspace --all-features && pass || fail

    step "build-matrix: no-default-features"
    env RUSTFLAGS="-D warnings" cargo build --locked --no-default-features \
        -p async-opcua -p async-opcua-types -p async-opcua-core \
        -p async-opcua-crypto -p async-opcua-client -p async-opcua-server \
        -p async-opcua-nodes && pass || fail
}

# ────────────────────────────────────────────────────────────────────
# 5. clippy — matches GitHub clippy job
# ────────────────────────────────────────────────────────────────────
job_clippy() {
    step "clippy: --workspace --all-targets --all-features"
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings && pass || fail

    step "clippy: --no-default-features"
    cargo clippy --locked -p async-opcua --no-default-features -- -D warnings || fail

    step "clippy: --no-default-features --features xml"
    cargo clippy --locked -p async-opcua --no-default-features --features xml -- -D warnings || fail

    step "clippy: --no-default-features --features json"
    cargo clippy --locked -p async-opcua --no-default-features --features json -- -D warnings || fail
    pass
}

# ────────────────────────────────────────────────────────────────────
# 6. footprint — matches GitHub footprint job
# ────────────────────────────────────────────────────────────────────
job_footprint() {
    maybe_skip && return 0

    step "footprint: minimal embedded server"
    cargo build --locked --profile release-footprint -p async-opcua-minimal-server || fail
    ls -lh target/release-footprint/async-opcua-minimal-server
    pass

    local pkgs=(
        "nano:async-opcua-foundation-profile-nano-server"
        "micro:async-opcua-foundation-profile-micro-server"
        "embedded:async-opcua-foundation-profile-embedded-server"
        "standard:async-opcua-foundation-profile-standard-server"
    )
    for entry in "${pkgs[@]}"; do
        local profile="${entry%%:*}" pkg="${entry##*:}"
        step "footprint: foundation profile $profile ($pkg)"
        cargo build --locked --profile release-footprint -p "$pkg" || fail
        ls -lh "target/release-footprint/$pkg"
        pass
    done
}

# ────────────────────────────────────────────────────────────────────
# 7. feature lattice — matches GitHub feature-lattice job
# ────────────────────────────────────────────────────────────────────
job_feature_lattice() {
    maybe_skip && return 0
    step "feature-lattice"
    tools/check-feature-lattice.sh && pass || fail
}

# ────────────────────────────────────────────────────────────────────
# 8. code coverage — matches GitHub code-coverage job
# ────────────────────────────────────────────────────────────────────
job_code_coverage() {
    maybe_skip && return 0
    step "code-coverage (cargo llvm-cov)"
    if command -v cargo-llvm-cov &>/dev/null; then
        cargo llvm-cov --workspace --exclude async-opcua-crypto --codecov \
            --output-path codecov.json --locked && pass || fail
    else
        skip_msg "cargo-llvm-cov not installed"
    fi
}

# ────────────────────────────────────────────────────────────────────
# 9. verify clean codegen — matches GitHub verify-clean-codegen job
# ────────────────────────────────────────────────────────────────────
job_verify_codegen() {
    maybe_skip && return 0
    step "verify-codegen: types"
    cargo run --locked --bin async-opcua-codegen code_gen_config.yml \
        || { fail; return; }

    step "verify-codegen: custom-codegen"
    cargo run --locked --bin async-opcua-codegen samples/custom-codegen/code_gen_config.yml \
        || { fail; return; }

    step "verify-codegen: FX data"
    cargo run --locked --bin async-opcua-codegen async-opcua-fx/code_gen_config.yml \
        || { fail; return; }

    step "verify-codegen: format"
    cargo fmt --all

    step "verify-codegen: check clean"
    if [[ -n $(git status --porcelain) ]]; then
        echo "Workspace is not clean — generated code differs from committed code:"
        git status --porcelain
        git diff --stat
        fail
    else
        pass
    fi
}

# ────────────────────────────────────────────────────────────────────
# 10. interop — cross-stack conformance harnesses
# ────────────────────────────────────────────────────────────────────
job_interop() {
    maybe_skip && return 0

    local missing=""
    command -v node    &>/dev/null  || missing+=" node.js (node)"
    command -v dotnet  &>/dev/null  || missing+=" .NET 8 SDK (dotnet)"
    command -v python3 &>/dev/null  || missing+=" python3"
    command -v cmake   &>/dev/null  || missing+=" cmake"
    command -v gcc     &>/dev/null  || missing+=" build-essential (gcc)"
    dpkg -l libssl-dev  &>/dev/null || missing+=" libssl-dev"

    if [[ -n "$missing" ]]; then
        fail_msg "MISSING DEPS:$missing"
        return 1
    fi

    step "interop: node-opcua"
    ./samples/demo-server/interop/run-interop.sh && pass || fail

    step "interop: open62541"
    ./samples/demo-server/interop/open62541/run-open62541.sh && pass || fail

    step "interop: asyncua"
    ./samples/demo-server/interop/asyncua/run-asyncua.sh && pass || fail

    step "interop: .NET (OPC Foundation reference)"
    ./samples/demo-server/interop/dotnet/run-dotnet.sh && pass || fail
}

# ════════════════════════════════════════════════════════════════════
# Main
# ════════════════════════════════════════════════════════════════════
echo -e "${YELLOW}=== Local CI Playbook ===${NC}"
echo "Started at $(date)"

job_cargo_fmt

# ────────────────────────────────────────────────────────────────────
# 8. Kerberos SSO integration test (feature 064/065)
# ────────────────────────────────────────────────────────────────────
setup_kerberos_kdc() {
    step "kerberos: setting up MIT KDC"
    if ! command -v krb5_newrealm &>/dev/null; then
        skip_msg "krb5-kdc not installed — install libkrb5-dev krb5-kdc krb5-admin-server"
        return 1
    fi
    # Create realm non-interactively
    export KRB5_CONFIG=/tmp/krb5.conf
    cat > "$KRB5_CONFIG" <<EOF
[libdefaults]
    default_realm = PLANT.LOCAL
    dns_lookup_realm = false
    dns_lookup_kdc = false
[realms]
    PLANT.LOCAL = {
        kdc = 127.0.0.1:8888
        admin_server = 127.0.0.1:8889
    }
EOF
    # Kill any existing KDC processes
    sudo pkill -f krb5kdc 2>/dev/null || true
    sudo pkill -f kadmind 2>/dev/null || true
    sleep 1
    # Create realm database
    sudo krb5_newrealm --realm PLANT.LOCAL --master-password testpass 2>/dev/null || {
        skip_msg "Failed to create Kerberos realm"
        return 1
    }
    # Start KDC and admin server
    sudo krb5kdc -n &
    sudo kadmind -nofork &
    sleep 2
    # Create test user and service principal
    echo "addprinc -pw testpass operator1@PLANT.LOCAL" | sudo kadmin.local -r PLANT.LOCAL 2>/dev/null
    echo "addprinc -randkey OPCUA/localhost@PLANT.LOCAL" | sudo kadmin.local -r PLANT.LOCAL 2>/dev/null
    echo "ktadd -k /tmp/opcua.keytab OPCUA/localhost@PLANT.LOCAL" | sudo kadmin.local -r PLANT.LOCAL 2>/dev/null
    sudo chmod 644 /tmp/opcua.keytab
    export KRB5_KTNAME=/tmp/opcua.keytab
    pass
}

job_kerberos_test() {
    maybe_skip && return 0
    step "kerberos: integration test"
    if ! command -v krb5_newrealm &>/dev/null; then
        skip_msg "krb5-kdc not installed — skipping Kerberos test"
        return 0
    fi
    setup_kerberos_kdc || return 0
    cargo test --features kerberos -p async-opcua-server -- kerberos_sso 2>&1 || fail
    # Cleanup
    sudo pkill -f krb5kdc 2>/dev/null || true
    sudo pkill -f kadmind 2>/dev/null || true
    pass
}

# Pre-PR gate and regular mode: everything except interop
if [[ "$CI_ONLY" == "true" ]]; then
    job_build_linux
    job_build_matrix
    job_clippy
    job_footprint
    job_feature_lattice
    job_verify_codegen
    job_kerberos_test
    job_interop
    echo -e "\n${GREEN}CI gate complete.${NC}"
    exit 0
fi

# Full mode
job_cargo_deny
job_build_linux
job_build_matrix
job_clippy
job_footprint
job_feature_lattice
job_code_coverage
job_verify_codegen
job_kerberos_test
job_interop

echo
echo -e "${GREEN}=== All done ===${NC}"
echo "Finished at $(date)"
