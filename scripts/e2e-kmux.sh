#!/usr/bin/env bash
# End-to-end check: starts target/kmux.app on a private socket, drives it with
# target/release/kanna, and checks the panes are really drawn (window pixels).
# Build first: scripts/build-kmux.sh && cargo build --release
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out="$repo_root/target/e2e"
mkdir -p "$out"
export KMUX_SOCKET="/tmp/kmux-e2e-$$.sock"
kanna="$repo_root/target/release/kanna"

"$repo_root/target/kmux.app/Contents/MacOS/kmux" >"$out/kmux.log" 2>&1 &
kmux_pid=$!
trap 'kill $kmux_pid 2>/dev/null || true; rm -f "$KMUX_SOCKET"' EXIT

fail() { echo "FAIL: $*" >&2; exit 1; }
for _ in $(seq 50); do [[ -S "$KMUX_SOCKET" ]] && break; sleep 0.1; done
[[ -S "$KMUX_SOCKET" ]] || fail "kmux did not open $KMUX_SOCKET"

raw() { # one control-protocol request, printed as JSON
  python3 -c 'import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1])
s.sendall(sys.argv[2].encode() + b"\n"); print(s.makefile().readline().strip())' "$KMUX_SOCKET" "$1"
}
field() { python3 -c 'import json, sys; v = json.load(sys.stdin)
for k in sys.argv[1].split("."): v = v[int(k)] if k.isdigit() else v[k]
print(json.dumps(v) if isinstance(v, (dict, list)) else v)' "$1"; }

# 1. A command in a new pane, a third of the window wide.
[[ "$("$kanna" open term --name e2e --split right --size 1/3 --cmd "printf 'KMUX E2E OK\n'; exec sleep 600")" == e2e ]] || fail "open did not print the pane name"
list="$("$kanna" list --json)"
[[ "$(field windows.0.tabs.0.layout <<<"$list")" == '{"children": [{"pane": "p1", "size": "2/3"}, {"pane": "e2e", "size": "1/3"}], "split": "row"}' ]] \
  || fail "layout: $(field windows.0.tabs.0.layout <<<"$list")"
sleep 1.5
[[ "$(field panes.1.state <<<"$("$kanna" list --json)")" == running ]] || fail "e2e is not running"

# 2. Both panes are drawn: the shell prompt and the command's output have ink.
snapshot="$(raw "{\"id\":1,\"cmd\":\"debug.snapshot\",\"args\":{\"path\":\"$out/two-panes.png\"}}")"
python3 - "$snapshot" <<'PY' || fail "panes not drawn: $snapshot"
import json, sys
panes = json.loads(sys.argv[1])["panes"]
assert len(panes) == 2 and all(p["ink"] > 0.001 for p in panes), panes
PY

# 3. A command that ends leaves its pane on screen, marked exited.
"$kanna" open term --name done --split down --cmd "echo bye" >/dev/null
sleep 1.5
[[ "$(field panes.2.state <<<"$("$kanna" list --json)")" == exited ]] || fail "done did not exit"

# 4. Errors map to exit codes.
set +e
"$kanna" close nope 2>/dev/null; [[ $? == 4 ]] || fail "unknown pane should exit 4"
"$kanna" open web 2>/dev/null; [[ $? == 5 ]] || fail "web panes should exit 5 for now"
"$kanna" open term --name e2e 2>/dev/null; [[ $? == 1 ]] || fail "a taken name should exit 1"
set -e

# 5. Closing panes redistributes space; closing the last closes the window.
"$kanna" close done
"$kanna" close e2e
[[ "$(field windows.0.tabs.0.layout <<<"$("$kanna" list --json)")" == '{"pane": "p1"}' ]] || fail "p1 should fill the window"
"$kanna" close p1
[[ "$(field windows <<<"$("$kanna" list --json)")" == '[]' ]] || fail "the window should close with its last pane"

echo "e2e OK (snapshot: $out/two-panes.png)"
