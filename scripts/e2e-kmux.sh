#!/usr/bin/env bash
# End-to-end check: starts target/kmux.app on a private socket, drives it with
# target/release/kanna, and checks the panes are really drawn (window pixels).
# Build first: scripts/build-kmux.sh && cargo build --release
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out="$repo_root/target/e2e"
mkdir -p "$out"
export KMUX_SOCKET="/tmp/kmux-e2e-$$.sock"
export KMUX_IGNORE_OCCLUSION=1 # render even if the window is covered
kanna="$repo_root/target/release/kanna"

"$repo_root/target/kmux.app/Contents/MacOS/kmux" >"$out/kmux.log" 2>&1 &
kmux_pid=$!
trap 'kill $kmux_pid 2>/dev/null || true; rm -f "$KMUX_SOCKET"' EXIT

fail() { echo "FAIL: $*" >&2; exit 1; }
for _ in $(seq 50); do [[ -S "$KMUX_SOCKET" ]] && break; sleep 0.1; done
[[ -S "$KMUX_SOCKET" ]] || fail "kmux did not open $KMUX_SOCKET"

# Every pane's marker text must be on screen with glyph pixels in each cell.
drawn() { # drawn NAME MARKER [COUNT]
  local snapshot
  snapshot="$(raw "{\"id\":1,\"cmd\":\"debug.snapshot\",\"args\":{\"path\":\"$out/$1.png\",\"marker\":\"$2\"}}")"
  python3 - "$snapshot" "${3:-0}" <<'PY' || fail "panes not drawn ($1): $snapshot"
import json, sys
panes = json.loads(sys.argv[1])["panes"]
assert int(sys.argv[2]) in (0, len(panes)), f"expected {sys.argv[2]} panes"
marked = [p for p in panes if p.get("marker", {}).get("found")]
assert marked and all(p["marker"]["inked"] == p["marker"]["glyphs"] > 0 for p in marked), panes
if sys.argv[2] != "0": assert len(marked) == len(panes), "every pane should show text"
PY
}
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

# 2. Both panes are drawn: the command's output, and the new shell's first line.
drawn two-panes "KMUX E2E OK"
drawn two-panes-shell "*" 2

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

# 6. Keyboard shortcuts, pressed through AppKit's key-event path.
key() { raw "{\"id\":1,\"cmd\":\"debug.key\",\"args\":{\"key\":\"$1\"}}" >/dev/null; sleep 0.4; }
state() { "$kanna" list --json; }
"$kanna" open term --name k1 >/dev/null
key "cmd+d"
key "cmd+shift+d"
layout="$(field windows.0.tabs.0.layout <<<"$(state)")"
[[ "$layout" == '{"children": [{"pane": "k1", "size": "1/2"}, {"children": [{"pane": "p5", "size": "1/2"}, {"pane": "p6", "size": "1/2"}], "size": "1/2", "split": "column"}], "split": "row"}' ]] \
  || fail "cmd+d / cmd+shift+d layout: $layout"
[[ "$(field windows.0.focused <<<"$(state)")" == p6 ]] || fail "the new pane should take focus"
key "cmd+]"; [[ "$(field windows.0.focused <<<"$(state)")" == k1 ]] || fail "cmd+] should wrap to k1"
key "cmd+["; [[ "$(field windows.0.focused <<<"$(state)")" == p6 ]] || fail "cmd+[ should wrap back to p6"
key "cmd+shift+return"; [[ "$(field windows.0.zoomed <<<"$(state)")" == p6 ]] || fail "cmd+shift+return should zoom"
key "cmd+["; [[ "$(field windows.0.zoomed <<<"$(state)")" == None ]] || fail "moving focus should unzoom"
sleep 1
drawn three-panes "*" 3
key "cmd+t"; [[ "$(field windows.0.tabs <<<"$(state)" | python3 -c 'import json,sys; print(len(json.load(sys.stdin)))')" == 2 ]] || fail "cmd+t should add a tab"
key "cmd+shift+["; [[ "$(field windows.0.tabs.0.active <<<"$(state)")" == True ]] || fail "cmd+shift+[ should go to the first tab"
key "cmd+shift+]"; [[ "$(field windows.0.tabs.1.active <<<"$(state)")" == True ]] || fail "cmd+shift+] should go to the second tab"
key "cmd+n"; [[ "$(field windows.1.key <<<"$(state)")" == True ]] || fail "cmd+n should open a key window"
key "cmd+\`"; [[ "$(field windows.0.key <<<"$(state)")" == True ]] || fail "cmd+\` should cycle to the first window"
key "cmd+w"; [[ "$(field windows.0.tabs <<<"$(state)" | python3 -c 'import json,sys; print(len(json.load(sys.stdin)))')" == 1 ]] || fail "cmd+w should close the only pane in tab 2"
key "cmd+shift+w"
[[ "$(field windows <<<"$(state)" | python3 -c 'import json,sys; print(len(json.load(sys.stdin)))')" == 1 ]] || fail "cmd+shift+w should close the window"

echo "e2e OK (snapshot: $out/two-panes.png)"
