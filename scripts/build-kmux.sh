#!/usr/bin/env bash
# Builds target/kmux.app. Set KMUX_CONFIGURATION=debug for a debug build.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ ! -d "$repo_root/target/ghosttykit/GhosttyKit.xcframework" ]]; then
  "$repo_root/scripts/build-ghosttykit.sh"
fi

configuration="${KMUX_CONFIGURATION:-release}"
build=(swift build --package-path "$repo_root/apps/kmux" --configuration "$configuration" --scratch-path "$repo_root/target/kmux-build")
"${build[@]}"
product_dir="$("${build[@]}" --show-bin-path)"

app="$repo_root/target/kmux.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$product_dir/kmux" "$app/Contents/MacOS/kmux"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>kmux</string>
<key>CFBundleIdentifier</key><string>dev.kanna.kmux</string>
<key>CFBundleName</key><string>kmux</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundleVersion</key><string>0.1.0</string>
<key>LSMinimumSystemVersion</key><string>15.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
echo "Built $app"
