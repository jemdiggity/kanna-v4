#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
. "$repo_root/scripts/env.sh"
ghosttykit="$repo_root/target/ghosttykit"
built="$(cat "$ghosttykit/ghostty.rev" 2>/dev/null || true)"
if [[ -n "${GHOSTTY_SOURCE_DIR:-}" || ! -d "$ghosttykit/GhosttyKit.xcframework" || "$built" != "$KANNA_GHOSTTY_COMMIT" ]]; then
  "$repo_root/scripts/build-ghosttykit.sh"
  built="$(cat "$ghosttykit/ghostty.rev" 2>/dev/null || true)"
fi
if [[ "$built" != "$KANNA_GHOSTTY_COMMIT" ]]; then
  if [[ -z "${GHOSTTY_SOURCE_DIR:-}" ]]; then
    echo "build-kanna-app: GhosttyKit is Ghostty ${built:-unknown}, but scripts/env.sh pins $KANNA_GHOSTTY_COMMIT" >&2
    exit 1
  fi
  echo "note: GhosttyKit is Ghostty $built from GHOSTTY_SOURCE_DIR, not the pinned $KANNA_GHOSTTY_COMMIT" >&2
fi

host="$(rustc -vV | sed -n 's/^host: //p')"
if [[ -z "${KANNA_GHOSTTY_VT_DIR:-}" ]]; then
  ghostty_vt="$repo_root/target/ghostty-vt/$host"
  vt_built="$(cat "$ghostty_vt/ghostty.rev" 2>/dev/null || true)"
  if [[ -n "${GHOSTTY_SOURCE_DIR:-}" || ! -f "$ghostty_vt/lib/libghostty-vt.a" || "$vt_built" != "$KANNA_GHOSTTY_COMMIT" ]]; then
    "$repo_root/scripts/build-ghostty-vt.sh" "$host"
  fi
fi

(cd "$repo_root" && cargo build --release -p kanna-layout -p kanna-daemon -p kanna-keeper)
"$repo_root/scripts/build-merman.sh"

configuration="${KANNA_CONFIGURATION:-release}"
bundle_id="${KANNA_BUNDLE_ID:-dev.kanna.app.dev}"
build=(swift build --package-path "$repo_root/apps/kanna" --configuration "$configuration" --scratch-path "$repo_root/target/kanna-build")
product_dir="$("${build[@]}" --show-bin-path)"
for library in "$repo_root"/target/release/*.a; do
  [[ "$library" -nt "$product_dir/Kanna" ]] && rm -f "$product_dir/Kanna"
done
"${build[@]}"

app="$repo_root/target/Kanna.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Helpers" "$app/Contents/Resources"
cp "$product_dir/Kanna" "$app/Contents/MacOS/Kanna"
cp "$repo_root/target/release/kanna-daemon" "$repo_root/target/release/kanna-keeper" "$app/Contents/Helpers/"
cp "$repo_root/assets/Kanna.icns" "$app/Contents/Resources/Kanna.icns"
cp "$repo_root/LICENSE" "$app/Contents/Resources/"
cp "$repo_root/apps/kanna/Resources/getting-started.md" "$app/Contents/Resources/"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>Kanna</string>
<key>CFBundleIconFile</key><string>Kanna</string>
<key>CFBundleIdentifier</key><string>$bundle_id</string>
<key>CFBundleName</key><string>Kanna</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundleVersion</key><string>0.1.0</string>
<key>LSMinimumSystemVersion</key><string>15.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
echo "Built $app"
