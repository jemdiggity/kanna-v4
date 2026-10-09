#!/usr/bin/env bash
# Packages target/kmux.app and the kmux CLI into an UNSIGNED local DMG:
# target/dist/kmux-<version>-<commit>-unsigned.dmg
#
# UNSIGNED: the app is only ad-hoc signed (no Developer ID, not notarized).
# It is for testing on your own Mac. Gatekeeper blocks it on other Macs.
# Releases are signed and notarized: see docs/research/distribution.md.
#
# Builds the app and CLI first if they're missing; never changes them.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
app="$repo_root/target/kmux.app"
cli="$repo_root/target/release/kmux"

[[ -d "$app" ]] || "$repo_root/scripts/build-kmux.sh"
[[ -x "$cli" ]] || (cd "$repo_root" && cargo build --release -p kmux)

version="$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$app/Contents/Info.plist")"
commit="$(git -C "$repo_root" rev-parse --short=12 HEAD)"
git -C "$repo_root" diff --quiet HEAD 2>/dev/null || commit="$commit-dirty"
name="kmux-$version-$commit-unsigned"

out="$repo_root/target/dist"
stage="$out/$name"
dmg="$out/$name.dmg"
rm -rf "$stage" "$dmg"
mkdir -p "$stage"

# A copy of the app with the CLI inside, where a release puts it too.
ditto "$app" "$stage/kmux.app"
mkdir -p "$stage/kmux.app/Contents/Helpers"
cp "$cli" "$stage/kmux.app/Contents/Helpers/kmux"

# Ad-hoc signature (inside out) so the bundle verifies. Not a Developer ID.
codesign --force --sign - "$stage/kmux.app/Contents/Helpers/kmux"
codesign --force --sign - "$stage/kmux.app"
codesign --verify --strict "$stage/kmux.app"

ln -s /Applications "$stage/Applications"
cat > "$stage/UNSIGNED - READ ME.txt" <<TXT
kmux $version ($commit) - UNSIGNED LOCAL BUILD

This build is not signed with a Developer ID and not notarized.
It is for testing on the Mac that built it.

Install: drag kmux.app to Applications.
CLI: ln -s /Applications/kmux.app/Contents/Helpers/kmux /usr/local/bin/kmux
     (or any directory on your PATH)

On another Mac, Gatekeeper refuses it. To run it anyway, after copying:
  xattr -dr com.apple.quarantine /Applications/kmux.app
TXT

hdiutil create -quiet -volname "kmux $version (unsigned)" -srcfolder "$stage" -fs HFS+ -format UDZO -ov "$dmg"
rm -rf "$stage"
echo "Built $dmg (UNSIGNED)"
