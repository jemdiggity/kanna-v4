#!/usr/bin/env bash
# Kanna's end-to-end scenarios: the reference for docs/plans/mux-build.md.
# Each scenario (S1–S8) is one behaviour of the finished system; the plan
# makes them pass milestone by milestone. Runs a private Kanna (daemon,
# keeper, app in the background) on its own socket and folder, never yours,
# and stops it by PID.
#
# usage: scripts/e2e-kanna.sh [--through M0|M1|M2|M3] [S1 S4 …]
#   --through M  run the scenarios due by milestone M (default: all);
#                later ones are listed as not due
#   S…           only these scenarios
#
# Uses target/release/{kanna,kanna-daemon,kanna-keeper} and target/Kanna.app
# (scripts/build-kanna.sh). The private instance gets:
#   KANNA_SOCKET     the daemon's socket (the CLI and the app connect to it)
#   KANNA_STATE_DIR  where the daemon, keeper and app keep their files
#                    (the keeper writes keeper.pid there)
#   KANNA_KEEPER     the keeper binary the daemon starts (once it uses one)
set -uo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bin="$repo_root/target/release"
app="$repo_root/target/Kanna.app/Contents/MacOS/Kanna"
out="$repo_root/target/e2e-kanna"
mkdir -p "$out"

through=M3 only=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --through) through="$2"; shift ;;
    S[1-8]) only+=("$1") ;;
    -h|--help) sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "e2e-kanna: unknown argument $1 (see --help)" >&2; exit 2 ;;
  esac
  shift
done

# A private instance. This may run inside a Kanna pane, whose variables point
# at your Kanna: replace them all.
unset KANNA_PANE KANNA_INSTANCE
new_state_dir() {
  KANNA_STATE_DIR="$(mktemp -d /tmp/kanna-e2e.XXXXXX)" || exit 1
  [[ -n "$KANNA_STATE_DIR" && -d "$KANNA_STATE_DIR" ]] || { echo "e2e-kanna: mktemp gave no folder" >&2; exit 1; }
  export KANNA_STATE_DIR KANNA_SOCKET="$KANNA_STATE_DIR/kanna.sock"
}
new_state_dir
export KANNA_IGNORE_OCCLUSION=1 # draw even if the window is covered
export KANNA_DEBUG=1
daemon_pid="" app_pid=""

# ── the instance ──────────────────────────────────────────────────────────

need() { [[ -x "$1" ]] || { echo "not built: ${1#$repo_root/}"; return 1; }; }
wait_for() { # wait_for SECONDS COMMAND…: until COMMAND succeeds
  local tries=$(($1 * 10)); shift
  for _ in $(seq "$tries"); do "$@" 2>/dev/null && return 0; sleep 0.1; done
  return 1
}
start_daemon() {
  need "$bin/kanna-daemon" || return 1
  if "$bin/kanna-daemon" --uses-keeper; then
    need "$bin/kanna-keeper" || return 1
    KANNA_KEEPER="$bin/kanna-keeper" "$bin/kanna-daemon" >>"$out/daemon.log" 2>&1 &
  else
    "$bin/kanna-daemon" >>"$out/daemon.log" 2>&1 &
  fi
  daemon_pid=$!
  wait_for 5 test -S "$KANNA_SOCKET" || { echo "the daemon did not open its socket"; return 1; }
}
stop_daemon() { [[ -n "$daemon_pid" ]] && kill "$daemon_pid" 2>/dev/null; wait "$daemon_pid" 2>/dev/null; daemon_pid=""; }
start_app() {
  need "$app" || return 1
  "$app" --bg >>"$out/app.log" 2>&1 &
  app_pid=$!
  wait_for 10 app_attached || { echo "the app did not attach to the daemon"; return 1; }
}
stop_app() { [[ -n "$app_pid" ]] && kill "$app_pid" 2>/dev/null; wait "$app_pid" 2>/dev/null; app_pid=""; }
app_attached() { [[ "$(raw '{"id":1,"cmd":"debug.app"}' | field ok)" == True ]]; }
cleanup() {
  stop_app; stop_daemon
  stop_keeper
  rm -rf "$KANNA_STATE_DIR"
}
stop_keeper() {
  local pid
  pid="$(cat "$KANNA_STATE_DIR/keeper.pid" 2>/dev/null)" || return 0
  [[ "$pid" =~ ^[0-9]+$ ]] || return 0
  [[ "$(ps -o command= -p "$pid" 2>/dev/null)" == *kanna-keeper*"--state-dir $KANNA_STATE_DIR"* ]] || return 0
  kill "$pid" 2>/dev/null
  wait_for 5 eval "! kill -0 $pid"
}
trap cleanup EXIT

# ── helpers ───────────────────────────────────────────────────────────────

kanna() { "$bin/kanna" "$@"; }
raw() { # one control-protocol request to the daemon, printed as JSON
  python3 -c 'import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.settimeout(30); s.connect(sys.argv[1])
s.sendall(sys.argv[2].encode() + b"\n"); print(s.makefile().readline().strip())' "$KANNA_SOCKET" "$1"
}
field() { python3 -c 'import json, sys; v = json.load(sys.stdin)
for k in sys.argv[1].split("."): v = v[int(k)] if k.isdigit() else v[k]
print(json.dumps(v) if isinstance(v, (dict, list)) else v)' "$1"; }
pane() { # pane NAME FIELD: a field of one pane in `kanna list --json`
  kanna list --json | python3 -c 'import json, sys
p = [p for p in json.load(sys.stdin)["panes"] if sys.argv[1] in (p.get("name"), p.get("id"))][0]
print(json.dumps(p.get(sys.argv[2])) if isinstance(p.get(sys.argv[2]), (dict, list)) else p.get(sys.argv[2]))' "$1" "$2"
}
shows() { kanna read "$1" | grep -q -- "$2"; } # shows PANE TEXT: the terminal's screen has TEXT
is() { [[ "$(pane "$1" "$2")" == "$3" ]]; }    # is PANE FIELD VALUE
check() { "$@" || { echo "failed: $*"; return 1; }; }
layout() { kanna list --json | field windows.0.tabs.0.layout; }

# ── the scenarios ─────────────────────────────────────────────────────────

# S1 · The CLI drives a terminal: open it, type into it, read its screen.
S1() {
  start_daemon || return 1
  kanna open --name s1 >/dev/null || return 1
  kanna send s1 'echo S1-$((40+2))' || return 1
  check wait_for 5 shows s1 S1-42
}

# S2 · Typing in the app's terminal reaches the shell, and its output is drawn.
S2() {
  start_daemon && start_app || return 1
  kanna open --name s2 >/dev/null || return 1
  raw '{"id":1,"cmd":"debug.type","args":{"pane":"s2","text":"echo S2-$((40+2))\r"}}' >/dev/null
  check wait_for 5 shows s2 S2-42 || return 1
  local drawn
  drawn="$(raw '{"id":1,"cmd":"debug.snapshot","args":{"path":"s2.png","marker":"S2-42"}}')"
  cp "$KANNA_STATE_DIR/s2.png" "$out/s2.png" 2>/dev/null
  check python3 -c 'import json, sys
m = [p["marker"] for p in json.loads(sys.argv[1])["panes"] if p.get("marker", {}).get("found")]
assert m and all(x["inked"] == x["glyphs"] > 0 for x in m)' "$drawn"
}

# S3 · Terminals don't need the app: list and read with it closed.
S3() {
  start_daemon && start_app || return 1
  kanna open --name s3 --cmd 'echo S3-READY; exec sleep 600' >/dev/null || return 1
  stop_app
  check wait_for 5 shows s3 S3-READY || return 1
  check is s3 state running
}

# S4 · Quitting the app loses nothing: the same process, the same screen.
S4() {
  start_daemon && start_app || return 1
  # A full-screen program: clears, draws at fixed places, then waits.
  kanna open --name s4 --cmd 'printf "\033[2J\033[3;5HS4 TOP\033[10;20HS4 MIDDLE"; exec sleep 600' >/dev/null || return 1
  wait_for 5 shows s4 'S4 MIDDLE' || { echo "s4 never drew"; return 1; }
  local pid before after
  pid="$(pane s4 pid)" before="$(kanna read s4)"
  stop_app; start_app || return 1
  check is s4 pid "$pid" || return 1
  # What the app draws after relaunch is what the daemon had.
  after="$(raw '{"id":1,"cmd":"debug.text","args":{"pane":"s4"}}' | field text)"
  check test "$after" == "$before"
}

# S5 · The layout comes back after a relaunch.
S5() {
  start_daemon && start_app || return 1
  kanna open --name left >/dev/null && kanna open --name right --split right --size 1/3 >/dev/null || return 1
  local before
  before="$(layout)"
  stop_app; start_app || return 1
  check test "$(layout)" == "$before"
}

# S6 · The daemon can restart: shells keep running, output meanwhile isn't lost.
S6() {
  start_daemon || return 1
  kanna open --name s6 >/dev/null || return 1
  local pid
  pid="$(pane s6 pid)"
  kanna send s6 'sleep 2; echo S6-AFTER' || return 1
  stop_daemon
  sleep 3 # the shell writes while no daemon is reading
  start_daemon || return 1
  check is s6 pid "$pid" || return 1
  check wait_for 5 shows s6 S6-AFTER
}

# S7 · Exit codes are the command's own.
S7() {
  start_daemon || return 1
  kanna open --name s7 --cmd 'exit 3' >/dev/null || return 1
  check wait_for 5 is s7 state exited || return 1
  check is s7 exitCode 3
}

# S8 · Web, markdown and iOS panes: opened, restored after a relaunch, and answering.
S8() {
  start_daemon && start_app || return 1
  printf '# S8 DOC\n' >"$KANNA_STATE_DIR/s8.md"
  printf '<title>S8 PAGE</title><p id="x">S8 WEB</p>\n' >"$KANNA_STATE_DIR/s8.html"
  kanna open web --name web "file://$KANNA_STATE_DIR/s8.html" >/dev/null || return 1
  kanna open md --name doc "$KANNA_STATE_DIR/s8.md" >/dev/null || return 1
  stop_app; start_app || return 1
  check is web url "file://$KANNA_STATE_DIR/s8.html" || return 1
  check is doc file "$KANNA_STATE_DIR/s8.md" || return 1
  page_text() { [[ "$(raw '{"id":1,"cmd":"web.eval","args":{"pane":"web","js":"document.getElementById(\"x\").textContent"}}' | field result)" == "S8 WEB" ]]; }
  check wait_for 5 page_text || return 1
  if xcrun simctl list devices booted 2>/dev/null | grep -q Booted; then
    kanna open ios --name sim >/dev/null || return 1
    check is sim state running
  else
    echo "(iOS part skipped: no booted simulator)"
  fi
}

# ── the run ───────────────────────────────────────────────────────────────

due() { # due SCENARIO: the milestone that makes it pass
  case "$1" in S1|S2) echo M0 ;; S3|S4|S5) echo M1 ;; S6|S7) echo M2 ;; S8) echo M3 ;; esac
}
[[ ${#only[@]} -gt 0 ]] && scenarios=("${only[@]}") || scenarios=(S1 S2 S3 S4 S5 S6 S7 S8)
passed=0 failed=0
for s in "${scenarios[@]}"; do
  m="$(due "$s")"
  if [[ "$m" > "$through" ]]; then printf '%-3s --   not due (%s)\n' "$s" "$m"; continue; fi
  # Each scenario on a fresh instance, in this shell (so we keep its PIDs).
  cleanup
  new_state_dir
  if "$s" >"$out/$s.log" 2>&1; then
    printf '%-3s PASS (%s)\n' "$s" "$m"; passed=$((passed + 1))
  else
    printf '%-3s FAIL (%s)  %s\n' "$s" "$m" "$(tail -1 "$out/$s.log")"; failed=$((failed + 1))
  fi
  stop_app; stop_daemon
done
echo "$passed passed, $failed failed"
[[ $failed -eq 0 ]]
