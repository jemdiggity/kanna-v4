#!/usr/bin/env bash
set -uo pipefail

usage() {
  cat <<'USAGE'
Kanna's end-to-end scenarios: the reference for docs/plans/mux-build.md.
Each scenario (S1–S9) is one behaviour of the finished system; the plan
makes them pass milestone by milestone. Runs a private Kanna (daemon,
keeper, app in the background) on its own socket and folder, never yours,
and stops it by PID.

usage: scripts/e2e-kanna.sh [--through M0|M1|M2|M3] [S1 S4 …]
  --through M  run the scenarios due by milestone M (default: all);
               later ones are listed as not due
  S…           only these scenarios

Uses target/release/{kanna,kanna-daemon,kanna-keeper} and target/Kanna.app
(scripts/build-kanna.sh). The private instance gets:
  KANNA_SOCKET     the daemon's socket (the CLI and the app connect to it)
  KANNA_STATE_DIR  where the daemon, keeper and app keep their files
                   (the keeper writes keeper.pid there)
  KANNA_KEEPER     the keeper binary the daemon starts (once it uses one)
USAGE
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bin="$repo_root/target/release"
app="$repo_root/target/Kanna.app/Contents/MacOS/Kanna"
out="$repo_root/target/e2e-kanna"
mkdir -p "$out"

through=M3 only=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --through) through="$2"; shift ;;
    S[1-9]) only+=("$1") ;;
    -h|--help) usage; exit 0 ;;
    *) echo "e2e-kanna: unknown argument $1 (see --help)" >&2; exit 2 ;;
  esac
  shift
done

unset KANNA_PANE KANNA_INSTANCE
new_state_dir() {
  KANNA_STATE_DIR="$(mktemp -d /tmp/kanna-e2e.XXXXXX)" || exit 1
  [[ -n "$KANNA_STATE_DIR" && -d "$KANNA_STATE_DIR" ]] || { echo "e2e-kanna: mktemp gave no folder" >&2; exit 1; }
  export KANNA_STATE_DIR KANNA_SOCKET="$KANNA_STATE_DIR/kanna.sock"
}
new_state_dir
export KANNA_IGNORE_OCCLUSION=1
export KANNA_DEBUG=1
daemon_pid="" app_pid=""

need() { [[ -x "$1" ]] || { echo "not built: ${1#$repo_root/}"; return 1; }; }
wait_for() {
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
stop_app() {
  [[ -n "$app_pid" ]] || return 0
  kill "$app_pid" 2>/dev/null; wait "$app_pid" 2>/dev/null
  wait_for 5 eval "! kill -0 $app_pid"
  app_pid=""
}
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

kanna() { "$bin/kanna" "$@"; }
raw() {
  python3 -c 'import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.settimeout(30); s.connect(sys.argv[1])
s.sendall(sys.argv[2].encode() + b"\n"); print(s.makefile().readline().strip())' "$KANNA_SOCKET" "$1"
}
field() { python3 -c 'import json, sys; v = json.load(sys.stdin)
for k in sys.argv[1].split("."): v = v[int(k)] if k.isdigit() else v[k]
print(json.dumps(v) if isinstance(v, (dict, list)) else v)' "$1"; }
pane() {
  kanna list --json | python3 -c 'import json, sys
p = [p for p in json.load(sys.stdin)["panes"] if sys.argv[1] in (p.get("name"), p.get("id"))][0]
print(json.dumps(p.get(sys.argv[2])) if isinstance(p.get(sys.argv[2]), (dict, list)) else p.get(sys.argv[2]))' "$1" "$2"
}
shows() { kanna read "$1" | grep -q -- "$2"; }
is() { [[ "$(pane "$1" "$2")" == "$3" ]]; }
check() { "$@" || { echo "failed: $*"; return 1; }; }
layout() { kanna list --json | field windows.0.tabs.0.layout; }

S1() {
  start_daemon || return 1
  kanna open --name s1 >/dev/null || return 1
  kanna send s1 'echo S1-$((40+2))' || return 1
  check wait_for 5 shows s1 S1-42
}

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

S3() {
  start_daemon && start_app || return 1
  kanna open --name s3 --cmd 'echo S3-READY; exec sleep 600' >/dev/null || return 1
  stop_app
  check wait_for 5 shows s3 S3-READY || return 1
  check is s3 state running
}

S4() {
  start_daemon && start_app || return 1
  kanna open --name s4 --cmd 'printf "\033[2J\033[3;5HS4 TOP\033[10;20HS4 MIDDLE"; exec sleep 600' >/dev/null || return 1
  wait_for 5 shows s4 'S4 MIDDLE' || { echo "s4 never drew"; return 1; }
  local pid before after
  pid="$(pane s4 pid)" before="$(kanna read s4)"
  stop_app; start_app || return 1
  check is s4 pid "$pid" || return 1
  after="$(raw '{"id":1,"cmd":"debug.text","args":{"pane":"s4"}}' | field text)"
  check test "$after" == "$before"
}

S5() {
  start_daemon && start_app || return 1
  kanna open --name left >/dev/null && kanna open --name right --split right --size 1/3 >/dev/null || return 1
  local before
  before="$(layout)"
  stop_app; start_app || return 1
  check test "$(layout)" == "$before"
}

S6() {
  start_daemon && start_app || return 1
  kanna open --name s6 >/dev/null || return 1
  kanna open --name s6b --split right >/dev/null || return 1
  local pid before
  pid="$(pane s6 pid)" before="$(layout)"
  kanna send s6 'echo S6-BEFORE; sleep 2; echo S6-AFTER' || return 1
  check wait_for 5 shows s6 S6-BEFORE || return 1
  stop_daemon
  sleep 3
  start_daemon || return 1
  check is s6 pid "$pid" || return 1
  check shows s6 S6-BEFORE || return 1
  check wait_for 5 shows s6 S6-AFTER || return 1
  check wait_for 10 app_attached || return 1
  check test "$(layout)" == "$before" || return 1
  app_shows() { raw '{"id":1,"cmd":"debug.text","args":{"pane":"s6"}}' | field text | grep -q S6-AFTER; }
  check wait_for 5 app_shows
}

S7() {
  start_daemon || return 1
  kanna open --name s7 --cmd 'exit 3' >/dev/null || return 1
  check wait_for 5 is s7 state exited || return 1
  check is s7 exitCode 3
}

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

S9() {
  local home="$KANNA_STATE_DIR/home" socket
  socket="$home/Library/Application Support/kanna-dev/kanna.sock"
  mkdir -p "$(dirname "$socket")"
  KANNA_SOCKET="$socket" start_daemon || return 1
  dev() { env -u KANNA_SOCKET HOME="$home" KANNA_MUX=kanna-dev KANNA_APP="$repo_root/target/Kanna.app" KANNA_BG=1 "$bin/kanna" "$@"; }
  dev open --name left >/dev/null || return 1
  KANNA_SOCKET="$socket" "$bin/kanna" open --name pinned --split right 2>"$KANNA_STATE_DIR/pinned.err"
  check test $? -eq 6 || return 1
  check grep -q "doesn't start Kanna.app" "$KANNA_STATE_DIR/pinned.err" || return 1
  dev open --name right --split right >/dev/null
  local status=$?
  app_pid="$(sed -n 's/.*the app attached (pid \([0-9]*\)).*/\1/p' "$out/daemon.log" | tail -1)"
  check test "$status" -eq 0 || return 1
  check test "$(dev list --json | field windows.0.tabs.0.layout)" == '{"children": [{"pane": "left", "size": "1/2"}, {"pane": "right", "size": "1/2"}], "split": "row"}'
}

due() {
  case "$1" in S1|S2) echo M0 ;; S3|S4|S5|S9) echo M1 ;; S6|S7) echo M2 ;; S8) echo M3 ;; esac
}
[[ ${#only[@]} -gt 0 ]] && scenarios=("${only[@]}") || scenarios=(S1 S2 S3 S4 S5 S6 S7 S8 S9)
passed=0 failed=0
for s in "${scenarios[@]}"; do
  m="$(due "$s")"
  if [[ "$m" > "$through" ]]; then printf '%-3s --   not due (%s)\n' "$s" "$m"; continue; fi
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
