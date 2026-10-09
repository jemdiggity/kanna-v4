#!/usr/bin/env bash
# Mermaid renderer bake-off. Needs: target/bakeoff/mermaid-12.1.0.min.js,
# the merman-cli 0.8.0 release in target/bakeoff/merman-rel, mmdr and selkie
# (cargo install --locked --root target/bakeoff/tools mermaid-rs-renderer selkie-rs)
# and a headless Chromium (CHROME, default: Playwright's headless shell).
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
chrome="${CHROME:-$(ls -d ~/Library/Caches/ms-playwright/chromium_headless_shell-*/chrome-headless-shell-mac-arm64/chrome-headless-shell | tail -1)}"
python3 "$here/reference.py" "$chrome"
(cd "$here/bm-cli" && swift build -c release >/dev/null)
python3 "$here/bakeoff.py" render "$chrome"
python3 "$here/bakeoff.py" metrics
python3 "$here/bakeoff.py" composites "$chrome"
