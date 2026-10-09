#!/usr/bin/env bash
set -uo pipefail

usage() {
  cat <<'USAGE'
Kanna's end-to-end scenarios: the reference for docs/plans/mux-build.md.
Each scenario (S1–S13, S20–S22) is one behaviour of the finished system; the plan
makes them pass milestone by milestone. Runs a private Kanna (daemon,
keeper, app in the background) on its own socket and folder, never yours,
and stops it by PID.

usage: scripts/e2e-kanna.sh [--through M0|M1|M2|M3|M4] [S1 S4 …]
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

through=M4 only=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --through) through="$2"; shift ;;
    S[1-9]|S1[0-9]|S2[0-2]|S3[0-6]) only+=("$1") ;;
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
export KANNA_NO_INITIAL_WINDOW=1
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
  stop_instances
  stop_app; stop_daemon
  stop_started_daemon
  stop_keeper
  rm -rf "$KANNA_STATE_DIR"
}
stop_started_daemon() {
  local pid
  pid="$(cat "$KANNA_STATE_DIR/daemon.pid" 2>/dev/null)" || return 0
  [[ "$pid" =~ ^[0-9]+$ ]] || return 0
  [[ "$(ps -o command= -p "$pid" 2>/dev/null)" == *Kanna.app/Contents/Helpers/kanna-daemon* ]] || return 0
  kill "$pid" 2>/dev/null
  wait_for 5 eval "! kill -0 $pid"
}
stop_keeper() {
  local pid
  pid="$(cat "$KANNA_STATE_DIR/keeper.pid" 2>/dev/null)" || return 0
  [[ "$pid" =~ ^[0-9]+$ ]] || return 0
  [[ "$(ps -o command= -p "$pid" 2>/dev/null)" == *kanna-keeper*"--state-dir $KANNA_STATE_DIR"* ]] || return 0
  kill "$pid" 2>/dev/null
  wait_for 5 eval "! kill -0 $pid"
}
stop_folder() {
  local dir="$1" pid
  [[ -d "$dir" ]] || return 0
  pid="$(sed -n 's/.*the app attached (pid \([0-9]*\)).*/\1/p' "$dir/daemon.log" 2>/dev/null | tail -1)"
  if [[ "$pid" =~ ^[0-9]+$ && "$(ps -o command= -p "$pid" 2>/dev/null)" == *"$repo_root/target/Kanna.app/Contents/MacOS/Kanna"* ]]; then
    kill "$pid" 2>/dev/null; wait_for 5 eval "! kill -0 $pid"
  fi
  pid="$(cat "$dir/daemon.pid" 2>/dev/null)"
  if [[ "$pid" =~ ^[0-9]+$ && "$(ps -o command= -p "$pid" 2>/dev/null)" == *kanna-daemon* ]]; then
    kill "$pid" 2>/dev/null; wait_for 5 eval "! kill -0 $pid"
  fi
  pid="$(cat "$dir/keeper.pid" 2>/dev/null)"
  if [[ "$pid" =~ ^[0-9]+$ && "$(ps -o command= -p "$pid" 2>/dev/null)" == *kanna-keeper*"--state-dir $dir"* ]]; then
    kill "$pid" 2>/dev/null; wait_for 5 eval "! kill -0 $pid"
  fi
}
stop_instances() {
  local dir
  for dir in "$KANNA_STATE_DIR"/home/Library/Application\ Support/kanna-dev*; do stop_folder "$dir"; done
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
  local dir="$KANNA_STATE_DIR" before
  printf '# S8 DOC\n\nS8 TEXT\n' >"$dir/s8.md"
  printf '<title>S8 PAGE</title><p id="x">S8 WEB</p>\n' >"$dir/s8.html"
  printf '<title>S8 NEXT</title><p>S8 NEXT</p>\n' >"$dir/s8b.html"
  page_shows() { raw "{\"id\":1,\"cmd\":\"debug.web\",\"args\":{\"pane\":\"$1\"}}" | field text | grep -q "$2"; }
  doc_shows() { raw "{\"id\":1,\"cmd\":\"debug.md\",\"args\":{\"pane\":\"$1\"}}" | field text | grep -q "$2"; }
  kanna open web --name web --history "file://$dir/s8.html" >/dev/null || return 1
  kanna open md --name doc --split right "$dir/s8.md" >/dev/null || return 1
  check wait_for 5 page_shows web "S8 WEB" || return 1
  check wait_for 5 doc_shows doc "S8 TEXT" || return 1
  kanna navigate web "file://$dir/s8b.html" >/dev/null || return 1
  check wait_for 5 page_shows web "S8 NEXT" || return 1
  kanna navigate web --back >/dev/null || return 1
  check is web url "file://$dir/s8.html" || return 1
  check wait_for 5 page_shows web "S8 WEB" || return 1
  before="$(layout)"
  stop_app
  check test "$(kanna list --json | field panes)" == "[]" || return 1
  kanna navigate web "file://$dir/s8b.html" 2>/dev/null
  check test $? -eq 6 || return 1
  start_app || return 1
  check test "$(layout)" == "$before" || return 1
  check is web url "file://$dir/s8.html" || return 1
  check is doc path "$dir/s8.md" || return 1
  check wait_for 5 page_shows web "S8 WEB" || return 1
  check wait_for 5 doc_shows doc "S8 TEXT" || return 1
  stop_daemon; start_daemon || return 1
  check wait_for 10 app_attached || return 1
  check is web state running || return 1
  check test "$(layout)" == "$before" || return 1
  check page_shows web "S8 WEB" || return 1
  local udid
  udid="$(xcrun simctl list devices booted 2>/dev/null | sed -n 's/^ *iPhone.*(\([0-9A-F-]\{36\}\)) (Booted).*/\1/p' | head -1)"
  if [[ -z "$udid" ]]; then
    echo "(iOS part skipped: no booted iPhone simulator)"
    return 0
  fi
  kanna open ios --name sim --app com.apple.Preferences --device "$udid" --tab >/dev/null || return 1
  check is sim state running || return 1
  check test "$(raw '{"id":1,"cmd":"debug.ios","args":{"pane":"sim"}}' | field ok)" == True || return 1
  stop_app; start_app || return 1
  check wait_for 60 is sim state running || return 1
  check test "$(raw '{"id":1,"cmd":"debug.ios","args":{"pane":"sim"}}' | field ok)" == True
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

S10() {
  env -u KANNA_NO_INITIAL_WINDOW "$app" --bg >>"$out/app.log" 2>&1 &
  app_pid=$!
  wait_for 10 app_attached || { echo "the app did not start a daemon and attach to it"; return 1; }
  local first
  first="$(cat "$KANNA_STATE_DIR/daemon.pid")"
  check grep -q "Kanna.app/Contents/Helpers/kanna-daemon" <(ps -o command= -p "$first") || return 1
  wait_for 5 eval '[[ "$(kanna list --json | field panes.0.type)" == term ]]' || { echo "the app did not open a first terminal"; return 1; }
  kanna open --name live --split right --cmd 'echo LIVE-READY; exec cat' >/dev/null || return 1
  app_shows_live() { raw '{"id":1,"cmd":"debug.text","args":{"pane":"live"}}' | field text | grep -q LIVE-READY; }
  check wait_for 5 app_shows_live || return 1
  kill "$first"
  wait_for 15 eval '[[ "$(cat "$KANNA_STATE_DIR/daemon.pid" 2>/dev/null)" != "$first" ]] && app_attached' || { echo "the app did not start a new daemon"; return 1; }
  check test "$(kanna list --json | field panes.0.type)" == term || return 1
  stop_app
  check kill -0 "$(cat "$KANNA_STATE_DIR/daemon.pid")" || return 1
}

says() { local got; got="$("$@" 2>&1)"; [[ "$got" == "$expected" ]] || { echo "kanna ${*:2}: expected \"$expected\", got \"$got\""; return 1; }; }
exits() { local want="$1"; shift; "$@" >/dev/null 2>&1; local got=$?; [[ $got == "$want" ]] || { echo "${*}: exit $got, expected $want"; return 1; }; }
app_text() { raw "{\"id\":1,\"cmd\":\"debug.text\",\"args\":{\"pane\":\"$1\"}}" | field text | grep -q -- "$2"; }
page_text() { raw "{\"id\":1,\"cmd\":\"debug.web\",\"args\":{\"pane\":\"$1\"}}" | field text | grep -q -- "$2"; }
doc_text() { raw "{\"id\":1,\"cmd\":\"debug.md\",\"args\":{\"pane\":\"$1\"}}" | field text | grep -q -- "$2"; }

S20() {
  start_daemon && start_app || return 1
  local dir="$KANNA_STATE_DIR" expected
  cd "$dir" || return 1
  expected="opened p1 (s20) in window w1, tab t1: running"; says kanna open --name s20 --cmd 'printf "S20 READY\n"; exec cat' || return 1
  expected="opened p2 (e2e) in window w1, tab t1: running"; says kanna open --name e2e --split right --size 1/3 || return 1
  check test "$(layout)" == '{"children": [{"pane": "s20", "size": "2/3"}, {"pane": "e2e", "size": "1/3"}], "split": "row"}' || return 1
  check wait_for 5 app_text s20 "S20 READY" || return 1
  check test "$(kanna list | head -2)" == $'*w1  focused: e2e\n  *t1 "Tab 1"  s20:2/3 | e2e:1/3' || return 1
  expected="focused s20 (key window w1)"; says kanna focus s20 || return 1
  expected="e2e is zoomed (run again to unzoom)"; says kanna zoom e2e || return 1
  check test "$(kanna list --json | field windows.0.zoomed)" == e2e || return 1
  expected="e2e is no longer zoomed"; says kanna zoom e2e || return 1
  expected="s20:3/4 | e2e:1/4"; says kanna resize e2e 1/4 || return 1
  expected="s20:1/2 / e2e:1/2"; says kanna move e2e --to s20 --side bottom || return 1
  expected="e2e:1/4 | s20:3/4"; says kanna arrange '{"split":"row","children":[{"pane":"e2e","size":"1/4"},{"pane":"s20"}]}' || return 1
  expected="opened p3 (t) in window w1, tab t2: running"; says kanna open --tab --name t || return 1
  expected='t2 is now "shells"'; says kanna rename-tab t2 shells || return 1
  expected="w1: t2, t1"; says kanna move-tab t2 --index 0 || return 1
  expected="switched to t1 (key window w1)"; says kanna focus --tab t1 || return 1
  expected="t"; says kanna move t --window new || return 1
  check test "$(kanna list --json | field windows.1.tabs.0.layout)" == '{"pane": "t"}' || return 1
  expected="key window is now w1"; says kanna focus --window w1 || return 1
  expected="closed window w2 (panes p3)"; says kanna close --window w2 || return 1
  kanna send e2e 'echo S20-$((40+2))' || return 1
  check wait_for 5 shows e2e S20-42 || return 1
  check wait_for 5 app_text e2e S20-42 || return 1
  kanna send e2e -- echo --json || return 1
  check wait_for 5 eval 'kanna read e2e --lines 2 | grep -qx -- --json' || return 1
  expected="p1 (s20) (printf \"S20 READY\\n\"; exec cat) is running"; says kanna restart s20 || return 1
  printf '<title>S20</title><p>S20 PAGE</p>\n' >"$dir/a.html"
  printf '<title>S20 B</title><p>S20 OTHER</p>\n' >"$dir/b.html"
  expected="opened p4 (site) in window w1, tab t1: running"; says kanna open web "file://$dir/a.html" --name site --history --split down || return 1
  check wait_for 5 page_text site "S20 PAGE" || return 1
  expected="p4 (site) (file://$dir/b.html) is running"; says kanna navigate site "file://$dir/b.html" || return 1
  check wait_for 5 page_text site "S20 OTHER" || return 1
  expected="p4 (site) (file://$dir/a.html) is running"; says kanna navigate site --back || return 1
  check wait_for 5 page_text site "S20 PAGE" || return 1
  expected="p4 (site) (file://$dir/b.html) is running"; says kanna navigate site --forward || return 1
  kanna open web "file://$dir/a.html" --name plain --tab >/dev/null || return 1
  exits 2 kanna navigate plain --back || return 1
  printf '# S20 DOC\n\nS20 TEXT\n' >"$dir/s20.md"
  printf '# S20 NEXT\n' >"$dir/next.md"
  kanna open md s20.md --name doc --tab >/dev/null || return 1
  check is doc path "$(pwd -P)/s20.md" || return 1
  check wait_for 5 doc_text doc "S20 TEXT" || return 1
  kanna navigate doc next.md >/dev/null || return 1
  check wait_for 5 doc_text doc "S20 NEXT" || return 1
  kanna navigate doc --back >/dev/null || return 1
  check wait_for 5 doc_text doc "S20 TEXT" || return 1
  local missing
  missing="$(kanna open md "$dir/missing.md" --tab 2>&1)"; check test $? -eq 1 || return 1
  check grep -q "No such file" <<<"$missing" || return 1
  check test "$(kanna capabilities | tail -1)" == "Kanna.app: attached" || return 1
  check grep -q "The running Kanna supports every command." <(kanna help) || return 1
  check test "$(kanna open --json --name j --tab | field pane.name)" == j || return 1
  exits 4 kanna close nope || return 1
  exits 1 kanna open --name e2e || return 1
  exits 2 kanna zoom || return 1
  exits 5 kanna raw '{"cmd":"nope"}' || return 1
  exits 0 kanna raw '{"cmd":"debug.app"}' || return 1
  kanna close --window w1 >/dev/null || return 1
  check test "$(kanna list --json | field windows)" == "[]" || return 1
  expected="no windows (open one with \`kanna open\`)"; says kanna list
}

S21() {
  start_daemon || return 1
  local expected
  expected="opened p1 (s21), unplaced until Kanna.app opens: running"; says kanna open --name s21 --cmd 'echo S21 READY; exec cat' || return 1
  check test "$(kanna list | head -1)" == "no windows: Kanna.app isn't open (these terminals keep running)" || return 1
  check wait_for 5 shows s21 "S21 READY" || return 1
  kanna send s21 S21-SENT || return 1
  check wait_for 5 shows s21 S21-SENT || return 1
  expected="p1 (s21) (echo S21 READY; exec cat) is running"; says kanna restart s21 || return 1
  check grep -q "Kanna.app: not attached" <(kanna capabilities) || return 1
  kanna open --name gone >/dev/null || return 1
  expected="closed p2"; says kanna close gone || return 1
  local args
  while IFS= read -r args; do
    eval "exits 6 kanna $args" || return 1
  done <<'NEEDS'
open --split right
open --tab
open --window new
open web http://localhost:1
open md /tmp/x.md
focus s21
focus --tab t1
zoom s21
resize s21 1/2
move s21 --window new
arrange '{"split":"row","children":[{"pane":"s21"}]}'
rename-tab t1 x
move-tab t1 --index 0
close --tab t1
close --window w1
navigate site http://localhost:1
raw '{"cmd":"debug.app"}'
NEEDS
  check grep -q "KANNA_SOCKET is set, so kanna doesn't start Kanna.app" <(kanna focus s21 2>&1) || return 1
  start_app || return 1
  check test "$(layout)" == '{"pane": "s21"}' || return 1
  expected="focused s21 (key window w1)"; says kanna focus s21 || return 1
  stop_app
  check test "$(kanna list --json | field windows)" == "[]"
}

S22() {
  local home="$KANNA_STATE_DIR/home" folder support
  support="$home/Library/Application Support"
  folder="$support/kanna-dev-e2e"
  mkdir -p "$home"
  inst() { env -u KANNA_SOCKET -u KANNA_STATE_DIR -u KANNA_INSTANCE HOME="$home" KANNA_APP="$repo_root/target/Kanna.app" "$bin/kanna" "$@"; }
  check test "$(inst instances)" == "No Kanna instances are running. Any command starts one, e.g. kanna open (default) or kanna --instance work open." || return 1
  inst --instance e2e --bg open --name s22 --cmd 'echo "S22 $KANNA_INSTANCE"; exec cat' >/dev/null || return 1
  check test -S "$folder/kanna.sock" || return 1
  check grep -q "Kanna.app/Contents/Helpers/kanna-daemon" <(ps -o command= -p "$(cat "$folder/daemon.pid")") || return 1
  check wait_for 5 eval 'inst --instance e2e read s22 | grep -q "S22 e2e"' || return 1
  check test "$(inst --instance e2e capabilities --json | field instance)" == e2e || return 1
  check wait_for 10 eval '[[ "$(inst --instance e2e capabilities --json | field app)" == True ]]' || return 1
  check wait_for 5 eval 'inst --instance e2e list --json | field windows.0.tabs.0.layout | grep -q s22' || return 1
  local listed
  listed="$(inst instances)"
  check grep -q "^  e2e  1 window, [12] panes\?$" <<<"$listed" || { echo "$listed"; return 1; }
  check test "$(inst --instance e2e instances --json | field instances.0.current)" == True || return 1
  inst --instance e2e open --name s22b --split right >/dev/null || return 1
  check test "$(inst --instance e2e list --json | field windows.0.tabs.0.layout)" == '{"children": [{"pane": "s22", "size": "1/2"}, {"pane": "s22b", "size": "1/2"}], "split": "row"}' || return 1
  stop_folder "$folder"
  check test "$(inst instances)" == "No Kanna instances are running. Any command starts one, e.g. kanna open (default) or kanna --instance work open."
}

key() { raw "{\"id\":1,\"cmd\":\"debug.key\",\"args\":{\"key\":\"$1\"}}" >/dev/null; sleep 0.3; }
state() { kanna list --json; }
win() {
  state | python3 -c 'import json, sys
w = next(w for w in json.load(sys.stdin)["windows"] if w["id"] == sys.argv[1])
v = w
for k in sys.argv[2].split("."): v = v[int(k)] if k.isdigit() else v[k]
print(json.dumps(v, sort_keys=True) if isinstance(v, (dict, list)) else v)' "$1" "$2"
}
xdg() { mkdir -p "$KANNA_STATE_DIR/xdg/ghostty"; printf '%s\n' "$@" >"$KANNA_STATE_DIR/xdg/ghostty/config"; }
term_app() { XDG_CONFIG_HOME="$KANNA_STATE_DIR/xdg" start_app; }
terminal() { raw "{\"id\":1,\"cmd\":\"debug.terminal\",\"args\":$2}" | python3 -c 'import json, sys; m = json.load(sys.stdin)
assert m.get("ok"), m
v = m
for k in sys.argv[1].split("."): v = v[int(k)] if k.isdigit() else v[k]
print(json.dumps(v) if isinstance(v, (dict, list)) else v)' "$1"; }
probe() {
  cat >"$KANNA_STATE_DIR/probe.py" <<'PY'
import os, sys, tty
tty.setraw(0)
arg = lambda a: a.encode().decode("unicode_escape").encode("latin1")
os.write(1, arg(sys.argv[1]) + b"READY\r\n")
until, count, got = arg(sys.argv[2]), int(sys.argv[3]), b""
while count == 0 or got.count(until) < count:
    chunk = os.read(0, 256)
    got += chunk
    if count == 0:
        os.write(1, b"IN " + chunk.replace(b"\x1b", b"^").replace(b"\x07", b"~").replace(b"\r", b"\\r") + b"\r\n")
os.write(1, b"GOT " + got.replace(b"\x1b", b"^").replace(b"\x07", b"~") + b"\r\n")
PY
  printf "python3 %s '%s' '%s' %s; exec cat" "$KANNA_STATE_DIR/probe.py" "$1" "${2:-\\007}" "${3:-1}"
}
count() { python3 -c 'import json, sys; print(len(json.load(sys.stdin)))'; }
snap() {
  local shot
  shot="$(raw "{\"id\":1,\"cmd\":\"debug.snapshot\",\"args\":{\"window\":\"$1\",\"path\":\"$2.png\",\"marker\":\"*\"}}")"
  cp "$KANNA_STATE_DIR/$2.png" "$out/$2.png" 2>/dev/null
  printf '%s\n' "$shot"
}
drawn() {
  python3 -c 'import json, sys
shot = json.loads(sys.argv[1]); want = int(sys.argv[2])
panes = shot.get("panes", [])
assert len(panes) == want, f"{len(panes)} panes drawn, wanted {want}: {shot}"
for p in panes:
    m = p.get("marker", {})
    assert p["ink"] > 0 and m.get("found") and m["inked"] == m["glyphs"] > 0, p' "$1" "$2"
}
click_tab() { raw "{\"id\":1,\"cmd\":\"debug.click\",\"args\":{\"tab\":\"$1\",\"clicks\":${2:-1}}}" >/dev/null; sleep 0.3; }
drag() { last_drag="$1 → $(raw "{\"id\":1,\"cmd\":\"debug.drag\",\"args\":$1}")"; sleep 0.3; }

S11() {
  start_daemon && start_app || return 1
  kanna open --name k1 >/dev/null || return 1
  key cmd+d; key cmd+shift+d
  check test "$(win w1 tabs.0.layout)" == '{"children": [{"pane": "k1", "size": "1/2"}, {"children": [{"pane": "p2", "size": "1/2"}, {"pane": "p3", "size": "1/2"}], "size": "1/2", "split": "column"}], "split": "row"}' || return 1
  check test "$(win w1 focused)" == p3 || return 1
  key "cmd+]"; check test "$(win w1 focused)" == k1 || return 1
  key "cmd+["; check test "$(win w1 focused)" == p3 || return 1
  key cmd+shift+return; check test "$(win w1 zoomed)" == p3 || return 1
  key "cmd+["; check test "$(win w1 zoomed)" == None || return 1
  raw '{"id":1,"cmd":"debug.click","args":{"pane":"k1"}}' >/dev/null; sleep 0.3
  check test "$(win w1 focused)" == k1 || return 1
  sleep 1
  check drawn "$(snap w1 s11-splits)" 3 || return 1
  printf '# S11 DOC\n\nclick me\n' >"$KANNA_STATE_DIR/s11.md"
  kanna open md --name doc --split right "$KANNA_STATE_DIR/s11.md" >/dev/null || return 1
  key "cmd+["; check test "$(win w1 focused)" != doc || return 1
  raw '{"id":1,"cmd":"debug.click","args":{"pane":"doc","at":[4,200]}}' >/dev/null; sleep 0.3
  check test "$(win w1 focused)" == doc || return 1
  mdzoom() { raw '{"id":1,"cmd":"debug.md","args":{"pane":"doc"}}' | field zoom; }
  key cmd+shift+=; check python3 -c 'import sys; assert float(sys.argv[1]) > 1' "$(mdzoom)" || return 1
  key cmd+0; check test "$(mdzoom)" == 1 || return 1
  local pinched
  pinched="$(raw '{"id":1,"cmd":"debug.pinch","args":{"pane":"doc"}}')"
  check python3 -c 'import json, sys; z = json.loads(sys.argv[1])["zooms"]; assert z and z[-1] > 1.2, z' "$pinched" || return 1
  check test "$(raw '{"id":1,"cmd":"debug.stats"}' | field pid)" == "$app_pid" || return 1
  kanna close doc >/dev/null || return 1
  key cmd+t; check test "$(win w1 tabs | count)" == 2 || return 1
  key "cmd+shift+["; check test "$(win w1 tabs.0.active)" == True || return 1
  key "cmd+shift+]"; check test "$(win w1 tabs.1.active)" == True || return 1
  local tab2
  tab2="$(win w1 tabs.1.id)"
  click_tab "$tab2" 2
  for k in w e b return; do key "$k"; done
  check test "$(win w1 tabs.1.title)" == web || return 1
  click_tab "$tab2" 2
  for k in x escape; do key "$k"; done
  check test "$(win w1 tabs.1.title)" == web || return 1
  click_tab "$(win w1 tabs.0.id)"; check test "$(win w1 tabs.0.active)" == True || return 1
  click_tab "$tab2"
  key cmd+n; check test "$(state | field windows | count)" == 2 || return 1
  check test "$(win w2 key)" == True || return 1
  sleep 1
  check drawn "$(snap w2 s11-second-window)" 1 || return 1
  key 'cmd+`'; check test "$(win w1 key)" == True || return 1
  key cmd+w; check test "$(win w1 tabs | count)" == 1 || return 1
  key cmd+shift+w; check test "$(state | field windows | count)" == 1 || return 1
  local menus
  menus="$(raw '{"id":1,"cmd":"debug.menu"}')"
  for item in "Split Right" "Split Down" "New Web Pane Right" "New Markdown Pane Below…" "Open URL…" "Back" "Forward" "Home" "Next Pane" \
              "Previous Pane" "Zoom" "Move Pane to New Window" "Restart" "Close Pane" "New Tab" "Next Tab" "Previous Tab" "Zoom In" "Zoom Out" \
              "Actual Size" "New Window" "Close Window" "Move Tab to New Window" "Next Window" "Previous Window" "Undo" "Cut" "Copy" "Paste" \
              "Select All" "New Instance" "Quit Kanna"; do
    [[ "$menus" == *"\"item\":\"$item\""* ]] || { echo "no menu item \"$item\""; return 1; }
  done
  [[ "$menus" == *'"item":"Split Right","menu":"Pane","shortcut":"⌘d"'* ]] || { echo "Split Right isn't ⌘D: $menus"; return 1; }
}

S12() {
  start_daemon && start_app || return 1
  kanna open --name da >/dev/null && kanna open --name db --split right >/dev/null || return 1
  drag '{"divider":"da","at":0.26}'
  check test "$(win w1 tabs.0.layout)" == '{"children": [{"pane": "da", "size": "1/4"}, {"pane": "db", "size": "3/4"}], "split": "row"}' || { echo "$last_drag"; return 1; }
  drag '{"divider":"da","at":0.6}'
  check test "$(win w1 tabs.0.layout)" == '{"children": [{"pane": "da", "size": "3/5"}, {"pane": "db", "size": "2/5"}], "split": "row"}' || return 1
  drag '{"divider":"da","at":0.01}'
  local share
  share="$(win w1 tabs.0.layout.children.0.size)"
  check python3 -c 'import sys; s = float(sys.argv[1].rstrip("%")); assert 5 < s < 12, s' "$share" || return 1
  drag '{"divider":"da","at":0.25}'
  kanna open --name wide >/dev/null || return 1
  check test "$(win w1 tabs.0.layout)" == '{"children": [{"pane": "da", "size": "1/4"}, {"pane": "db", "size": "3/8"}, {"pane": "wide", "size": "3/8"}], "split": "row"}' || return 1
  kanna focus da >/dev/null && kanna open --name tall >/dev/null || return 1
  check test "$(win w1 tabs.0.layout)" == '{"children": [{"children": [{"pane": "da", "size": "1/2"}, {"pane": "tall", "size": "1/2"}], "size": "1/4", "split": "column"}, {"pane": "db", "size": "3/8"}, {"pane": "wide", "size": "3/8"}], "split": "row"}' || return 1
  key cmd+alt+right; check test "$(win w1 focused)" == db || return 1
  key cmd+alt+left; check test "$(win w1 focused)" == tall || return 1
  local before after
  size() { raw "{\"id\":1,\"cmd\":\"debug.text\",\"args\":{\"pane\":\"$1\"}}" | field columns; }
  before="$(size db)"
  kanna focus db >/dev/null
  key cmd+=; key cmd+shift+=
  after="$(size db)"
  check test "${after:-0}" -lt "${before:-0}" -a "${after:-0}" -gt 0 || { echo "columns $before → $after"; return 1; }
  key cmd+0
  check test "$(size db)" == "$before" || return 1
  sleep 1
  check drawn "$(snap w1 s12-resized)" 4 || return 1
  kanna open --name ended --cmd 'exit 3' --tab >/dev/null || return 1
  check wait_for 5 is ended state exited || return 1
  sleep 0.5
  check test "$(snap w1 s12-exited | field panes.0.notice)" == "exited (3)" || return 1
  kanna close ended >/dev/null || return 1
  drag "{\"pane\":\"wide\",\"to\":{\"outside\":true}}"
  check test "$(state | field windows | count)" == 2 || { echo "$last_drag"; return 1; }
  sleep 1
  local frames
  frames() { python3 -c 'import json, sys; print(json.dumps(sorted([w["id"], w["frame"]] for w in json.load(open(sys.argv[1]))["windows"])))' "$KANNA_STATE_DIR/layout.json"; }
  frames="$(frames)"
  check python3 -c 'import json, sys; f = json.loads(sys.argv[1]); assert len(f) == 2 and f[0][1] != f[1][1], f' "$frames" || return 1
  stop_app; start_app || return 1
  sleep 1
  stop_app
  check test "$(frames | field 0)" == "$(field 0 <<<"$frames")" || return 1
  frames="$(frames)"
  start_app || return 1
  sleep 1
  stop_app
  check test "$(frames)" == "$frames" || return 1
  start_app || return 1
  kanna close wide >/dev/null || return 1
  check test "$(state | field windows | count)" == 1 || return 1
  key cmd+shift+w
  check test "$(state | field windows | count)" == 0 || return 1
  check app_attached || return 1
  check test "$(kanna list --json | field panes)" == "[]"
}

S13() {
  start_daemon && start_app || return 1
  kanna open --name da >/dev/null && kanna open --name db --split right >/dev/null || return 1
  drag '{"pane":"da","to":{"pane":"db","x":0.5,"y":0.95}}'
  check test "$(win w1 tabs.0.layout)" == '{"children": [{"pane": "db", "size": "1/2"}, {"pane": "da", "size": "1/2"}], "split": "column"}' || { echo "$last_drag"; return 1; }
  drag '{"pane":"da","to":{"pane":"db","x":0.5,"y":0.5}}'
  check test "$(win w1 tabs.0.layout)" == '{"children": [{"pane": "da", "size": "1/2"}, {"pane": "db", "size": "1/2"}], "split": "column"}' || return 1
  drag '{"pane":"db","to":{"pane":"da","x":0.02,"y":0.5}}'
  check test "$(win w1 tabs.0.layout)" == '{"children": [{"pane": "db", "size": "1/2"}, {"pane": "da", "size": "1/2"}], "split": "row"}' || return 1
  drag '{"pane":"da","to":{"plus":"w1"}}'
  check test "$(win w1 tabs.1.layout)" == '{"pane": "da"}' || { echo "$(win w1 tabs) $last_drag"; return 1; }
  local first second
  first="$(win w1 tabs.0.id)" second="$(win w1 tabs.1.id)"
  drag "{\"pane\":\"da\",\"to\":{\"tab\":\"$first\"}}"
  check test "$(win w1 tabs | count)" == 1 || return 1
  check test "$(win w1 tabs.0.layout)" == '{"children": [{"pane": "db", "size": "1/2"}, {"pane": "da", "size": "1/2"}], "split": "row"}' || return 1
  kanna open --name dc --tab >/dev/null || return 1
  first="$(win w1 tabs.0.id)" second="$(win w1 tabs.1.id)"
  drag "{\"tab\":\"$second\",\"to\":{\"tab\":\"$first\"}}"
  check test "$(win w1 tabs.0.id)" == "$second" || { echo "$(win w1 tabs) $last_drag"; return 1; }
  drag "{\"tab\":\"$second\",\"to\":{\"outside\":true}}"
  check test "$(state | field windows | count)" == 2 || { echo "$last_drag"; return 1; }
  drag '{"pane":"dc","to":{"outside":true}}'
  check test "$(state | field windows | count)" == 2 || return 1
  drag '{"pane":"da","to":{"outside":true}}'
  check test "$(state | field windows | count)" == 3 || return 1
  local menu
  menu="$(raw "{\"id\":1,\"cmd\":\"debug.click\",\"args\":{\"tab\":\"$(win w1 tabs.0.id)\",\"button\":1}}")"
  check test "$(field menu <<<"$menu")" == '["Move Tab to New Window", "Close Tab"]' || return 1
  menu="$(raw '{"id":1,"cmd":"debug.click","args":{"pane":"db","button":1}}')"
  for item in "Split Right" "Close Pane" "Move Pane to New Window" "Restart"; do
    [[ "$menu" == *"\"$item\""* ]] || { echo "the pane menu has no \"$item\": $menu"; return 1; }
  done
  sleep 1
  local w
  for w in $(state | python3 -c 'import json, sys; print(" ".join(w["id"] for w in json.load(sys.stdin)["windows"]))'); do
    check drawn "$(snap "$w" "s13-$w")" 1 || return 1
  done
}

near() { python3 -c 'import sys
a, b = (bytes.fromhex(x.lstrip("#")) for x in sys.argv[1:3])
sys.exit(0 if max(abs(x - y) for x, y in zip(a, b)) <= 16 else 1)' "$1" "$2"; }

S30() {
  xdg 'palette = 1=#0102fe' 'cursor-style = bar' \
    'theme = light:'"$KANNA_STATE_DIR"'/light,dark:'"$KANNA_STATE_DIR"'/dark'
  printf 'background = #f0f0e0\nforeground = #202020\n' >"$KANNA_STATE_DIR/light"
  printf 'background = #123456\nforeground = #e5d4b0\n' >"$KANNA_STATE_DIR/dark"
  start_daemon && term_app || return 1
  terminal background '{"appearance":"dark"}' >/dev/null 2>&1
  kanna open --name s30 --cmd "$(probe '\033]11;?\007\033]10;?\007\033]4;1;?\007\033[?996n' n 1)" >/dev/null || return 1
  check wait_for 5 shows s30 'GOT ^]11;rgb:1212/3434/5656~^]10;rgb:e5e5/d4d4/b0b0~^]4;1;rgb:0101/0202/fefe~^\[?997;1n' || return 1
  check test "$(terminal settings.cursorStyle '{"pane":"s30"}')" == bar || return 1
  local drawn
  drawn="$(raw '{"id":1,"cmd":"debug.snapshot","args":{"path":"s30.png"}}' | field panes.0.background)"
  cp "$KANNA_STATE_DIR/s30.png" "$out/s30-dark.png" 2>/dev/null
  check near "$drawn" '#123456' || return 1
  kanna open --name s30b --cmd "$(probe '\033[?2031h' n 1)" >/dev/null || return 1
  check wait_for 5 shows s30b READY || return 1
  terminal background '{"pane":"s30","appearance":"light"}' >/dev/null
  check wait_for 5 shows s30b 'GOT ^\[?997;2n' || return 1
  check wait_for 5 eval '[[ "$(terminal settings.background "{\"pane\":\"s30\"}")" == "#f0f0e0" ]]' || return 1
  kanna open --name s30c --cmd "$(probe '\033]11;?\007')" >/dev/null || return 1
  check wait_for 5 shows s30c 'GOT ^]11;rgb:f0f0/f0f0/e0e0~' || return 1
  sleep 0.5
  drawn="$(raw '{"id":1,"cmd":"debug.snapshot","args":{"path":"s30-light.png"}}' | field panes.0.background)"
  cp "$KANNA_STATE_DIR/s30-light.png" "$out/s30-light.png" 2>/dev/null
  check near "$drawn" '#f0f0e0'
}

S31() {
  start_daemon && start_app || return 1
  kanna open --name s31 --cmd "printf '\033]7;file://localhost/private/tmp\007\033]2;S31 TITLE\007S31-READY'; exec cat" >/dev/null || return 1
  check wait_for 5 shows s31 S31-READY || return 1
  check wait_for 5 is s31 cwd /private/tmp || return 1
  check wait_for 5 eval '[[ "$(terminal title "{\"pane\":\"s31\"}")" == "S31 TITLE" ]]' || return 1
  stop_app
  kanna open --name s31b --cmd "printf '\033]7;file://localhost/usr\007'; exec cat" >/dev/null || return 1
  check wait_for 5 is s31b cwd /usr || return 1
  stop_daemon; start_daemon || return 1
  check is s31b cwd /usr || return 1
  kanna restart s31b --cmd 'pwd; exec cat' >/dev/null 2>&1 || kanna restart s31b >/dev/null || return 1
  check wait_for 5 is s31b cwd /usr
}

S32() {
  xdg 'clipboard-read = allow'
  start_daemon && term_app || return 1
  kanna open --name s32 --cmd "$(probe '\033[?2004h' x 0)" >/dev/null || return 1
  check wait_for 5 shows s32 READY || return 1
  terminal clipboard '{"pane":"s32","clipboard":"S32 PASTE"}' >/dev/null || return 1
  raw '{"id":1,"cmd":"debug.key","args":{"pane":"s32","key":"cmd+v"}}' >/dev/null
  check wait_for 5 shows s32 'IN ^\[200~S32 PASTE^\[201~' || return 1
  kanna open --name s32w --cmd "printf '\033]52;c;UzMyIENPUElFRA==\007S32W'; exec cat" >/dev/null || return 1
  check wait_for 5 shows s32w S32W || return 1
  check wait_for 5 eval '[[ "$(terminal clipboard "{\"pane\":\"s32\"}")" == "S32 COPIED" ]]' || return 1
  kanna open --name s32r --cmd "$(probe '\033]52;c;?\007' '\\' 1)" >/dev/null || return 1
  check wait_for 5 shows s32r 'GOT ^]52;c;UzMyIENPUElFRA==^' || return 1
  kanna open --name s32s --cmd "printf 'S32 SELECTED'; exec cat" >/dev/null || return 1
  check wait_for 5 shows s32s 'S32 SELECTED' || return 1
  terminal selection '{"pane":"s32s","action":"select_all"}' >/dev/null
  terminal clipboard '{"pane":"s32s","action":"copy_to_clipboard"}' >/dev/null
  check wait_for 5 eval 'terminal clipboard "{\"pane\":\"s32s\"}" | grep -q "S32 SELECTED"' || return 1
  stop_app; term_app || return 1
  terminal clipboard '{"pane":"s32","clipboard":"AFTER"}' >/dev/null || return 1
  raw '{"id":1,"cmd":"debug.key","args":{"pane":"s32","key":"cmd+v"}}' >/dev/null
  check wait_for 5 shows s32 'IN ^\[200~AFTER^\[201~'
}

S33() {
  start_daemon && start_app || return 1
  kanna open --name s33 --cmd "$(probe '\033[>1u\033[?1000h\033[?1006h\033[?1004h' x 0)" >/dev/null || return 1
  check wait_for 5 shows s33 READY || return 1
  stop_app; start_app || return 1
  raw '{"id":1,"cmd":"debug.key","args":{"pane":"s33","key":"shift+return"}}' >/dev/null
  check wait_for 5 shows s33 'IN ^\[13;2u' || return 1
  terminal cols '{"pane":"s33","click":{"at":[5,5]}}' >/dev/null || return 1
  check wait_for 5 shows s33 'IN ^\[<0;1;1M' || return 1
  check wait_for 5 shows s33 '\[<0;1;1m' || return 1
  kanna open --name s33b --split right >/dev/null || return 1
  check wait_for 5 eval '[[ "$(kanna read s33 | grep -oE "IN \^\[[IO]" | tail -1)" == "IN ^[O" ]]' || return 1
  kanna focus s33 >/dev/null || return 1
  check wait_for 5 eval '[[ "$(kanna read s33 | grep -oE "IN \^\[[IO]" | tail -1)" == "IN ^[I" ]]'
}

S34() {
  start_daemon && start_app || return 1
  kanna open --name s34 >/dev/null || return 1
  kanna send s34 'seq 1 3000' || return 1
  check wait_for 5 shows s34 '^3000$' || return 1
  local cols wide cell
  cols="$(terminal cols '{"pane":"s34"}')" cell="$(terminal cellWidth '{"pane":"s34"}')"
  terminal cols '{"pane":"s34","zoom":1}' >/dev/null
  check wait_for 5 eval '[[ "$(terminal cellWidth "{\"pane\":\"s34\"}")" -gt "$cell" ]]' || return 1
  check wait_for 5 eval '[[ "$(terminal cols "{\"pane\":\"s34\"}")" -lt "$cols" ]]' || return 1
  terminal cols '{"pane":"s34","zoom":0}' >/dev/null
  terminal cols '{"pane":"s34","zoom":-1}' >/dev/null
  check wait_for 5 eval '[[ "$(terminal cols "{\"pane\":\"s34\"}")" -gt "$cols" ]]' || return 1
  wide="$(terminal cols '{"pane":"s34"}')"
  kanna send s34 "stty size" || return 1
  check wait_for 5 eval 'kanna read s34 | grep -qE " $wide\$"' || return 1
  check test "$(kanna read s34 --lines 10000 | grep -c '^[0-9]*$')" -ge 3000 || return 1
  stop_app; start_app || return 1
  check test "$(kanna read s34 --lines 10000 | grep -c '^[0-9]*$')" -ge 3000 || return 1
  local before narrow
  before="$(terminal cols '{"pane":"s34"}')"
  key cmd+=; key cmd+=
  check wait_for 5 eval '[[ "$(terminal cols "{\"pane\":\"s34\"}")" -lt "$before" ]]' || return 1
  narrow="$(terminal cols '{"pane":"s34"}')"
  kanna send s34 "clear; stty size" || return 1
  check wait_for 5 eval 'kanna read s34 | grep -qE " $narrow\$"'
}

S35() {
  start_daemon && start_app || return 1
  local dir="$KANNA_STATE_DIR/links"
  mkdir -p "$dir"; printf 'plain\n' >"$dir/notes.txt"; printf '#!/bin/sh\n' >"$dir/run.command"
  cat >"$KANNA_STATE_DIR/links.sh" <<SH
printf 'https://example.com/s35\r\n'
printf '\033]8;;https://example.com/osc8\033\\\\osc8-web-link-osc8-web-link\033]8;;\033\\\\\r\n'
printf '\033]8;;x-apple.systempreferences:com.apple.Security\033\\\\osc8-custom-scheme-osc8-custom\033]8;;\033\\\\\r\n'
printf '\033]8;;file:///System/Applications/Calculator.app\033\\\\osc8-application-osc8-application\033]8;;\033\\\\\r\n'
printf '\033]8;;file://$dir/run.command\033\\\\osc8-command-file-osc8-command-file\033]8;;\033\\\\\r\n'
printf '\033]8;;file://$dir/notes.txt\033\\\\osc8-text-file-osc8-text-file-osc8\033]8;;\033\\\\\r\n'
printf '$dir/run.command\r\n'
SH
  kanna open --name s35 --cmd "sh '$KANNA_STATE_DIR/links.sh'; printf '\007\033]9;S35 NOTE\007\033]9;4;1;40\007'; sleep 1.5; printf '\033]777;notify;S35 T;S35 B\007S35-READY'; exec cat" >/dev/null || return 1
  check wait_for 5 shows s35 S35-READY || return 1
  local row
  for row in 0 1 2 3 4 5 6; do
    terminal cols "{\"pane\":\"s35\",\"click\":{\"cell\":[3,$row],\"mods\":[\"cmd\"]}}" >/dev/null || return 1
  done
  local links
  check wait_for 5 eval 'test "$(terminal links "{\"pane\":\"s35\"}" | python3 -c "import json, sys; print(len(json.load(sys.stdin)))")" == 7' || { terminal links '{"pane":"s35"}'; return 1; }
  links="$(terminal links '{"pane":"s35"}' | python3 -c 'import json, sys; print("\n".join(l["decision"] + " " + l["url"] for l in json.load(sys.stdin)))')"
  check test "$links" == "allow https://example.com/s35
allow https://example.com/osc8
confirm x-apple.systempreferences:com.apple.Security
deny:unsafeFile file:///System/Applications/Calculator.app
deny:unsafeFile file://$dir/run.command
allow file://$dir/notes.txt
deny:unsafeFile $dir/run.command" || { echo "$links"; return 1; }
  check test "$(terminal opened '{"pane":"s35"}' | python3 -c 'import json, sys; print(len(json.load(sys.stdin)))')" == 3 || return 1
  check test "$(terminal bells '{"pane":"s35"}')" -ge 1 || return 1
  check terminal notifications '{"pane":"s35"}' | grep -q "S35 NOTE" || return 1
  check terminal notifications '{"pane":"s35"}' | grep -q "S35 B" || return 1
  check test "$(terminal progress.value '{"pane":"s35"}')" == 40
}

due() {
  case "$1" in S1|S2) echo M0 ;; S3|S4|S5|S9) echo M1 ;; S6|S7) echo M2 ;; S8|S10) echo M3 ;; S11|S12|S13|S20|S21|S22|S3[0-6]) echo M4 ;; esac
}
[[ ${#only[@]} -gt 0 ]] && scenarios=("${only[@]}") || scenarios=(S1 S2 S3 S4 S5 S6 S7 S8 S9 S10 S11 S12 S13 S20 S21 S22 S30 S31 S32 S33 S34 S35)
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
