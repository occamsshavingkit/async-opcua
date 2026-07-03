#!/usr/bin/env bash
# T035 (feature 054): verify the feature lattice compiles (FR-006).
#
# Checks:
#   1. Each alias standalone: cargo check -p async-opcua --no-default-features --features <alias>
#   2. Each of the 15 gates individually disabled from the full surface
#      (--no-default-features + all-but-one).
#   3. No-default baseline.
#
# Sampling rationale: 2^15 = 32768 combinations is intractable. We check each
# gate individually (15 + 4 aliases + 1 baseline = 20 checks), which catches
# any gate that has a compile-time dependency on another gate that isn't
# expressed via Cargo requires-relations.
set -euo pipefail

crate="async-opcua"
server_crate="async-opcua-server"
fail=0

all_gates=(
    subscriptions
    subscriptions-standard
    events
    alarms
    method-call
    history
    history-aggregates
    query
    node-management
    diagnostics
    rbac
    gds
    fota
    programs
    lds
)

aliases=(nano micro embedded standard)

# --- 1. Alias standalone checks ---------------------------------------------
echo "=== Alias standalone checks ==="
for alias in "${aliases[@]}"; do
    echo -n "  $alias ... "
    if cargo check --locked -p "$crate" --no-default-features --features "$alias" >&/tmp/lattice-$alias.log 2>&1; then
        echo "OK"
    else
        echo "FAIL"
        fail=1
        cat /tmp/lattice-$alias.log
    fi
done

# --- 2. Each gate individually disabled from the full server surface --------
echo "=== Per-gate disable checks (all-but-one) ==="
# Build the full feature list for the server crate.
all_features_csv=$(IFS=, ; echo "${all_gates[*]}")
for gate in "${all_gates[@]}"; do
    # All gates except $gate
    others=()
    for g in "${all_gates[@]}"; do
        [ "$g" != "$gate" ] && others+=("$server_crate/$g")
    done
    others_csv=$(IFS=, ; echo "${others[*]}")
    echo -n "  without $gate ... "
    if cargo check --locked -p "$crate" --no-default-features --features "$others_csv" >&/tmp/lattice-no-$gate.log 2>&1; then
        echo "OK"
    else
        echo "FAIL"
        fail=1
        cat /tmp/lattice-no-$gate.log
    fi
done

# --- 3. No-default baseline ------------------------------------------------
echo "=== No-default baseline ==="
echo -n "  no-default ... "
if cargo check --locked -p "$crate" --no-default-features >&/tmp/lattice-no-default.log 2>&1; then
    echo "OK"
else
    echo "FAIL"
    fail=1
    cat /tmp/lattice-no-default.log
fi

# --- Summary ----------------------------------------------------------------
if [ "$fail" -ne 0 ]; then
    echo "FAIL: lattice check found compilation errors"
    exit 1
fi
echo "clean: all alias standalone, per-gate disable, and no-default checks pass"
