#!/bin/sh
# Shared build settings. Source it: `. scripts/env.sh`.

# kmux builds GhosttyKit from upstream Ghostty at this commit.
KMUX_GHOSTTY_REPO="https://github.com/ghostty-org/ghostty.git"
KMUX_GHOSTTY_COMMIT="a806905ea1e3b1564b7cc4ab1c54084939ca59b3"


# Ghostty at that commit needs exactly this Zig. Homebrew may have moved on,
# so if `zig` isn't it, the official release is downloaded into target/.
KMUX_ZIG_VERSION="0.16.0"
KMUX_ZIG_SHA256_aarch64="b23d70deaa879b5c2d486ed3316f7eaa53e84acf6fc9cc747de152450d401489"
KMUX_ZIG_SHA256_x86_64="0387557ed1877bc6a2e1802c8391953baddba76081876301c522f52977b52ba7"

# Prints the path of a Zig $KMUX_ZIG_VERSION: `zig` if it is that version,
# else target/zig-$KMUX_ZIG_VERSION, downloaded and checked on first use.
kmux_zig() {
  if command -v zig >/dev/null 2>&1 && [ "$(zig version)" = "$KMUX_ZIG_VERSION" ]; then
    command -v zig
    return
  fi
  arch="$(uname -m)"; [ "$arch" = arm64 ] && arch=aarch64
  dir="$1/target/zig-$KMUX_ZIG_VERSION"
  if [ ! -x "$dir/zig" ]; then
    name="zig-$arch-macos-$KMUX_ZIG_VERSION"
    eval "sum=\$KMUX_ZIG_SHA256_$arch"
    echo "Downloading Zig $KMUX_ZIG_VERSION ($(zig version 2>/dev/null || echo "no zig") on PATH)" >&2
    mkdir -p "$1/target"
    curl -fsSL "https://ziglang.org/download/$KMUX_ZIG_VERSION/$name.tar.xz" -o "$1/target/$name.tar.xz" || return 1
    echo "$sum  $1/target/$name.tar.xz" | shasum -a 256 -c - >/dev/null || { echo "Zig download checksum mismatch" >&2; return 1; }
    tar -xJf "$1/target/$name.tar.xz" -C "$1/target" && rm "$1/target/$name.tar.xz"
    mv "$1/target/$name" "$dir"
  fi
  echo "$dir/zig"
}

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
