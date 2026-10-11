#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
. "$repo_root/scripts/env.sh"
host="$(rustc -vV | sed -n 's/^host: //p')"
kanna_borrow_built "$repo_root" ghosttykit
kanna_borrow_built "$repo_root" "ghostty-vt/$host"

vt="$repo_root/target/ghostty-vt/$host"
if [[ -n "${GHOSTTY_SOURCE_DIR:-}" || ! -f "$vt/lib/libghostty-vt.a" || "$(cat "$vt/ghostty.rev" 2>/dev/null || true)" != "$KANNA_GHOSTTY_COMMIT" ]]; then
  "$repo_root/scripts/build-ghostty-vt.sh"
fi

(cd "$repo_root" && cargo build --workspace --release)
"$repo_root/scripts/build-kanna-app.sh"
