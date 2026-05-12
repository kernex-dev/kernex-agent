#!/usr/bin/env bash
# Verify that the provider list stays in sync across:
#   - the PROVIDERS const in src/runtime_glue.rs (source of truth at runtime)
#   - the provider table in README.md (what users read on crates.io)
#   - the --provider help text in src/cli.rs (what users see in `kx --help`)
#
# Bedrock is intentionally excluded: it is an optional Cargo feature and
# the README explicitly documents it as not part of the built-in count.
#
# Run from repo root: scripts/check-provider-table.sh
# Used by .github/workflows/ci.yml.

set -euo pipefail

cd "$(dirname "$0")/.."

# Extract provider names from the PROVIDERS const block. The const lives
# in src/runtime_glue.rs after the lib extraction; this script tries that
# file first and falls back to src/main.rs for older worktrees.
provider_source=""
for candidate in src/runtime_glue.rs src/main.rs; do
    if [ -f "$candidate" ] && grep -q '^pub const PROVIDERS: &\[ProviderSpec\] = &\[\|^const PROVIDERS: &\[ProviderSpec\] = &\[' "$candidate"; then
        provider_source="$candidate"
        break
    fi
done
if [ -z "$provider_source" ]; then
    echo "FAIL: could not locate the PROVIDERS const in src/runtime_glue.rs or src/main.rs" >&2
    exit 1
fi

# Uses a tmpfile so we stay compatible with bash 3.2 (no mapfile, no <(...) array reads).
const_names_file=$(mktemp)
trap 'rm -f "$const_names_file"' EXIT
awk '
    /^pub const PROVIDERS: &\[ProviderSpec\] = &\[|^const PROVIDERS: &\[ProviderSpec\] = &\[/ { in_const = 1; next }
    in_const && /^\];/                          { exit }
    in_const && /name:[[:space:]]*"[^"]+"/ {
        match($0, /"[^"]+"/)
        print substr($0, RSTART+1, RLENGTH-2)
    }
' "$provider_source" > "$const_names_file"

const_count=$(wc -l < "$const_names_file" | tr -d ' ')

if [ "$const_count" -eq 0 ]; then
    echo "FAIL: could not parse any provider names from $provider_source PROVIDERS const" >&2
    exit 1
fi

# Count provider rows in README.md (lines starting with "| `--provider ").
readme_count=$(grep -c '^| `--provider ' README.md || true)

if [ "$const_count" -ne "$readme_count" ]; then
    echo "FAIL: drift detected" >&2
    echo "  PROVIDERS const in $provider_source: $const_count entries" >&2
    echo "  Provider rows in README.md:          $readme_count rows" >&2
    echo "  Names in const:" >&2
    sed 's/^/    /' "$const_names_file" >&2
    exit 1
fi

# Every provider in the const must appear in the --provider help text
# in src/cli.rs (the comma-separated list on the doc comment line).
help_line=$(grep -m1 '/// AI provider to use' src/cli.rs || true)
if [ -z "$help_line" ]; then
    echo "FAIL: could not locate '--provider' help text in src/cli.rs" >&2
    exit 1
fi

missing_file=$(mktemp)
trap 'rm -f "$const_names_file" "$missing_file"' EXIT
while IFS= read -r name; do
    [ -z "$name" ] && continue
    if ! echo "$help_line" | grep -qw "$name"; then
        echo "$name (cli.rs help)" >> "$missing_file"
    fi
    if ! grep -q "^| \`--provider $name\`" README.md; then
        echo "$name (README row)" >> "$missing_file"
    fi
done < "$const_names_file"

if [ -s "$missing_file" ]; then
    echo "FAIL: providers missing from cli.rs help or README:" >&2
    sed 's/^/  - /' "$missing_file" >&2
    exit 1
fi

echo "OK: $const_count providers in sync (PROVIDERS const, README table, --provider help)"
