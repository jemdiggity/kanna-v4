#!/bin/sh
# Shared build settings. Source it: `. scripts/env.sh`.

# kmux builds GhosttyKit from upstream Ghostty at this commit (needs Zig 0.16).
KMUX_GHOSTTY_REPO="https://github.com/ghostty-org/ghostty.git"
KMUX_GHOSTTY_COMMIT="a806905ea1e3b1564b7cc4ab1c54084939ca59b3"

# Prints the Ghostty source tree: GHOSTTY_SOURCE_DIR if set, else
# target/ghostty-source, cloned at the pinned commit on first use.
kmux_ghostty_source() {
  if [ -n "${GHOSTTY_SOURCE_DIR:-}" ]; then
    echo "$GHOSTTY_SOURCE_DIR"
    return
  fi
  dir="$1/target/ghostty-source"
  if [ ! -f "$dir/build.zig" ]; then
    mkdir -p "$1/target"
    git clone -q "$KMUX_GHOSTTY_REPO" "$dir" >&2 || return 1
  fi
  if [ "$(git -C "$dir" rev-parse HEAD)" != "$KMUX_GHOSTTY_COMMIT" ]; then
    git -C "$dir" checkout -q --detach "$KMUX_GHOSTTY_COMMIT" >&2 || return 1
  fi
  echo "$dir"
}
