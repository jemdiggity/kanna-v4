#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
crate="$repo_root/crates/kanna-merman"
header="$crate/include/kanna_merman.h"
out="$repo_root/target/merman"
xcframework="$out/KannaMerman.xcframework"
library="$repo_root/target/kanna-merman/release/libkanna_merman.a"

CARGO_TARGET_DIR="$repo_root/target/kanna-merman" cargo build --release --locked --manifest-path "$crate/Cargo.toml"

if [[ -f "$xcframework/Info.plist" && "$xcframework" -nt "$library" && "$xcframework" -nt "$header" && "$xcframework" -nt "${BASH_SOURCE[0]}" ]]; then
  echo "Up to date: $xcframework"
  exit 0
fi
rm -rf "$xcframework" "$out/headers"
mkdir -p "$out/headers/KannaMerman"
cp "$header" "$out/headers/KannaMerman/"
cat > "$out/headers/KannaMerman/module.modulemap" <<'MAP'
module KannaMerman {
    header "kanna_merman.h"
    export *
}
MAP
xcodebuild -create-xcframework -library "$library" -headers "$out/headers" -output "$xcframework" >/dev/null
echo "Built $xcframework"
