#!/usr/bin/env bash
# T034 (feature 054): build and measure each profile benchmark binary.
#
# Builds each of the six matrix packages in an ISOLATED cargo invocation
# (--locked --profile embedded), emits bytes + MiB + markdown rows.
# Used identically by docs and CI (research.md R10 caveat is a hard rule).
#
# Usage: tools/footprint.sh [--json]
#
# One package per invocation is mandatory: a workspace-wide build unifies
# features across all samples, invalidating the per-profile surface.
set -euo pipefail

profile="embedded"
declare -a packages=(
    "async-opcua-foundation-profile-nano-server"
    "async-opcua-foundation-profile-micro-server"
    "async-opcua-foundation-profile-embedded-server"
    "async-opcua-foundation-profile-standard-server"
    "async-opcua-minimal-server"
    "async-opcua-simple-server"
)

if [ "${1:-}" = "--json" ]; then
    echo "{"
    echo '  "profile": "'"$profile"'",'
    echo '  "packages": ['
fi

first=1
for pkg in "${packages[@]}"; do
    # Each package built in isolation (R10 hard rule).
    cargo build --locked --profile "$profile" -p "$pkg" >&2

    binary="target/$profile/$pkg"
    if [ ! -f "$binary" ]; then
        echo "warning: $binary not found, skipping" >&2
        continue
    fi

    bytes=$(stat -c %s "$binary")
    mib=$(awk "BEGIN { printf \"%.2f\", $bytes / 1048576 }")

    if [ "${1:-}" = "--json" ]; then
        [ $first -eq 0 ] && echo ","
        first=0
        printf '    {"package": "%s", "bytes": %s, "mib": %s}' "$pkg" "$bytes" "$mib"
    else
        printf "| %s | %s | %s MiB |\n" "$pkg" "$bytes" "$mib"
    fi
done

if [ "${1:-}" = "--json" ]; then
    echo ""
    echo "  ]"
    echo "}"
fi
