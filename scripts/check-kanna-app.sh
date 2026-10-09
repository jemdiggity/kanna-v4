#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
app="$repo_root/target/Kanna.app/Contents/MacOS/Kanna"
[[ -x "$app" ]] || { echo "FAIL: build first: scripts/build-kanna-app.sh" >&2; exit 1; }
out="$repo_root/target/check-kanna-app"
mkdir -p "$out"
dir="$(mktemp -d "${TMPDIR:-/tmp}/kanna-check.XXXXXX")"
[[ -n "$dir" && -d "$dir" ]] || { echo "FAIL: mktemp gave no folder" >&2; exit 1; }
export KANNA_ECHO=1
export KANNA_DEBUG=1
export KANNA_STATE_DIR="$dir"
export KANNA_APP_SOCKET="$dir/app.sock"
export KANNA_IGNORE_OCCLUSION=1

"$app" --bg >"$out/app.log" 2>&1 &
app_pid=$!
stop() {
  if [[ -n "$app_pid" ]]; then kill "$app_pid" 2>/dev/null || true; wait "$app_pid" 2>/dev/null || true; fi
  app_pid=""
}
trap 'stop; rm -rf "$dir"' EXIT

fail() { echo "FAIL: $*" >&2; echo "--- app log:" >&2; cat "$out/app.log" >&2; exit 1; }
raw() {
  python3 -c 'import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.settimeout(30); s.connect(sys.argv[1])
s.sendall(sys.argv[2].encode() + b"\n"); print(s.makefile().readline().strip())' "$KANNA_APP_SOCKET" "$1"
}
field() { python3 -c 'import json, sys; v = json.load(sys.stdin)
for k in sys.argv[1].split("."): v = v[int(k)] if k.isdigit() else v[k]
print(json.dumps(v) if isinstance(v, (dict, list)) else v)' "$1"; }

for _ in $(seq 100); do [[ -S "$KANNA_APP_SOCKET" ]] && break; kill -0 $app_pid 2>/dev/null || fail "the app exited"; sleep 0.1; done
[[ -S "$KANNA_APP_SOCKET" ]] || fail "the app did not open $KANNA_APP_SOCKET"

reply="$(raw '{"id":1,"cmd":"debug.app"}')"
[[ "$(field ok <<<"$reply")" == True ]] || fail "debug.app: $reply"
[[ "$(field pid <<<"$reply")" == "$app_pid" ]] || fail "debug.app answered from another process: $reply"

reply="$(raw '{"id":2,"cmd":"debug.type","args":{"text":"echo hi\n"}}')"
[[ "$(field ok <<<"$reply")" == True ]] || fail "debug.type: $reply"
text=""
for _ in $(seq 50); do
  text="$(field text <<<"$(raw '{"id":3,"cmd":"debug.text"}')")"
  [[ "$(printf '%s\n' "$text" | head -1)" == "echo hi" ]] && break
  sleep 0.1
done
[[ "$(printf '%s\n' "$text" | head -1)" == "echo hi" ]] || fail "the terminal shows: $(printf '%q' "$text")"

drawn() {
  python3 - "$1" <<'PY'
import json, sys
marker = json.loads(sys.argv[1])["panes"][0].get("marker", {})
sys.exit(0 if marker.get("found") and marker["inked"] == marker["glyphs"] > 0 else 1)
PY
}
snapshot=""
for _ in $(seq 20); do
  snapshot="$(raw '{"id":4,"cmd":"debug.snapshot","args":{"marker":"echo hi"}}')"
  drawn "$snapshot" && break
  sleep 0.2
done
snapshot="$(raw '{"id":5,"cmd":"debug.snapshot","args":{"path":"echo.png","marker":"echo hi"}}')"
drawn "$snapshot" || fail "echo hi is not drawn: $snapshot"
[[ -f "$dir/echo.png" ]] || fail "debug.snapshot did not save echo.png in $dir: $snapshot"
cp "$dir/echo.png" "$out/echo.png"

for name in echo.png ../escape.png /tmp/escape.png; do
  reply="$(raw "{\"id\":6,\"cmd\":\"debug.snapshot\",\"args\":{\"path\":\"$name\"}}")"
  [[ "$(field ok <<<"$reply")" == False ]] || fail "debug.snapshot wrote $name: $reply"
done

kill "$app_pid"
for _ in $(seq 50); do kill -0 "$app_pid" 2>/dev/null || break; sleep 0.1; done
kill -0 "$app_pid" 2>/dev/null && fail "the app did not stop on SIGTERM"
wait "$app_pid" 2>/dev/null || true
app_pid=""
[[ -e "$KANNA_APP_SOCKET" ]] && fail "the app left its socket behind"
echo "check-kanna-app OK ($out/echo.png)"
