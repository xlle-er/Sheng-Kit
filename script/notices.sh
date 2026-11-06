#!/usr/bin/env bash
# Regenerates THIRD_PARTY_NOTICES.md with cargo-about (configuration in about.toml/about.hbs).
# With --check, writes to a temporary file and fails if the committed file differs.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
if ! cargo about --version >/dev/null 2>&1; then
    echo "notices: cargo-about is not installed; install it with:" >&2
    echo "    cargo install --locked cargo-about" >&2
    exit 1
fi
if [[ "${1:-}" == "--check" ]]; then
    mkdir -p .build
    generated=".build/THIRD_PARTY_NOTICES.generated.md"
    cargo about generate --locked --workspace --fail --config about.toml --output-file "$generated" about.hbs
    if ! diff -u THIRD_PARTY_NOTICES.md "$generated"; then
        echo "notices: THIRD_PARTY_NOTICES.md is out of date; run script/notices.sh" >&2
        exit 1
    fi
    exit 0
fi
cargo about generate --locked --workspace --fail --config about.toml --output-file THIRD_PARTY_NOTICES.md about.hbs
