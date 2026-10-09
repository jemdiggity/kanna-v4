#!/usr/bin/env bash
# Builds Kanna: Kanna.app (Kanna's own copy of the kmux app, built from a kmux
# checkout beside this repo, ../kmux) and target/release/kanna (the command line).
# Kanna.app has its own sockets (~/Library/Application Support/kanna) and
# variables (KANNA_SOCKET, KANNA_PANE), so it never meets a kmux you run.
# KMUX_DIR overrides where kmux is checked out.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
kmux="${KMUX_DIR:-$repo_root/../kmux}"
if [[ ! -x "$kmux/scripts/build-kmux.sh" ]]; then
  echo "kmux isn't checked out at $kmux: git clone https://github.com/tampopogk/kmux.git \"$kmux\"" >&2
  exit 1
fi
kmux="$(cd "$kmux" && pwd)"

# kmux.rev is the kmux commit this kanna was last built and tested with.
pinned="$(cat "$repo_root/kmux.rev")"
actual="$(git -C "$kmux" rev-parse HEAD)"
if [[ "$actual" != "$pinned" ]]; then
  if [[ "${KANNA_ALLOW_KMUX_DRIFT:-}" != 1 ]]; then
    echo "build-kanna: $kmux is at ${actual:0:10}, kmux.rev says ${pinned:0:10}" >&2
    echo "      (git -C \"$kmux\" checkout $pinned, or KANNA_ALLOW_KMUX_DRIFT=1 and update kmux.rev once this build works)" >&2
    exit 1
  fi
  echo "note: $kmux is at ${actual:0:10}, kmux.rev says ${pinned:0:10} (KANNA_ALLOW_KMUX_DRIFT=1)" >&2
fi

KMUX_BRAND=kanna KMUX_BRAND_NAME=Kanna KMUX_BUNDLE_ID=dev.kanna.kanna KMUX_ICON="$repo_root/assets/Kanna.icns" \
  "$kmux/scripts/build-kmux.sh"
app="$kmux/target/Kanna.app"
# So `kanna` can start it by bundle ID before it has ever been opened.
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$app"

(cd "$repo_root" && cargo build --release -p kanna)
echo "Built $app and $repo_root/target/release/kanna"
