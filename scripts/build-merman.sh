#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
kmux="${KMUX_DIR:-$repo_root/../kmux}"
crate="$kmux/crates/kmux-merman"
[[ -f "$crate/Cargo.toml" ]] || { echo "build-merman: no kmux checkout with crates/kmux-merman at $kmux (set KMUX_DIR)" >&2; exit 1; }
out="$repo_root/target/merman"
xcframework="$out/KannaMerman.xcframework"
library="$repo_root/target/kanna-merman/release/libkmux_merman.a"

CARGO_TARGET_DIR="$repo_root/target/kanna-merman" cargo build --release --locked --manifest-path "$crate/Cargo.toml"

if [[ -f "$xcframework/Info.plist" && "$xcframework" -nt "$library" ]]; then
  echo "Up to date: $xcframework"
  exit 0
fi
rm -rf "$xcframework" "$out/headers"
mkdir -p "$out/headers"
cp "$crate/include/kmux_merman.h" "$out/headers/"
cat > "$out/headers/module.modulemap" <<'MAP'
module KannaMerman {
    header "kmux_merman.h"
    export *
}
MAP
xcodebuild -create-xcframework -library "$library" -headers "$out/headers" -output "$xcframework" >/dev/null
echo "Built $xcframework"
