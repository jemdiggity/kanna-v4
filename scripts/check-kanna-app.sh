#!/usr/bin/env bash
# Checks Kanna.app (scripts/build-kanna-app.sh): starts target/Kanna.app in the
# background in echo mode (KANNA_ECHO=1: no daemon) on a private debug socket,
# types "echo hi" into its terminal through the real key path, and checks the stub backend echoed it, both as
# the terminal's text and as glyphs drawn in the window. Stops the app by PID.
# Never touches another running Kanna or kmux.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
app="$repo_root/target/Kanna.app/Contents/MacOS/Kanna"
[[ -x "$app" ]] || { echo "FAIL: build first: scripts/build-kanna-app.sh" >&2; exit 1; }
out="$repo_root/target/check-kanna-app"
mkdir -p "$out"
dir="$(mktemp -d "${TMPDIR:-/tmp}/kanna-check.XXXXXX")"
# Echo mode: no daemon; the app's one terminal echoes, on its own debug socket.
export KANNA_ECHO=1
export KANNA_APP_SOCKET="$dir/app.sock"
export KANNA_IGNORE_OCCLUSION=1 # draw even if the window is covered

"$app" --bg >"$out/app.log" 2>&1 &
app_pid=$!
stop() { # only the app started here, by its PID
  if [[ -n "$app_pid" ]]; then kill "$app_pid" 2>/dev/null || true; wait "$app_pid" 2>/dev/null || true; fi
  app_pid=""
}
trap 'stop; rm -rf "$dir"' EXIT

fail() { echo "FAIL: $*" >&2; echo "--- app log:" >&2; cat "$out/app.log" >&2; exit 1; }
raw() { # one debug request, printed as JSON
  python3 -c 'import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.settimeout(30); s.connect(sys.argv[1])
s.sendall(sys.argv[2].encode() + b"\n"); print(s.makefile().readline().strip())' "$KANNA_APP_SOCKET" "$1"
}
field() { python3 -c 'import json, sys; v = json.load(sys.stdin)
for k in sys.argv[1].split("."): v = v[int(k)] if k.isdigit() else v[k]
print(json.dumps(v) if isinstance(v, (dict, list)) else v)' "$1"; }

for _ in $(seq 100); do [[ -S "$KANNA_APP_SOCKET" ]] && break; kill -0 $app_pid 2>/dev/null || fail "the app exited"; sleep 0.1; done
[[ -S "$KANNA_APP_SOCKET" ]] || fail "the app did not open $KANNA_APP_SOCKET"

# 1. The app answers, and it's the one started here.
reply="$(raw '{"id":1,"cmd":"debug.app"}')"
[[ "$(field ok <<<"$reply")" == True ]] || fail "debug.app: $reply"
[[ "$(field pid <<<"$reply")" == "$app_pid" ]] || fail "debug.app answered from another process: $reply"

# 2. Typing echoes: the line, then the cursor on the next one.
reply="$(raw '{"id":2,"cmd":"debug.type","args":{"text":"echo hi\n"}}')"
[[ "$(field ok <<<"$reply")" == True ]] || fail "debug.type: $reply"
text=""
for _ in $(seq 50); do
  text="$(field text <<<"$(raw '{"id":3,"cmd":"debug.text"}')")"
  [[ "$(printf '%s\n' "$text" | head -1)" == "echo hi" ]] && break
  sleep 0.1
done
[[ "$(printf '%s\n' "$text" | head -1)" == "echo hi" ]] || fail "the terminal shows: $(printf '%q' "$text")"

# 3. And it's drawn: every glyph of the marker has ink in the window.
snapshot=""
for _ in $(seq 20); do
  snapshot="$(raw "{\"id\":4,\"cmd\":\"debug.snapshot\",\"args\":{\"path\":\"$out/echo.png\",\"marker\":\"echo hi\"}}")"
  python3 - "$snapshot" <<'PY' && break
import json, sys
marker = json.loads(sys.argv[1])["panes"][0].get("marker", {})
sys.exit(0 if marker.get("found") and marker["inked"] == marker["glyphs"] > 0 else 1)
PY
  sleep 0.2
done
python3 - "$snapshot" <<'PY' || fail "echo hi is not drawn: $snapshot"
import json, sys
marker = json.loads(sys.argv[1])["panes"][0].get("marker", {})
assert marker.get("found") and marker["inked"] == marker["glyphs"] > 0, marker
PY

# 4. It stops cleanly when asked, and takes its socket with it.
kill "$app_pid"
for _ in $(seq 50); do kill -0 "$app_pid" 2>/dev/null || break; sleep 0.1; done
kill -0 "$app_pid" 2>/dev/null && fail "the app did not stop on SIGTERM"
wait "$app_pid" 2>/dev/null || true
app_pid=""
[[ -e "$KANNA_APP_SOCKET" ]] && fail "the app left its socket behind"
echo "check-kanna-app OK ($out/echo.png)"
