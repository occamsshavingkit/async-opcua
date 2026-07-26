#!/usr/bin/env bash
# Check that every Cargo feature declared as companion-{name} has a matching
# companion!("companion-{name}", ...) gate in src/companion/mod.rs, and vice versa.
#
# Usage:
#   tools/check-companion-features.sh

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
EXIT=0

# Extract companion features from standalone declarations and the aggregate
# `companion = [...]` feature in Cargo.toml.
cargo_features=$(
  perl -0777 -ne '
    my ($features) = /^\[features\]\s*(.*?)(?=^\[[^]]+\]\s*$|\z)/ms;
    next unless defined $features;
    while ($features =~ /^\s*(companion[-_][a-z0-9_]+)\s*=/mg) {
      print "$1\n";
    }
    while ($features =~ /^\s*companion\s*=\s*\[(.*?)\]/msg) {
      my $members = $1;
      while ($members =~ /"(companion[-_][a-z0-9_]+)"/g) {
        print "$1\n";
      }
    }
  ' "$ROOT/async-opcua-server/Cargo.toml" | sort -u
)

# Extract companion feature strings from companion!(...) macro invocations
code_features=$(
  perl -0777 -ne 'while (/companion!\s*\(\s*"(companion[-_][a-z0-9_]+)"/g) { print "$1\n" }' "$ROOT/async-opcua-server/src/companion/mod.rs" | sort -u
)

# Check: features in code but missing from Cargo
missing_in_cargo=$(comm -13 <(echo "$cargo_features") <(echo "$code_features"))
if [[ -n "$missing_in_cargo" ]]; then
  echo "FAIL: Features used in companion!() macro but missing from Cargo.toml:"
  echo "$missing_in_cargo" | sed 's/^/  /'
  EXIT=1
fi

# Check: features in Cargo but missing from code
missing_in_code=$(comm -23 <(echo "$cargo_features") <(echo "$code_features"))
if [[ -n "$missing_in_code" ]]; then
  echo "FAIL: Features declared in Cargo.toml but missing companion!() gate in mod.rs:"
  echo "$missing_in_code" | sed 's/^/  /'
  EXIT=1
fi

if [[ "$EXIT" -eq 0 ]]; then
  echo "PASS: All companion features match between Cargo.toml and src/companion/mod.rs"
fi

exit "$EXIT"
