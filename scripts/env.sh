#!/bin/sh
# Shared build settings. Source it: `. scripts/env.sh`.
# Follows kmux's scripts/env.sh (at kmux.rev), with Kanna's Ghostty: the fork.

# Kanna builds GhosttyKit from the Ghostty fork's external-I/O branch
# (kmux-external-io: upstream a806905, kmux's pin, plus the external-I/O
# patch), at this commit. Until the branch is pushed, the fork is the local
# clone KANNA_GHOSTTY_FORK (default ~/work/ghostty).
KANNA_GHOSTTY_FORK="${KANNA_GHOSTTY_FORK:-$HOME/work/ghostty}"
KANNA_GHOSTTY_BRANCH="kmux-external-io"
KANNA_GHOSTTY_COMMIT="0e701462880ce3f00f9f8d9ed21a047913c1aa38"

# Ghostty at that commit needs exactly this Zig (as kmux's pin does). Homebrew
# may have moved on, so if `zig` isn't it, the official release is downloaded
# into target/.
KANNA_ZIG_VERSION="0.16.0"
KANNA_ZIG_SHA256_aarch64="b23d70deaa879b5c2d486ed3316f7eaa53e84acf6fc9cc747de152450d401489"
KANNA_ZIG_SHA256_x86_64="0387557ed1877bc6a2e1802c8391953baddba76081876301c522f52977b52ba7"

# Prints the path of a Zig $KANNA_ZIG_VERSION: `zig` if it is that version,
# else target/zig-$KANNA_ZIG_VERSION, downloaded and checked on first use.
kanna_zig() {
  if command -v zig >/dev/null 2>&1 && [ "$(zig version)" = "$KANNA_ZIG_VERSION" ]; then
    command -v zig
    return
  fi
  arch="$(uname -m)"; [ "$arch" = arm64 ] && arch=aarch64
  dir="$1/target/zig-$KANNA_ZIG_VERSION"
  if [ ! -x "$dir/zig" ]; then
    name="zig-$arch-macos-$KANNA_ZIG_VERSION"
    eval "sum=\$KANNA_ZIG_SHA256_$arch"
    echo "Downloading Zig $KANNA_ZIG_VERSION ($(zig version 2>/dev/null || echo "no zig") on PATH)" >&2
    mkdir -p "$1/target"
    curl -fsSL "https://ziglang.org/download/$KANNA_ZIG_VERSION/$name.tar.xz" -o "$1/target/$name.tar.xz" || return 1
    echo "$sum  $1/target/$name.tar.xz" | shasum -a 256 -c - >/dev/null || { echo "Zig download checksum mismatch" >&2; return 1; }
    tar -xJf "$1/target/$name.tar.xz" -C "$1/target" && rm "$1/target/$name.tar.xz"
    mv "$1/target/$name" "$dir"
  fi
  echo "$dir/zig"
}

# Prints the Ghostty source tree: GHOSTTY_SOURCE_DIR if set (built in place,
# as it is), else target/ghostty-source, a clone of the fork at the pinned
# commit. The clone keeps the build's outputs out of the fork's checkout.
kanna_ghostty_source() {
  if [ -n "${GHOSTTY_SOURCE_DIR:-}" ]; then
    echo "$GHOSTTY_SOURCE_DIR"
    return
  fi
  dir="$1/target/ghostty-source"
  if [ ! -f "$dir/build.zig" ]; then
    mkdir -p "$1/target"
    git clone -q --no-checkout --branch "$KANNA_GHOSTTY_BRANCH" "$KANNA_GHOSTTY_FORK" "$dir" >&2 || return 1
  fi
  if [ "$(git -C "$dir" rev-parse HEAD 2>/dev/null)" != "$KANNA_GHOSTTY_COMMIT" ] || [ ! -f "$dir/build.zig" ]; then
    git -C "$dir" fetch -q origin "$KANNA_GHOSTTY_BRANCH" >&2 || return 1
    git -C "$dir" checkout -q --detach "$KANNA_GHOSTTY_COMMIT" >&2 || return 1
  fi
  echo "$dir"
}

kanna_ghostty_rev() {
  rev="$(git -C "$1" rev-parse HEAD 2>/dev/null)" || { echo unknown; return; }
  if [ -n "$(git -C "$1" status --porcelain 2>/dev/null)" ]; then rev="$rev-dirty"; fi
  echo "$rev"
}
