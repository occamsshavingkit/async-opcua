#!/usr/bin/env bash
# T005 (feature 054): verify that profile-excluded subsystems are ABSENT from a
# profile benchmark binary.
#
#   tools/check-profile-absence.sh <package> <deny-features> <deny-symbols>
#
#   <package>       cargo package to build/inspect (one isolated invocation)
#   <deny-features> comma list of async-opcua-server features that must NOT resolve
#   <deny-symbols>  comma list of demangled symbol substrings that must NOT appear
#
# Two independent guards:
#  1. Feature guard: `cargo tree -e features` for the package must not resolve any
#     denied async-opcua-server feature (catches manifest-level leaks).
#  2. Symbol guard: the binary, rebuilt with symbols kept (strip=false override into
#     a separate target dir), must not contain any denied symbol substring
#     (catches code-level leaks that survive feature resolution, e.g. an ungated
#     `use`/registration path).
#
# Symbol checks run against `cargo build` artifacts — dev-dependencies (used by the
# behavior tests) do not apply here, so test-graph feature unification cannot mask a
# leak (specs/054-profile-polish/tasks.md, test-graph unification caveat).
set -euo pipefail

if [ "$#" -ne 3 ]; then
    echo "usage: $0 <package> <deny-features-csv> <deny-symbols-csv>" >&2
    exit 2
fi

package="$1"
deny_features="$2"
deny_symbols="$3"
target_dir="target/profile-absence"
fail=0

# --- 1. feature guard -------------------------------------------------------
tree_output=$(cargo tree --locked -p "$package" -e features 2>/dev/null)
IFS=',' read -ra features <<<"$deny_features"
for feature in "${features[@]}"; do
    [ -z "$feature" ] && continue
    if grep -q "async-opcua-server feature \"$feature\"" <<<"$tree_output"; then
        echo "LEAK(feature): $package resolves async-opcua-server feature '$feature'" >&2
        fail=1
    fi
done

# --- 2. symbol guard --------------------------------------------------------
# Keep symbols: override the embedded profile's strip=true, in a separate target
# dir so the real (stripped) benchmark artifacts are not invalidated.
CARGO_PROFILE_EMBEDDED_STRIP=false \
    cargo build --locked --profile embedded --target-dir "$target_dir" -p "$package" >&2

binary="$target_dir/embedded/$package"
if [ ! -f "$binary" ]; then
    echo "error: built binary not found at $binary" >&2
    exit 2
fi

symbols=$(nm -C "$binary" 2>/dev/null || true)
IFS=',' read -ra sentinels <<<"$deny_symbols"
for sentinel in "${sentinels[@]}"; do
    [ -z "$sentinel" ] && continue
    count=$(grep -cF "$sentinel" <<<"$symbols" || true)
    if [ "$count" -ne 0 ]; then
        echo "LEAK(symbol): $package contains $count symbols matching '$sentinel'" >&2
        fail=1
    fi
done

if [ "$fail" -ne 0 ]; then
    echo "FAIL: profile-excluded subsystems leaked into $package" >&2
    exit 1
fi
echo "clean: $package has none of [$deny_features] / [$deny_symbols]"
