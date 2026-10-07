#!/usr/bin/env bash
# Builds target/ghosttykit/GhosttyKit.xcframework from the pinned Ghostty.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
. "$repo_root/scripts/env.sh"
source_dir="$(kmux_ghostty_source "$repo_root")"

# ReleaseSafe: fast under heavy output, and Zig's safety checks still turn
# bugs into clean crashes. Debug builds stall the main thread for seconds.
optimize="${KMUX_GHOSTTY_OPTIMIZE:-ReleaseSafe}"
(cd "$source_dir" && zig build -Demit-xcframework=true -Demit-macos-app=false -Doptimize="$optimize")

out="$repo_root/target/ghosttykit"
xcframework="$out/GhosttyKit.xcframework"
mkdir -p "$out"
rm -rf "$xcframework"
cp -R "$source_dir/macos/GhosttyKit.xcframework" "$out/"

# SwiftPM 6.3 skips xcframework static libraries whose names lack the "lib"
# prefix, which leaves every ghostty_* symbol undefined at link time.
plist="$xcframework/Info.plist"
i=0
while library=$(/usr/libexec/PlistBuddy -c "Print :AvailableLibraries:$i:LibraryPath" "$plist" 2>/dev/null); do
  identifier=$(/usr/libexec/PlistBuddy -c "Print :AvailableLibraries:$i:LibraryIdentifier" "$plist")
  if [[ "$library" != lib* ]]; then
    mv "$xcframework/$identifier/$library" "$xcframework/$identifier/lib$library"
    /usr/libexec/PlistBuddy -c "Set :AvailableLibraries:$i:LibraryPath lib$library" "$plist"
  fi
  i=$((i + 1))
done
echo "Built $xcframework"
