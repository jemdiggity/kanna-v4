#!/usr/bin/env bash
# Builds target/Kanna.app from apps/kanna, against GhosttyKit from the Ghostty
# fork (built first if missing: scripts/build-ghosttykit.sh). As kmux's
# scripts/build-kmux.sh. Set KANNA_CONFIGURATION=debug for a debug build.
#
# For now its bundle ID is dev.kanna.app.dev, so it never meets the Kanna.app
# that scripts/build-kanna.sh builds from kmux (dev.kanna.kanna). It isn't
# registered with Launch Services; start it by path.
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

configuration="${KANNA_CONFIGURATION:-release}"
bundle_id="${KANNA_BUNDLE_ID:-dev.kanna.app.dev}"
build=(swift build --package-path "$repo_root/apps/kanna" --configuration "$configuration" --scratch-path "$repo_root/target/kanna-build")
"${build[@]}"
product_dir="$("${build[@]}" --show-bin-path)"

app="$repo_root/target/Kanna.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$product_dir/Kanna" "$app/Contents/MacOS/Kanna"
cp "$repo_root/assets/Kanna.icns" "$app/Contents/Resources/Kanna.icns"
cp "$repo_root/LICENSE" "$app/Contents/Resources/"

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
