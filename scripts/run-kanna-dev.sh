#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: scripts/run-kanna-dev.sh

Starts the development Kanna next to any other Kanna or kmux: its daemon (and
keeper) on ~/Library/Application Support/kanna-dev/kanna.sock, then
target/Kanna.app (dev.kanna.app.dev). A daemon already answering there is kept.
Build first with cargo build --workspace --release and scripts/build-kanna-app.sh.
EOF
}
[[ $# -eq 0 ]] || { usage; [[ "$1" == -h || "$1" == --help ]] && exit 0; exit 2; }

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
bin="$repo_root/target/release"
app="$repo_root/target/Kanna.app"
dir="$HOME/Library/Application Support/kanna-dev"
socket="$dir/kanna.sock"

for f in "$bin/kanna-daemon" "$bin/kanna-keeper" "$app/Contents/MacOS/Kanna"; do
  [[ -x "$f" ]] || { echo "run-kanna-dev: $f isn't built" >&2; exit 1; }
done

answering() {
  python3 -c 'import socket,sys
s=socket.socket(socket.AF_UNIX); s.settimeout(1); s.connect(sys.argv[1])' "$socket" 2>/dev/null
}

mkdir -p "$dir"
if answering; then
  echo "run-kanna-dev: daemon already running on $socket"
else
  for name in $(compgen -e | grep -E '^(KANNA|KMUX)_' || true); do unset "$name"; done
  KANNA_SOCKET="$socket" KANNA_STATE_DIR="$dir" KANNA_KEEPER="$bin/kanna-keeper" \
    nohup "$bin/kanna-daemon" >>"$dir/daemon.log" 2>&1 </dev/null &
  disown
  for _ in $(seq 50); do answering && break; sleep 0.1; done
  answering || { echo "run-kanna-dev: the daemon didn't start; see $dir/daemon.log" >&2; exit 1; }
  echo "run-kanna-dev: daemon started (pid $!) on $socket"
fi

open -a "$app"
echo "run-kanna-dev: opened $app"
