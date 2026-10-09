#!/usr/bin/env bash
# Builds target/kmux.app. Set KMUX_CONFIGURATION=debug for a debug build.
#
# Products built on kmux can ship their own copy of the app under their own
# brand (Kanna builds target/Kanna.app this way):
#   KMUX_BRAND=kanna KMUX_BRAND_NAME=Kanna KMUX_BUNDLE_ID=dev.kanna.kanna KMUX_ICON=path/to.icns
# The brand names the app, its socket folder and files, and the variables its
# terminals get (KANNA_SOCKET, …), so it never meets a kmux the user runs.
# KMUX_TERMINAL_WRAPPER names a program every terminal runs through (a path
# inside the app's Contents, e.g. Helpers/kanna-keeper; see Brand.swift).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ ! -d "$repo_root/target/ghosttykit/GhosttyKit.xcframework" ]]; then
  "$repo_root/scripts/build-ghosttykit.sh"
fi

# merman (Mermaid layout for diagrams): cheap when it's already built.
"$repo_root/scripts/build-merman.sh"

configuration="${KMUX_CONFIGURATION:-release}"
brand="${KMUX_BRAND:-kmux}"
brand_name="${KMUX_BRAND_NAME:-$brand}"
bundle_id="${KMUX_BUNDLE_ID:-dev.kanna.$brand}"
# Icon source: apps/kmux/Icon/make-icon.swift (regenerate kmux.icns with it).
icon="${KMUX_ICON:-$repo_root/apps/kmux/Icon/kmux.icns}"
build=(swift build --package-path "$repo_root/apps/kmux" --configuration "$configuration" --scratch-path "$repo_root/target/kmux-build")
"${build[@]}"
product_dir="$("${build[@]}" --show-bin-path)"

app="$repo_root/target/$brand_name.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$product_dir/kmux" "$app/Contents/MacOS/$brand"
cp "$icon" "$app/Contents/Resources/$brand.icns"
cp "$repo_root/LICENSE" "$repo_root/NOTICE" "$app/Contents/Resources/"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>$brand</string>
<key>CFBundleIconFile</key><string>$brand</string>
<key>CFBundleIdentifier</key><string>$bundle_id</string>
<key>CFBundleName</key><string>$brand_name</string>
<key>KmuxBrand</key><string>$brand</string>
<key>KmuxTerminalWrapper</key><string>${KMUX_TERMINAL_WRAPPER:-}</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundleVersion</key><string>0.1.0</string>
<key>LSMinimumSystemVersion</key><string>15.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
echo "Built $app"
