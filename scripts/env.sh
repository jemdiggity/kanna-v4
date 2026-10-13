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

kanna_main_checkout() {
  common="$(git -C "$1" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)" || return 1
  main="$(dirname "$common")"
  [ "$(cd "$main" && pwd -P)" != "$(cd "$1" && pwd -P)" ] || return 1
  echo "$main"
}

kanna_borrow_cargo() {
  here="$1/target/release"
  [ -e "$here" ] && return
  main="$(kanna_main_checkout "$1")" || return 0
  [ -d "$main/target/release/.fingerprint" ] || return 0
  mkdir -p "$1/target"
  if cp -c -R -p "$main/target/release" "$here.borrowing" 2>/dev/null; then
    rm -rf "$here.borrowing"/.fingerprint/kanna-*
    mv "$here.borrowing" "$here"
    echo "Cloned $main/target/release's dependencies"
  else
    rm -rf "$here.borrowing"
  fi
}

kanna_borrow_swiftpm() {
  here="$1/target/kanna-build"
  [ -e "$here" ] && return
  main="$(kanna_main_checkout "$1")" || return 0
  from="$main/target/kanna-build"
  [ -f "$from/workspace-state.json" ] && [ -d "$from/checkouts" ] && [ -d "$from/repositories" ] || return 0
  mkdir -p "$here.borrowing"
  if cp -c -R -p "$from/repositories" "$from/checkouts" "$here.borrowing/" 2>/dev/null &&
     sed "s#$main/#$1/#g" "$from/workspace-state.json" > "$here.borrowing/workspace-state.json"; then
    mv "$here.borrowing" "$here"
    echo "Cloned $from's Swift packages"
  else
    rm -rf "$here.borrowing"
  fi
}
