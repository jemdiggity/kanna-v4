#!/usr/bin/env bash
# Runs the shared protocol cases (tests/kmux-protocol/cases) against the
# reference model and the native core, so the two stay in sync.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
node "$repo_root/tests/kmux-protocol/run-model.mjs"
swift test --package-path "$repo_root/apps/kmux" --scratch-path "$repo_root/target/kmux-build" --filter ProtocolCasesTests 2>&1 \
  | grep -E "native:|skipped|✘|error|Test run"
