#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
export CLANG_MODULE_CACHE_PATH="$ROOT/.build/module-cache"
export SWIFTPM_MODULECACHE_OVERRIDE="$CLANG_MODULE_CACHE_PATH"
export SHENG_ENGINE_PROFILE=debug
mkdir -p .build
# Provenance tooling is local to the spec workflow and not part of the published tree.
if [ -d script/provenance ]; then python3 -I -B -m unittest discover -s script/provenance -p 'test_*.py'; fi
for tool in deny about; do
    if ! cargo "$tool" --version >/dev/null 2>&1; then
        echo "check: cargo-$tool is not installed; install it with:" >&2
        echo "    cargo install --locked cargo-$tool" >&2
        exit 1
    fi
done
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --locked -p sheng-engine
cargo deny --locked check advisories bans licenses sources
script/notices.sh --check
swift test --scratch-path .build/native --disable-sandbox
