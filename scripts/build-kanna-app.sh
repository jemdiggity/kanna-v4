#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
. "$repo_root/scripts/env.sh"
case "${1:-${KANNA_CHANNEL:-dev}}" in
  production|staging|dev) export KANNA_CHANNEL="${1:-${KANNA_CHANNEL:-dev}}" ;;
  *) echo "usage: scripts/build-kanna-app.sh [production|staging|dev]   (default: dev, or \$KANNA_CHANNEL)" >&2; exit 2 ;;
esac
macos_target="$(kanna_macos_target "$repo_root")"
host="$(rustc -vV | sed -n 's/^host: //p')"
kanna_borrow_built "$repo_root" ghosttykit
kanna_borrow_built "$repo_root" "ghostty-vt/$host"
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

if [[ -z "${KANNA_GHOSTTY_VT_DIR:-}" ]]; then
  ghostty_vt="$repo_root/target/ghostty-vt/$host"
  vt_built="$(cat "$ghostty_vt/ghostty.rev" 2>/dev/null || true)"
  if [[ -n "${GHOSTTY_SOURCE_DIR:-}" || ! -f "$ghostty_vt/lib/libghostty-vt.a" || "$vt_built" != "$KANNA_GHOSTTY_COMMIT" ]]; then
    "$repo_root/scripts/build-ghostty-vt.sh" "$host"
  fi
fi

kanna_borrow_cargo "$repo_root"
kanna_cargo_release_for "$repo_root" "$macos_target"
(cd "$repo_root" && cargo build --release -p kanna-ffi -p kanna-daemon -p kanna-keeper -p kanna)

if [[ "$KANNA_CHANNEL" == dev ]]; then configuration="${KANNA_CONFIGURATION:-debug}"; else configuration="${KANNA_CONFIGURATION:-release}"; fi
while IFS='=' read -r key value; do
  case "$key" in
    channel) channel="$value" ;; name) app_name="$value" ;; bundle_id) bundle_id="$value" ;; instance) instance="$value" ;; folder) folder="$value" ;;
  esac
done < <("$repo_root/target/release/kanna-daemon" --build)
kanna_borrow_swiftpm "$repo_root"
build=(swift build --package-path "$repo_root/apps/kanna" --configuration "$configuration" --scratch-path "$repo_root/target/kanna-build")
product_dir="$("${build[@]}" --show-bin-path)"
[[ "$repo_root/target/release/libkanna_ffi.a" -nt "$product_dir/Kanna" ]] && rm -f "$product_dir/Kanna"
"${build[@]}"

app="$repo_root/target/Kanna.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Helpers" "$app/Contents/Resources"
cp "$product_dir/Kanna" "$app/Contents/MacOS/Kanna"
cp "$repo_root/target/release/kanna-daemon" "$repo_root/target/release/kanna-keeper" "$repo_root/target/release/kanna" "$app/Contents/Helpers/"
cp "$repo_root/assets/Kanna.icns" "$app/Contents/Resources/Kanna.icns"
cp "$repo_root/LICENSE" "$app/Contents/Resources/"
install -m 755 "$repo_root/apps/kanna/Resources/planner.sh" "$app/Contents/Resources/"
mkdir -p "$app/Contents/Resources/kanna-skills"
for skill in kanna-tasks kanna-planning kanna-new-project kanna-markup kanna-explain kanna-quiz; do cp -RL "$repo_root/.agents/kanna-skills/skills/$skill" "$app/Contents/Resources/kanna-skills/"; done

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>Kanna</string>
<key>CFBundleIconFile</key><string>Kanna</string>
<key>CFBundleIdentifier</key><string>$bundle_id</string>
<key>CFBundleName</key><string>$app_name</string>
<key>CFBundleDisplayName</key><string>$app_name</string>
<key>KannaChannel</key><string>$channel</string>
<key>KannaInstance</key><string>$instance</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundleVersion</key><string>0.1.0</string>
<key>LSMinimumSystemVersion</key><string>$macos_target</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
echo "Built $app: $app_name ($bundle_id), $channel channel, instance $instance in $folder"
