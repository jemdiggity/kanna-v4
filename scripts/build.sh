#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
scripts="$repo_root/scripts"
case "${1:-${KANNA_CHANNEL:-dev}}" in
  production|staging|dev) export KANNA_CHANNEL="${1:-${KANNA_CHANNEL:-dev}}" ;;
  *) echo "usage: scripts/build.sh [production|staging|dev]   (default: dev, or \$KANNA_CHANNEL)" >&2; exit 2 ;;
esac
echo "==> Channel: $KANNA_CHANNEL"
. "$scripts/env.sh"
macos_target="$(kanna_macos_target "$repo_root")"
echo "==> macOS deployment target: $macos_target"
host="$(rustc -vV | sed -n 's/^host: //p')"
pinned() { [[ -z "${GHOSTTY_SOURCE_DIR:-}" && "$(cat "$1/ghostty.rev" 2>/dev/null)" == "$KANNA_GHOSTTY_COMMIT" ]]; }

echo "==> Ghostty: using the main checkout's pinned builds, if this is a worktree"
kanna_borrow_built "$repo_root" ghosttykit
kanna_borrow_built "$repo_root" "ghostty-vt/$host"

if pinned "$repo_root/target/ghostty-vt/$host"; then
  echo "==> libghostty-vt: already Ghostty $KANNA_GHOSTTY_COMMIT"
else
  echo "==> libghostty-vt (scripts/build-ghostty-vt.sh)"
  "$scripts/build-ghostty-vt.sh" "$host"
fi

if pinned "$repo_root/target/ghosttykit"; then
  echo "==> GhosttyKit: already Ghostty $KANNA_GHOSTTY_COMMIT"
else
  echo "==> GhosttyKit (scripts/build-ghosttykit.sh)"
  "$scripts/build-ghosttykit.sh"
fi

echo "==> kanna, kanna-daemon, kanna-keeper, libkanna_ffi (cargo build --workspace --release)"
kanna_borrow_cargo "$repo_root"
kanna_cargo_release_for "$repo_root" "$macos_target"
cargo build --manifest-path "$repo_root/Cargo.toml" --workspace --release

echo "==> Swift app and target/Kanna.app (scripts/build-kanna-app.sh)"
"$scripts/build-kanna-app.sh"
