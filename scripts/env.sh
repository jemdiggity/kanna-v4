#!/bin/sh
KANNA_GHOSTTY_FORK="${KANNA_GHOSTTY_FORK:-https://github.com/jemdiggity/ghostty.git}"
KANNA_GHOSTTY_BRANCH="kanna4"
KANNA_GHOSTTY_COMMIT="b0126824af3f5be412615f03ed10e65fbf519ba2"

KANNA_ZIG_VERSION="0.16.0"
KANNA_ZIG_SHA256_aarch64="b23d70deaa879b5c2d486ed3316f7eaa53e84acf6fc9cc747de152450d401489"
KANNA_ZIG_SHA256_x86_64="0387557ed1877bc6a2e1802c8391953baddba76081876301c522f52977b52ba7"

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

kanna_ghostty_source() {
  if [ -n "${GHOSTTY_SOURCE_DIR:-}" ]; then
    echo "$GHOSTTY_SOURCE_DIR"
    return
  fi
  dir="$1/target/ghostty-source"
  if [ ! -f "$dir/build.zig" ]; then
    mkdir -p "$1/target"
    git clone -q --no-checkout --filter=blob:none --branch "$KANNA_GHOSTTY_BRANCH" "$KANNA_GHOSTTY_FORK" "$dir" >&2 || return 1
  fi
  if [ "$(git -C "$dir" rev-parse HEAD 2>/dev/null)" != "$KANNA_GHOSTTY_COMMIT" ] || [ ! -f "$dir/build.zig" ]; then
    git -C "$dir" remote set-url origin "$KANNA_GHOSTTY_FORK" >&2 || return 1
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
