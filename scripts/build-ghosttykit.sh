#!/usr/bin/env bash
# Builds target/ghosttykit/GhosttyKit.xcframework from the Ghostty fork
# (external I/O), pinned in scripts/env.sh. As kmux's scripts/build-ghosttykit.sh.
# GHOSTTY_SOURCE_DIR builds another Ghostty tree instead, in place.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
. "$repo_root/scripts/env.sh"
source_dir="$(kanna_ghostty_source "$repo_root")"
zig="$(kanna_zig "$repo_root")"

# ReleaseSafe: fast under heavy output, and Zig's safety checks still turn
# bugs into clean crashes. Debug builds stall the main thread for seconds.
optimize="${KANNA_GHOSTTY_OPTIMIZE:-ReleaseSafe}"
(cd "$source_dir" && "$zig" build -Demit-xcframework=true -Demit-macos-app=false -Doptimize="$optimize")

out="$repo_root/target/ghosttykit"
xcframework="$out/GhosttyKit.xcframework"
mkdir -p "$out"
rm -rf "$xcframework"
cp -R "$source_dir/macos/GhosttyKit.xcframework" "$out/"
# Which Ghostty this is, so a stale framework is easy to spot.
git -C "$source_dir" rev-parse HEAD > "$out/ghostty.rev" 2>/dev/null || true

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
grep -q GHOSTTY_IO_MODE_EXTERNAL "$xcframework"/*/Headers/ghostty.h \
  || { echo "GhosttyKit has no external I/O: $source_dir isn't the fork's kmux-external-io" >&2; exit 1; }
echo "Built $xcframework"
