#!/bin/sh
KANNA_GHOSTTY_FORK="${KANNA_GHOSTTY_FORK:-https://github.com/jemdiggity/ghostty.git}"
KANNA_GHOSTTY_BRANCH="kanna4"
KANNA_GHOSTTY_COMMIT="b0126824af3f5be412615f03ed10e65fbf519ba2"

KANNA_ZIG_VERSION="0.16.0"
KANNA_ZIG_SHA256_aarch64="b23d70deaa879b5c2d486ed3316f7eaa53e84acf6fc9cc747de152450d401489"
KANNA_ZIG_SHA256_x86_64="0387557ed1877bc6a2e1802c8391953baddba76081876301c522f52977b52ba7"

kanna_macos_target() {
  cargo_target="$(sed -n 's/^MACOSX_DEPLOYMENT_TARGET *= *{ *value *= *"\([0-9.]*\)".*/\1/p' "$1/.cargo/config.toml" 2>/dev/null)"
  swift_target="$(sed -n -e 's/.*\.macOS(\.v\([0-9]*\)_\([0-9]*\)).*/\1.\2/p' -e 's/.*\.macOS(\.v\([0-9]*\)).*/\1.0/p' -e 's/.*\.macOS("\([0-9.]*\)").*/\1/p' "$1/apps/kanna/Package.swift")"
  if [ -z "$cargo_target" ] || [ "$cargo_target" != "$swift_target" ]; then
    echo "macOS deployment target: .cargo/config.toml has ${cargo_target:-none}, apps/kanna/Package.swift has ${swift_target:-none}; make them the same" >&2
    return 1
  fi
  echo "$cargo_target"
}

kanna_cargo_release_for() {
  stamp="$1/target/release/macos-deployment-target"
  [ "$(cat "$stamp" 2>/dev/null)" = "$2" ] && return
  if [ -d "$1/target/release" ]; then
    echo "==> target/release was built for macOS $(cat "$stamp" 2>/dev/null || echo "the default"), not $2: cleaning it" >&2
    cargo clean --manifest-path "$1/Cargo.toml" --release >&2 || return 1
  fi
  mkdir -p "$1/target/release" && echo "$2" > "$stamp"
}

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

# A worktree uses the main checkout's prebuilt Ghostty pieces (target/ghosttykit, target/ghostty-vt/HOST)
# instead of building its own, when the main checkout's were built from the pinned commit. It links them
# into its own target/, so nothing there is copied and a rebuild here replaces the link, never the original.
kanna_borrow_built() {
  here="$1/target/$2"
  [ -e "$here" ] && return
  common="$(git -C "$1" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)" || return 0
  main="$(dirname "$common")/target/$2"
  [ "$main" != "$here" ] && [ -d "$main" ] || return 0
  [ "$(cat "$main/ghostty.rev" 2>/dev/null)" = "$KANNA_GHOSTTY_COMMIT" ] || return 0
  mkdir -p "$(dirname "$here")" && ln -s "$main" "$here"
}
