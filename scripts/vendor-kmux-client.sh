#!/usr/bin/env bash
# Copies kmux's control-socket client (crates/kmux-client, MIT) into
# vendor/kmux-client from the public kmux repo, at REF (default: main).
# Usage: scripts/vendor-kmux-client.sh [REF]
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
url="https://github.com/tampopogk/kmux.git"
ref="${1:-main}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

git clone -q --filter=blob:none --no-checkout "$url" "$work/kmux"
git -C "$work/kmux" checkout -q "$ref"
commit="$(git -C "$work/kmux" rev-parse HEAD)"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$work/kmux/Cargo.toml" | head -1)"
edition="$(sed -n 's/^edition = "\(.*\)"/\1/p' "$work/kmux/Cargo.toml" | head -1)"

dest="$repo_root/vendor/kmux-client"
rm -rf "$dest"
mkdir -p "$dest"
cp -R "$work/kmux/crates/kmux-client/src" "$dest/src"
cp "$work/kmux/LICENSE" "$dest/LICENSE"
# kmux's crates inherit version and edition from its workspace; vendored, they're spelled out.
sed -e "s/^version.workspace = true/version = \"$version\"/" -e "s/^edition.workspace = true/edition = \"$edition\"/" \
  "$work/kmux/crates/kmux-client/Cargo.toml" > "$dest/Cargo.toml"
cat > "$dest/VENDORED.md" <<MD
# Vendored: kmux-client

Copied from [kmux]($url) \`crates/kmux-client\` at commit \`$commit\`, MIT-licensed (see LICENSE).
Don't edit it here: change it in kmux, then run \`scripts/vendor-kmux-client.sh [REF]\`.
MD
echo "Vendored kmux-client at ${commit:0:7}"
