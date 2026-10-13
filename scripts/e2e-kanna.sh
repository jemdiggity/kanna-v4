#!/usr/bin/env bash
set -uo pipefail

usage() {
  cat <<'USAGE'
Kanna's end-to-end scenarios: the reference for docs/plans/mux-build.md.
Each scenario (S1–S13, S20–S22, S30–S62) is one behaviour of the finished system; the plan
makes them pass milestone by milestone. Runs a private Kanna (daemon,
keeper, app in the background) on its own socket and folder, never yours,
and stops it by PID.

usage: scripts/e2e-kanna.sh [--through M0|M1|M2|M3|M4] [S1 S4 …]
  --through M  run the scenarios due by milestone M (default: all);
               later ones are listed as not due
  S…           only these scenarios

Uses target/release/{kanna,kanna-daemon,kanna-keeper} and target/Kanna.app (or $KANNA_E2E_APP)
(scripts/build.sh). The private instance gets:
  KANNA_SOCKET     the daemon's socket (the CLI and the app connect to it)
  KANNA_STATE_DIR  where the daemon, keeper and app keep their files
                   (the keeper writes keeper.pid there)
  KANNA_KEEPER     the keeper binary the daemon starts (once it uses one)
S57 runs the real claude and codex (on PATH, or $KANNA_E2E_CLAUDE / $KANNA_E2E_CODEX)
against tools/agent-harness's mock model, each in a HOME of its own.
USAGE
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bin="$repo_root/target/release"
app="${KANNA_E2E_APP:-$repo_root/target/Kanna.app}/Contents/MacOS/Kanna"
out="$repo_root/target/e2e-kanna"
mkdir -p "$out"

through=M4 only=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --through) through="$2"; shift ;;
    S[1-9]|S1[0-9]|S2[0-2]|S3[0-9]|S4[0-9]|S5[0-9]|S6[0-9]) only+=("$1") ;;
    -h|--help) usage; exit 0 ;;
    *) echo "e2e-kanna: unknown argument $1 (see --help)" >&2; exit 2 ;;
  esac
  shift
done

unset KANNA_PANE KANNA_INSTANCE
new_state_dir() {
  KANNA_STATE_DIR="$(mktemp -d /tmp/kanna-e2e.XXXXXX)" || exit 1
  export KANNA_AGENT_RULES_DIR="$KANNA_STATE_DIR/agent-rules-override"
  export KANNA_AGENTS_HOME="$KANNA_STATE_DIR/home"
  [[ -n "$KANNA_STATE_DIR" && -d "$KANNA_STATE_DIR" ]] || { echo "e2e-kanna: mktemp gave no folder" >&2; exit 1; }
  export KANNA_STATE_DIR KANNA_SOCKET="$KANNA_STATE_DIR/kanna.sock"
  mkdir -p "$KANNA_AGENTS_HOME/.kanna-v4" && echo '{"version": 2, "setup": false}' >"$KANNA_AGENTS_HOME/.kanna-v4/agents.json"
}
new_state_dir
export KANNA_IGNORE_OCCLUSION=1
export KANNA_DEBUG=1
export KANNA_NO_INITIAL_WINDOW=1
export KANNA_AGENT_RULES_URL=none
export KANNA_AGENT="$repo_root/tools/mock-agent/mock-agent"
export KANNA_AGENT_SKILLS="${app%/MacOS/Kanna}/Resources/kanna-skills"
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
  stop_agents
  rm -rf "$KANNA_STATE_DIR"
}
stop_agents() {
  [[ -d "$KANNA_STATE_DIR/agents" ]] || return 0
  pkill -f "$KANNA_STATE_DIR/agents/" 2>/dev/null
  wait_for 10 eval "! pgrep -f '$KANNA_STATE_DIR/agents/' >/dev/null" || pkill -9 -f "$KANNA_STATE_DIR/agents/" 2>/dev/null
  return 0
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
  local home="$KANNA_STATE_DIR/home" socket folder instance
  folder="$("$bin/kanna-daemon" --build | sed -n 's/^folder=//p')"
  instance="$("$bin/kanna-daemon" --build | sed -n 's/^instance=//p')"
  [[ "$instance" == default ]] || folder="$folder-$instance"
  socket="$home/Library/Application Support/$folder/kanna.sock"
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
  local home="$KANNA_AGENTS_HOME"
  mkdir -p "$home/.claude" "$home/.codex" && echo '{"model":"opus"}' >"$home/.claude/settings.json" && echo '{"hooks":{}}' >"$home/.codex/hooks.json"
  check grep -q "you said no" <(kanna agents) || return 1
  check test -z "$(echo y | kanna agents setup --ask)" || { echo "S10: setup --ask asked after you said no"; return 1; }
  check grep -q "Kanna's skills are installed" <(kanna agents setup) || return 1
  check grep -q "kanna task name" "$home/.claude/skills/kanna-tasks/SKILL.md" || return 1
  check test -f "$home/.agents/skills/kanna-planning/SKILL.md" || return 1
  check grep -qF "$KANNA_SOCKET" "$home/.claude/settings.json" || return 1
  check grep -q "kanna-tasks skill" "$home/.claude/settings.json" || return 1
  check grep -q "kanna-tasks skill" "$home/.codex/hooks.json" || return 1
  check grep -q "up to date" <(kanna agents) || return 1
  check test -z "$(echo y | kanna agents update --ask)" || { echo "S10: update --ask asked with nothing new"; return 1; }
  kanna agents remove >/dev/null || return 1
  check test ! -e "$home/.agents/skills/kanna-tasks" || return 1
  check test "$(tr -d ' \n' <"$home/.claude/settings.json")" == '{"model":"opus"}' || return 1
  check test "$(tr -d ' \n' <"$home/.codex/hooks.json")" == '{"hooks":{}}' || return 1
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
  check test "$(inst instances)" == "No Kanna instances are running. Any command starts one, e.g. kanna open or kanna --instance work open." || return 1
  inst --instance e2e --bg open --name s22 --cmd 'echo "S22 $KANNA_INSTANCE"; exec cat' >/dev/null || return 1
  check test -S "$folder/kanna.sock" || return 1
  check grep -q "Kanna.app/Contents/Helpers/kanna-daemon" <(ps -o command= -p "$(cat "$folder/daemon.pid")") || return 1
  check wait_for 5 eval 'inst --instance e2e read s22 | grep -q "S22 e2e"' || return 1
  check test "$(inst --instance e2e capabilities --json | field instance)" == e2e || return 1
  check wait_for 10 eval '[[ "$(inst --instance e2e capabilities --json | field app)" == True ]]' || return 1
  check wait_for 5 eval 'inst --instance e2e list --json | field windows.0.tabs.0.layout | grep -q s22' || return 1
  local listed
  listed="$(inst instances)"
  check grep -q "^\* e2e  dev  1 window, [12] panes\?\( .*\)\?$" <<<"$(inst --instance e2e instances)" || { inst --instance e2e instances; return 1; }
  check grep -q "^  e2e  dev  1 window, [12] panes\?\( .*\)\?$" <<<"$listed" || { echo "$listed"; return 1; }
  check test "$(inst --instance e2e instances --json | field instances.0.current)" == True || return 1
  inst --instance e2e open --name s22b --split right >/dev/null || return 1
  check test "$(inst --instance e2e list --json | field windows.0.tabs.0.layout)" == '{"children": [{"pane": "s22", "size": "1/2"}, {"pane": "s22b", "size": "1/2"}], "split": "row"}' || return 1
  stop_folder "$folder"
  check test "$(inst instances)" == "No Kanna instances are running. Any command starts one, e.g. kanna open or kanna --instance work open."
}

key() { raw "{\"id\":1,\"cmd\":\"debug.key\",\"args\":{\"key\":\"$1\"}}" >/dev/null; sleep 0.3; }
state() { kanna list --json; }
win() {
  state | python3 -c 'import json, sys
w = next(w for w in json.load(sys.stdin)["windows"] if w["id"] == sys.argv[1])
v = w
for k in sys.argv[2].split("."): v = v[int(k)] if k.isdigit() else v.get(k)
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
  key cmd+t; check wait_for 5 eval '[[ "$(win w1 tabs.1.layout)" == *p4* ]]' || return 1
  check test "$(win w1 zoomed)" == p4 || return 1
  key "cmd+shift+["; check test "$(win w1 zoomed)" == p3 || return 1
  kanna close p4 >/dev/null || return 1
  check test "$(win w1 zoomed)" == p3 || return 1
  key "cmd+["; check test "$(win w1 zoomed)" == p2 || return 1
  check test "$(win w1 focused)" == p2 || return 1
  key "cmd+]"; check test "$(win w1 zoomed)" == p3 || return 1
  check drawn "$(snap w1 s11-zoom-p3)" 1 || return 1
  key "cmd+]"; check test "$(win w1 zoomed)" == k1 || return 1
  check drawn "$(snap w1 s11-zoom-k1)" 1 || return 1
  key cmd+shift+return; check test "$(win w1 zoomed)" == None || return 1
  key "cmd+["; check test "$(win w1 focused)" == p3 || return 1
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
  local width_before; width_before="$(raw '{"id":1,"cmd":"debug.md","args":{"pane":"doc"}}' | field layout_width)"
  key cmd+shift+=; check python3 -c 'import sys; assert float(sys.argv[1]) > 1' "$(mdzoom)" || return 1
  check test "$(raw '{"id":1,"cmd":"debug.md","args":{"pane":"doc"}}' | field layout_width)" == "$width_before" || { echo "width $width_before -> $(raw '{"id":1,"cmd":"debug.md","args":{"pane":"doc"}}' | field layout_width)"; return 1; }
  key cmd+0; check test "$(mdzoom)" == 1 || return 1
  local pinched
  pinched="$(raw '{"id":1,"cmd":"debug.pinch","args":{"pane":"doc"}}')"
  check python3 -c 'import json, sys; z = json.loads(sys.argv[1])["zooms"]; assert z and z[-1] > 1.2, z' "$pinched" || return 1
  check test "$(raw '{"id":1,"cmd":"debug.stats"}' | field pid)" == "$app_pid" || return 1
  kanna close doc >/dev/null || return 1
  printf '<title>S11 PAGE</title><p>S11 WEB</p>\n' >"$KANNA_STATE_DIR/s11.html"
  kanna open web --name page --split right "file://$KANNA_STATE_DIR/s11.html" >/dev/null || return 1
  webzoom() { raw '{"id":1,"cmd":"debug.web","args":{"pane":"page"}}' | field "$1"; }
  check wait_for 5 eval 'webzoom text | grep -q "S11 WEB"' || return 1
  raw '{"id":1,"cmd":"debug.click","args":{"pane":"page","at":[4,200]}}' >/dev/null; sleep 0.3
  check test "$(win w1 focused)" == page || return 1
  key cmd+shift+=; check python3 -c 'import sys; assert float(sys.argv[1]) > 1' "$(webzoom zoom)" || return 1
  key cmd+-; check python3 -c 'import sys; assert float(sys.argv[1]) == 1' "$(webzoom zoom)" || return 1
  pinched="$(raw '{"id":1,"cmd":"debug.pinch","args":{"pane":"page"}}')"
  check python3 -c 'import json, sys; z = json.loads(sys.argv[1])["zooms"]; assert z and z[-1] > 1.2, z' "$pinched" || return 1
  key cmd+0; check python3 -c 'import sys; assert float(sys.argv[1]) == 1' "$(webzoom zoom)" || return 1
  key cmd+-; check python3 -c 'import sys; assert float(sys.argv[1]) < 1' "$(webzoom zoom)" || return 1
  key cmd+0
  kanna close page >/dev/null || return 1
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
  raw '{"id":1,"cmd":"debug.menu","args":{"item":"New Window"}}' >/dev/null; sleep 0.3
  check test "$(state | field windows | count)" == 2 || return 1
  check test "$(raw '{"id":1,"cmd":"debug.desk","args":{"window":"w2"}}' | field desk)" == False || return 1
  check test "$(win w2 key)" == True || return 1
  sleep 1
  check drawn "$(snap w2 s11-second-window)" 1 || return 1
  key 'cmd+`'; check test "$(win w1 key)" == True || return 1
  key cmd+w; check test "$(win w1 tabs | count)" == 1 || return 1
  key cmd+shift+w; check test "$(state | field windows | count)" == 1 || return 1
  local menus
  menus="$(raw '{"id":1,"cmd":"debug.menu"}')"
  for item in "Split Right" "Split Down" "New Web Pane Right" "New Markdown Pane Below…" "Open URL…" "Back" "Forward" "Home" "Next Pane" \
              "Previous Pane" "Zoom" "Move Pane to New Window" "Reload" "Close Pane" "New Tab" "Next Tab" "Previous Tab" "Zoom In" "Zoom Out" \
              "Actual Size" "New Window" "Close Window" "Move Tab to New Window" "Next Window" "Previous Window" "Undo" "Cut" "Copy" "Paste" \
              "Select All" "New Desk" "Quit $(/usr/libexec/PlistBuddy -c "Print CFBundleName" "${app%/Contents/MacOS/Kanna}/Contents/Info.plist")"; do
    [[ "$menus" == *"\"item\":\"$item\""* ]] || { echo "no menu item \"$item\""; return 1; }
  done
  [[ "$menus" == *'"item":"Split Right","menu":"Pane","shortcut":"⌘d"'* ]] || { echo "Split Right isn't ⌘D: $menus"; return 1; }
  [[ "$menus" == *'"item":"New Desk","menu":"File","shortcut":"⌘n"'* ]] || { echo "New Desk isn't File ⌘N: $menus"; return 1; }
  [[ "$menus" == *'"item":"New Window","menu":"Window","shortcut":""'* ]] || { echo "New Window has a shortcut: $menus"; return 1; }
  [[ "$menus" != *'"New Instance"'* && "$menus" != *'"Start a Task"'* ]] || { echo "a removed menu item is back: $menus"; return 1; }
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
  for item in "Split Right" "Close Pane" "Move Pane to New Window" "Reload"; do
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
  check wait_for 5 shows s30 'GOT ^]11;rgb:1212/3434/5656~^]10;rgb:e5e5/d4d4/b0b0~^]4;1;rgb:0101/0202/fefe~^\[?997;1n' || { kanna read s30; return 1; }
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
  check wait_for 5 eval 'kanna read s32 | tr -d "\\n" | sed "s/IN //g" | grep -q "\\^\\[200~S32 PASTE\\^\\[201~"' || { kanna read s32; return 1; }
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
  check wait_for 5 eval 'kanna read s32 | tr -d "\\n" | sed "s/IN //g" | grep -q "\\^\\[200~AFTER\\^\\[201~"'
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
deny:unsupportedScheme x-apple.systempreferences:com.apple.Security
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

lum() { python3 -c 'import sys
r, g, b = bytes.fromhex(sys.argv[1].lstrip("#"))
print(int(0.2126 * r + 0.7152 * g + 0.0722 * b))' "$1"; }
tabbar() { raw "{\"id\":1,\"cmd\":\"debug.snapshot\",\"args\":{\"path\":\"$1.png\"}}" | field tabBar.background; cp "$KANNA_STATE_DIR/$1.png" "$out/$1.png" 2>/dev/null; }

S36() {
  start_daemon && start_app || return 1
  kanna open --name s36 --cmd "$(probe '\033[?2004h' x 0)" >/dev/null || return 1
  check wait_for 5 shows s36 READY || return 1
  local shade n=0
  for look in dark light dark; do
    n=$((n + 1))
    terminal background "{\"appearance\":\"$look\"}" >/dev/null 2>&1
    sleep 0.5
    shade="$(lum "$(tabbar "s36-$n-$look")")"
    if [[ $look == dark ]]; then check test "$shade" -lt 96 || return 1; else check test "$shade" -gt 160 || return 1; fi
  done
  local file="$KANNA_STATE_DIR/Screen Shot (1).png"
  printf '\x89PNG\r\n\x1a\n' >"$file"
  raw "{\"id\":1,\"cmd\":\"debug.drop\",\"args\":{\"pane\":\"s36\",\"files\":[\"$file\",\"/tmp\"]}}" >/dev/null
  check wait_for 5 eval 'kanna read s36 | tr -d "\\n" | sed "s/IN //g" | grep -qF "^[200~${KANNA_STATE_DIR}/Screen\\ Shot\\ \\(1\\).png /tmp^[201~"' || { kanna read s36; return 1; }
  raw "{\"id\":1,\"cmd\":\"debug.drop\",\"args\":{\"pane\":\"s36\",\"image\":\"$file\"}}" >/dev/null
  check wait_for 5 eval 'kanna read s36 | tr -d "\\n" | sed "s/IN //g" | grep -qE "kanna-drops/image-[0-9-]+-[0-9A-F]{8}\\.png\\^\\[201~"' || { kanna read s36; return 1; }
  local dropped
  dropped="$(kanna read s36 | tr -d "\n" | sed "s/IN //g" | grep -oE "/[^~]*kanna-drops/image-[0-9-]+-[0-9A-F]{8}\.png" | tail -1)"
  check cmp -s "$file" "$dropped" || return 1
  raw "{\"id\":1,\"cmd\":\"debug.drop\",\"args\":{\"pane\":\"s36\",\"files\":[\"/tmp/a\\nb\"]}}" >/dev/null
  check wait_for 5 eval 'kanna read s36 | tr -d "\\n" | sed "s/IN //g" | grep -qF "^[200~\$'"'"'/tmp/a\\x0ab'"'"'^[201~"' || { kanna read s36; return 1; }
  raw '{"id":1,"cmd":"debug.drop","args":{"pane":"s36","text":"S36 TEXT"}}' >/dev/null
  check wait_for 5 eval 'kanna read s36 | tr -d "\\n" | sed "s/IN //g" | grep -qF "^[200~S36 TEXT^[201~"'
}

S37() {
  start_daemon && start_app || return 1
  kanna open --name agent --cmd "$(probe '\033[?2004h' x 0)" >/dev/null || return 1
  check wait_for 5 shows agent READY || return 1
  local dir="$KANNA_STATE_DIR/doc" fixture="$repo_root/references/prototypes/doc-markup/fixture" agent_id at xy
  mkdir -p "$dir/.git" "$dir/.kanna/docs"
  cp "$fixture/review.md" "$dir/review.md"
  sed "s|FILE|$dir/review.md|" "$fixture/review.threads.json" >"$dir/.kanna/docs/review.md.json"
  agent_id="$(pane agent id)"
  KANNA_PANE="$agent_id" kanna open md --name doc --tab "$dir/review.md" >/dev/null || return 1
  s37_md() { raw "{\"id\":1,\"cmd\":\"debug.md\",\"args\":{\"pane\":\"doc\"${1:-}}}"; }
  check wait_for 5 eval '[[ "$(s37_md | field markup)" == False && "$(s37_md | field agent)" == None ]]' || { echo "open md should show the file without comments"; s37_md; return 1; }
  KANNA_PANE="$agent_id" kanna markup doc | grep -qF "comments on in" || return 1
  s37_count() { s37_md | field threads | python3 -c 'import json, sys; print(len(json.load(sys.stdin)))'; }
  s37_thread() { s37_md | field threads | python3 -c 'import json, sys
t = [t for t in json.load(sys.stdin) if t["id"] == sys.argv[1]][0]
v = t[sys.argv[2]]
print(len(v) if isinstance(v, list) else v)' "$1" "$2"; }
  s37_shot() { raw "{\"id\":1,\"cmd\":\"debug.snapshot\",\"args\":{\"path\":\"$1.png\"}}" >/dev/null; cp "$KANNA_STATE_DIR/$1.png" "$out/$1.png" 2>/dev/null; }
  check wait_for 5 eval '[[ "$(s37_md | field agent)" == "$agent_id" ]]' || { echo "agent=$agent_id"; kanna list --json; s37_md; return 1; }
  check test "$(s37_count)" == 3 || return 1
  sleep 0.5; s37_shot s37-threads
  at="$(s37_md | field text | python3 -c 'import sys; print(sys.stdin.read().index("SENDME"))')"
  s37_md ",\"select\":[$at,6]" >/dev/null
  key cmd+return; check test "$(s37_md | field box)" == new || return 1
  sleep 0.3; s37_shot s37-composer
  raw '{"id":1,"cmd":"debug.type","args":{"focused":true,"text":"Please fix"}}' >/dev/null
  check test "$(s37_md | field box_text)" == "Please fix" || return 1
  key cmd+return
  check wait_for 5 eval '[[ "$(s37_thread c4 status)" == sent ]]' || { s37_md; return 1; }
  check test "$(s37_thread c4 line)" == 22 || return 1
  check wait_for 5 eval 'kanna read agent | sed "s/^IN //" | tr -d "\\n " | grep -qF "^[200~[kannacommentc4·review.md:22]>SENDMEPleasefix^[201~"' || { kanna read agent; return 1; }
  xy="$(s37_md ',"mark":"c1"')"
  raw "{\"id\":1,\"cmd\":\"debug.click\",\"args\":{\"pane\":\"doc\",\"at\":[$(echo "$xy" | field x),$(echo "$xy" | field y)]}}" >/dev/null
  check test "$(s37_md | field box)" == c1 || return 1
  check test "$(s37_md | field active)" == c1 || return 1
  sleep 0.3; s37_shot s37-thread
  raw '{"id":1,"cmd":"debug.type","args":{"focused":true,"text":"Thanks"}}' >/dev/null
  key return
  check wait_for 5 eval '[[ "$(s37_thread c1 messages)" == 3 ]]' || return 1
  check wait_for 5 eval 'kanna read agent | sed "s/^IN //" | tr -d "\\n " | grep -qF "Thanks^[201~"' || return 1
  key escape; check test "$(s37_md | field box)" == None || return 1
  xy="$(s37_md ',"mark":"c3"')"
  raw "{\"id\":1,\"cmd\":\"debug.click\",\"args\":{\"pane\":\"doc\",\"command\":true,\"at\":[$(echo "$xy" | field x),$(echo "$xy" | field y)]}}" >/dev/null
  check test "$(s37_md | field box)" == new || return 1
  key escape; check test "$(s37_md | field box)" == None || return 1
  check test "$(s37_count)" == 4 || return 1
  kanna markup reply c4 --doc "$dir/review.md" "Fixed the wording." >/dev/null || return 1
  check wait_for 5 eval '[[ "$(s37_thread c4 messages)" == 2 ]]' || return 1
  kanna markup resolve c4 --doc "$dir/review.md" >/dev/null || return 1
  check wait_for 5 eval '[[ "$(s37_thread c4 state)" == resolved ]]' || return 1
  raw '{"id":1,"cmd":"debug.app","args":{"appearance":"dark"}}' >/dev/null
  sleep 0.5; s37_shot s37-dark
  kanna markup doc --off | grep -qF "comments off in" || return 1
  check wait_for 5 eval '[[ "$(s37_md | field markup)" == False ]]' || return 1
  KANNA_PANE="$agent_id" kanna markup "$dir/review.md" --name doc2 --tab >/dev/null || return 1
  check wait_for 5 eval '[[ "$(raw "{\"id\":1,\"cmd\":\"debug.md\",\"args\":{\"pane\":\"doc2\"}}" | field markup)" == True ]]'
}

S38() {
  start_daemon && start_app || return 1
  kanna open --name dt1 --cmd "printf 'test result: ok. 41 passed; 0 failed\n'; exec sleep 600" >/dev/null || return 1
  kanna open --name dt2 --tab --cmd "printf 'dev server on http://localhost:3000\n'; exec sleep 600" >/dev/null || return 1
  kanna rename-tab t1 tests >/dev/null || return 1
  kanna rename-tab t2 server >/dev/null || return 1
  local opened refused shot first
  opened="$(kanna open --name agent --dock --cmd "printf 'agent: zoom as a mode\n'; exec sleep 600")"
  [[ "$opened" == *"(agent) docked left in window w1: running" ]] || { echo "open --dock should say where it docked: $opened"; return 1; }
  check test "$(win w1 dock)" == '{"edge": "left", "pane": "agent", "width": "2/5"}' || return 1
  [[ "$(kanna list)" == *"dock: p3 (agent)  left 2/5"* ]] || { echo "list should show the dock on its own line: $(kanna list)"; return 1; }
  sleep 1
  check drawn "$(snap w1 s38-docked)" 2 || return 1
  key "cmd+shift+["
  check test "$(win w1 tabs.0.active)" == True || return 1
  check test "$(win w1 dock.pane)" == agent || return 1
  sleep 0.5
  check drawn "$(snap w1 s38-first-tab)" 2 || return 1
  key "cmd+shift+]"; check test "$(win w1 tabs.1.active)" == True || return 1
  refused="$(kanna close agent 2>&1)" && { echo "close on the docked pane should fail"; return 1; }
  [[ "$refused" == *"docked: it closes with its window"* ]] || { echo "close on the docked pane should say why: $refused"; return 1; }
  kanna focus agent >/dev/null || return 1
  key cmd+w; check test "$(win w1 dock.pane)" == agent || return 1
  key "cmd+]"; check test "$(win w1 focused)" == dt2 || return 1
  key "cmd+]"; check test "$(win w1 focused)" == agent || return 1
  key cmd+shift+return; check test "$(win w1 zoomed)" == agent || return 1
  shot="$(snap w1 s38-zoomed)"
  check test "$(field panes <<<"$shot" | count)" == 1 || return 1
  key "cmd+]"; check test "$(win w1 zoomed)" == dt2 || return 1
  check test "$(field panes <<<"$(snap w1 s38-zoomed-server)" | count)" == 1 || return 1
  key "cmd+shift+["; check test "$(win w1 zoomed)" == dt1 || return 1
  check test "$(win w1 tabs.0.active)" == True || return 1
  key "cmd+shift+]"; check test "$(win w1 zoomed)" == dt2 || return 1
  key "cmd+["; check test "$(win w1 zoomed)" == agent || return 1
  key cmd+shift+return; check test "$(win w1 zoomed)" == None || return 1
  kanna open --name dt3 --split right --cmd "printf 'GET / 200 4ms\n'; exec sleep 600" >/dev/null || return 1
  check test "$(win w1 focused)" == dt3 || return 1
  raw '{"id":1,"cmd":"debug.click","args":{"pane":"agent"}}' >/dev/null; sleep 0.3
  raw '{"id":1,"cmd":"debug.click","args":{"pane":"dt3"}}' >/dev/null; sleep 0.3
  check test "$(win w1 focused)" == dt3 || return 1
  key cmd+shift+return; check test "$(win w1 zoomed)" == dt3 || return 1
  sleep 0.5
  shot="$(snap w1 s38-tab-zoomed)"
  check drawn "$shot" 1 || return 1
  check python3 -c 'import json, sys; s = json.loads(sys.argv[1]); assert s["panes"][0]["width"] == s["width"], s' "$shot" || return 1
  key cmd+shift+return; check test "$(win w1 zoomed)" == None || return 1
  sleep 0.5
  check drawn "$(snap w1 s38-tab-unzoomed)" 3 || return 1
  kanna close dt3 >/dev/null || return 1
  kanna move agent --dock right >/dev/null || return 1
  check test "$(win w1 dock)" == '{"edge": "right", "pane": "agent", "width": "2/5"}' || return 1
  drag '{"divider":"agent","at":0.34}'
  check test "$(win w1 dock.width)" == 1/3 || { echo "$(win w1 dock) $last_drag"; return 1; }
  sleep 0.5
  check drawn "$(snap w1 s38-right)" 2 || return 1
  first="$(win w1 tabs.0.id)"
  kanna move-tab "$first" --window new >/dev/null || return 1
  check test "$(win w1 dock.pane)" == agent || return 1
  check test "$(win w1 tabs | count)" == 1 || return 1
  kanna close dt1 >/dev/null || return 1
  check test "$(kanna restart agent --json | field pane.state)" == running || return 1
  kanna move agent --tab >/dev/null || return 1
  check test "$(win w1 dock)" == None || return 1
  check test "$(win w1 tabs | count)" == 2 || return 1
  refused="$(kanna move agent --dock 2>&1)" && { echo "docking a pane that isn't a task should fail"; return 1; }
  [[ "$refused" == *"agent isn't a task, and only a task's agent docks"* ]] || { echo "docking a pane that isn't a task should say why: $refused"; return 1; }
  check test "$(win w1 dock)" == None || return 1
  kanna close agent >/dev/null || return 1
  opened="$(kanna open --name agent --dock --frozen --cmd "printf 'agent: zoom as a mode\n'; exec sleep 600")" || return 1
  [[ "$opened" == *"(agent) docked left, frozen, in window w1: running" ]] || { echo "open --dock --frozen should say it's frozen: $opened"; return 1; }
  [[ "$(kanna list)" == *"(agent)  left 2/5  frozen"* ]] || { echo "list should show the dock frozen: $(kanna list)"; return 1; }
  refused="$(kanna move agent --dock right 2>&1)" && { echo "moving a frozen docked pane should fail"; return 1; }
  [[ "$refused" == *"frozen: agent ("*") is frozen in its dock and doesn't move"* ]] || { echo "moving a frozen docked pane should say why: $refused"; return 1; }
  refused="$(kanna move dt2 --to agent 2>&1)" && { echo "swapping onto a frozen docked pane should fail"; return 1; }
  [[ "$refused" == *"is frozen in w1's dock; nothing drops on it"* ]] || { echo "swapping onto a frozen docked pane should say why: $refused"; return 1; }
  check test "$(raw '{"id":1,"cmd":"debug.drag","args":{"pane":"dt2","to":{"pane":"agent","x":0.5,"y":0.5}}}' | field dropped)" == False || return 1
  check test "$(raw '{"id":1,"cmd":"debug.drag","args":{"pane":"agent","to":{"pane":"dt2","x":0.5,"y":0.5}}}' | field dropped)" == False || return 1
  check test "$(win w1 dock.pane)" == agent || return 1
  kanna close dt2 >/dev/null || return 1
  check test "$(win w1 tabs)" == '[]' || return 1
  check test "$(win w1 dock.pane)" == agent || return 1
  sleep 0.5
  check drawn "$(snap w1 s38-only-dock)" 1 || return 1
  check test "$(win w1 focused)" == agent || return 1
  key cmd+shift+return; check test "$(win w1 zoomed)" == agent || return 1
  sleep 0.5
  shot="$(snap w1 s38-only-dock-zoomed)"
  check drawn "$shot" 1 || return 1
  check python3 -c 'import json, sys; s = json.loads(sys.argv[1]); assert s["panes"][0]["width"] == s["width"], s' "$shot" || return 1
  key cmd+shift+return; check test "$(win w1 zoomed)" == None || return 1
  kanna close --window w1 >/dev/null || return 1
  [[ "$(kanna list)" != *agent* ]] || { echo "closing the window should close its docked pane"; return 1; }

  local main lone before after
  main="$(kanna open --name tab1 --json | field window)" || return 1
  kanna open --name back --dock right --cmd "printf 'DOCK BACK\n'; exec sleep 600" >/dev/null || return 1
  kanna resize back 1/4 >/dev/null || return 1
  lone="$(kanna open --name lone --dock --window new --json | field window)" || return 1
  sleep 1.5
  check python3 -c 'import json, sys
windows = {w["id"]: w for w in json.load(open(sys.argv[1]))["windows"]}
main, lone = windows[sys.argv[2]], windows[sys.argv[3]]
assert main["dock"]["edge"] == "right" and main["dock"]["width"] == 0.25, main
assert lone["dock"]["edge"] == "left" and lone["tabs"] == [], lone' "$KANNA_STATE_DIR/layout.json" "$main" "$lone" || return 1
  before="$(state | field windows)"
  stop_app; start_app || return 1
  sleep 1
  after="$(state | field windows)"
  check test "$after" == "$before" || { echo "before: $before"; echo "after:  $after"; return 1; }
  check wait_for 5 app_text back "DOCK BACK" || return 1
  sleep 0.5
  check drawn "$(snap "$main" s38-restored)" 2
}

S39() {
  start_daemon && start_app || return 1
  kanna open --name agent --cmd "$(probe '\033[?2004h' x 0)" >/dev/null || return 1
  check wait_for 5 shows agent READY || return 1
  local dir="$KANNA_STATE_DIR/page" agent_id xy mark
  mkdir -p "$dir/.git"
  cat >"$dir/page.html" <<'HTML'
<!doctype html>
<html><head><meta charset="utf-8"><style>body { font: 16px sans-serif; margin: 40px; height: 2400px; width: 2400px; } button { font-size: 22px; padding: 10px 24px; } #red { position: absolute; left: 560px; top: 420px; width: 120px; height: 60px; background: #ff0000; }</style></head>
<body>
<h1>Mockup</h1>
<p>A page to mark up.</p>
<button id="go">Ship it</button>
<div id="red"></div>
</body></html>
HTML
  agent_id="$(pane agent id)"
  KANNA_PANE="$agent_id" kanna open web --name plain --tab "$dir/page.html" >/dev/null || return 1
  check test "$(pane plain url)" == "file://$dir/page.html" || return 1
  check wait_for 5 eval 'raw "{\"id\":1,\"cmd\":\"debug.web\",\"args\":{\"pane\":\"plain\"}}" | grep -q "Ship it"' || return 1
  [[ "$(raw '{"id":1,"cmd":"debug.web","args":{"pane":"plain"}}')" != *threads* ]] || { echo "open web should show the page without comments"; return 1; }
  kanna close plain >/dev/null || return 1
  KANNA_PANE="$agent_id" kanna markup --name page --tab "$dir/page.html" >/dev/null || return 1
  s39_web() { raw "{\"id\":1,\"cmd\":\"debug.web\",\"args\":{\"pane\":\"page\"${1:-}}}"; }
  s39_thread() { s39_web | field threads | python3 -c 'import json, sys
t = [t for t in json.load(sys.stdin) if t["id"] == sys.argv[1]][0]
for k in sys.argv[2].split("."): t = t[k]
print(len(t) if isinstance(t, list) else t)' "$1" "$2"; }
  s39_click() { raw "{\"id\":1,\"cmd\":\"debug.click\",\"args\":{\"pane\":\"page\",$1}}" >/dev/null; }
  s39_shot() { raw "{\"id\":1,\"cmd\":\"debug.snapshot\",\"args\":{\"path\":\"$1.png\"}}" >/dev/null; cp "$KANNA_STATE_DIR/$1.png" "$out/$1.png" 2>/dev/null; }
  check wait_for 5 eval '[[ "$(s39_web | field agent)" == "$agent_id" ]]' || { kanna list --json; s39_web; return 1; }
  check test "$(pane page markup)" == True || return 1
  check wait_for 5 eval 's39_web ",\"find\":\"#go\"" | grep -q at' || return 1
  xy="$(s39_web ',"find":"#go"' | field at)"
  s39_click "\"command\":true,\"at\":$xy"
  check wait_for 5 eval '[[ "$(s39_web | field box)" == new ]]' || { s39_web; return 1; }
  sleep 0.3; s39_shot s39-composer
  raw '{"id":1,"cmd":"debug.type","args":{"focused":true,"text":"Bigger"}}' >/dev/null
  key cmd+return
  check wait_for 5 eval '[[ "$(s39_thread c1 status)" == sent ]]' || { s39_web; return 1; }
  check test "$(s39_thread c1 page.selector)" == "#go" || return 1
  check test "$(s39_thread c1 page.text)" == "Ship it" || return 1
  check test "$(s39_thread c1 line)" == 6 || return 1
  mark="$(s39_thread c1 mark)"
  check test "$mark" == ".kanna/docs/marks/page.html-c1.png" || return 1
  [[ "$(head -c 4 "$dir/$mark" | tail -c 3)" == PNG ]] || { echo "the mark should be a PNG: $dir/$mark"; return 1; }
  cp "$dir/$mark" "$out/s39-mark-c1.png"
  check wait_for 5 eval 'kanna read agent | sed "s/^IN //" | tr -d "\\n " | grep -qF "^[200~[kannacommentc1·page.html:6]>thepage,element#go\"Shipit\">box"' || { kanna read agent; return 1; }
  check wait_for 5 eval 'kanna read agent | sed "s/^IN //" | tr -d "\\n " | grep -qF ">markedup:.kanna/docs/marks/page.html-c1.pngBigger^[201~"' || { kanna read agent; return 1; }
  s39_click '"command":true,"at":[300,330],"to":[500,420]'
  check wait_for 5 eval '[[ "$(s39_web | field box)" == new ]]' || return 1
  raw '{"id":1,"cmd":"debug.type","args":{"focused":true,"text":"Too empty"}}' >/dev/null
  key cmd+return
  check wait_for 5 eval '[[ "$(s39_thread c2 status)" == sent ]]' || { s39_web; return 1; }
  check test "$(s39_thread c2 page.selector)" == "" || return 1
  check wait_for 5 eval 'kanna read agent | sed "s/^IN //" | tr -d "\\n " | grep -qF "[kannacommentc2·page.html:1]>thepage,abox>box"' || { kanna read agent; return 1; }
  check wait_for 5 eval '[[ "$(s39_web | field marks | python3 -c "import json, sys; print(sorted(json.load(sys.stdin)))")" == "['"'"'c1'"'"', '"'"'c2'"'"']" ]]' || { s39_web; return 1; }
  sleep 0.3; s39_shot s39-marks
  xy="$(s39_web | field marks.c1)"
  s39_click "\"at\":$xy"
  check wait_for 5 eval '[[ "$(s39_web | field box)" == c1 ]]' || { s39_web; return 1; }
  check test "$(s39_web | field active)" == c1 || return 1
  sleep 0.3; s39_shot s39-thread
  raw '{"id":1,"cmd":"debug.type","args":{"focused":true,"text":"Thanks"}}' >/dev/null
  key return
  check wait_for 5 eval '[[ "$(s39_thread c1 messages)" == 2 ]]' || return 1
  key escape; check wait_for 5 eval '[[ "$(s39_web | field box)" == None ]]' || return 1
  s39_click "\"at\":$xy"
  check wait_for 5 eval '[[ "$(s39_web | field box)" == c1 ]]' || return 1
  s39_click '"at":[150,450]'
  check wait_for 5 eval '[[ "$(s39_web | field box)" == None ]]' || { s39_web; return 1; }
  check test "$(s39_web | field cards.c2.reply)" == False || { s39_web; return 1; }
  s39_click "\"at\":$(s39_web | field cards.c2.at)"
  check wait_for 5 eval '[[ "$(s39_web | field active)" == c2 ]]' || { s39_web; return 1; }
  check wait_for 5 eval '[[ "$(s39_web | field cards.c2.reply)" == True ]]' || { s39_web; return 1; }
  kanna markup reply c2 --doc "$dir/page.html" "Filled it." >/dev/null || return 1
  check wait_for 5 eval '[[ "$(s39_thread c2 messages)" == 2 ]]' || return 1
  sed -i '' 's|<p>A page to mark up.</p>|<p>A page to mark up.</p><p>More above the button.</p>|' "$dir/page.html"
  check wait_for 5 eval 's39_web | field text | grep -q "More above"' || return 1
  check wait_for 5 eval '[[ "$(s39_web | field marks | python3 -c "import json, sys; print(len(json.load(sys.stdin)))")" == 2 ]]' || return 1
  kanna focus page >/dev/null || return 1
  key cmd+=; key cmd+=
  check wait_for 5 eval '[[ "$(s39_web | field zoom)" == 1.25 ]]' || { s39_web; return 1; }
  sleep 0.3
  check test "$(python3 -c 'import json, sys; a, b = json.loads(sys.argv[1]), json.loads(sys.argv[2]); print(abs(a[0] - b[0]) < 2 and abs(a[1] - b[1]) < 2)' "$(s39_web | field areas.c1)" "$(s39_web ',"find":"#go"' | field at)")" == True || { s39_web; s39_web ',"find":"#go"'; return 1; }
  s39_shot s39-zoomed
  key cmd+0
  kanna navigate page "file://$dir/page.html#later" >/dev/null || return 1
  check wait_for 5 eval '[[ "$(s39_web | field hash)" == "#later" ]]' || { s39_web; return 1; }
  check wait_for 5 eval '[[ "$(s39_web | field marks)" == "{}" ]]' || { s39_web; return 1; }
  kanna navigate page "file://$dir/page.html" >/dev/null || return 1
  check wait_for 5 eval '[[ "$(s39_web | field hash)" == "" ]]' || return 1
  xy="$(s39_web ',"find":"#red"' | field at)"
  s39_click "\"command\":true,\"at\":$xy"
  check wait_for 5 eval '[[ "$(s39_web | field box)" == new ]]' || return 1
  raw '{"id":1,"cmd":"debug.type","args":{"focused":true,"text":"Red"}}' >/dev/null
  key cmd+return
  check wait_for 5 eval '[[ "$(s39_thread c3 status)" == sent ]]' || { s39_web; return 1; }
  s39_red() {
    raw "{\"id\":1,\"cmd\":\"debug.snapshot\",\"args\":{\"path\":\"$1.png\"}}" >/dev/null; cp "$KANNA_STATE_DIR/$1.png" "$out/$1.png"
    python3 - "$KANNA_STATE_DIR/$1.png" <<'PY'
import struct, sys, zlib
data = open(sys.argv[1], "rb").read()
at, idat = 8, b""
while at < len(data):
    n = struct.unpack(">I", data[at:at + 4])[0]
    kind, chunk = data[at + 4:at + 8], data[at + 8:at + 8 + n]
    at += 12 + n
    if kind == b"IHDR": w, h, _, ct = struct.unpack(">IIBB", chunk[:10])
    if kind == b"IDAT": idat += chunk
raw, bpp = zlib.decompress(idat), 4 if ct == 6 else 3
stride, prev, o, hits = w * bpp, bytearray(w * bpp), 0, []
for y in range(h):
    f, line = raw[o], bytearray(raw[o + 1:o + 1 + stride])
    o += 1 + stride
    for x in range(stride):
        a = line[x - bpp] if x >= bpp else 0
        b, c = prev[x], prev[x - bpp] if x >= bpp else 0
        if f == 1: line[x] = (line[x] + a) & 255
        elif f == 2: line[x] = (line[x] + b) & 255
        elif f == 3: line[x] = (line[x] + (a + b) // 2) & 255
        elif f == 4:
            p = a + b - c
            line[x] = (line[x] + (a if abs(p - a) <= abs(p - b) and abs(p - a) <= abs(p - c) else b if abs(p - b) <= abs(p - c) else c)) & 255
    prev = line
    for x in range(0, w, 2):
        if line[x * bpp] > 200 and line[x * bpp + 1] < 90 and line[x * bpp + 2] < 90: hits.append((x, y))
xs, ys = [p[0] for p in hits], [p[1] for p in hits]
print(f"[{(min(xs) + max(xs)) / 4}, {(min(ys) + max(ys)) / 4}]")
PY
  }
  s39_moved() { python3 -c 'import json, sys; a, b, c, d = (json.loads(v) for v in sys.argv[1:]); dx, dy = (b[0] - a[0]) - (d[0] - c[0]), (b[1] - a[1]) - (d[1] - c[1]); print(abs(dx) < 3 and abs(dy) < 3, dx, dy)' "$@"; }
  local red0 mark0 red1 mark1
  sleep 0.3
  red0="$(s39_red s39-red-start)"; mark0="$(s39_web | field areas.c3)"
  key cmd+=; key cmd+=
  raw '{"id":1,"cmd":"debug.web","args":{"pane":"page","js":"scrollTo(150, 200)"}}' >/dev/null
  sleep 0.6
  red1="$(s39_red s39-red-zoomed)"; mark1="$(s39_web | field areas.c3)"
  [[ "$(s39_moved "$red0" "$red1" "$mark0" "$mark1")" == True* ]] || { echo "the mark should move with the page: red $red0 → $red1, mark $mark0 → $mark1: $(s39_moved "$red0" "$red1" "$mark0" "$mark1")"; return 1; }
  raw '{"id":1,"cmd":"debug.pinch","args":{"pane":"page","steps":[0.15,0.1]}}' >/dev/null
  sleep 0.6
  red1="$(s39_red s39-red-pinched)"; mark1="$(s39_web | field areas.c3)"
  [[ "$(s39_moved "$red0" "$red1" "$mark0" "$mark1")" == True* ]] || { echo "the mark should move with a pinch: red $red0 → $red1, mark $mark0 → $mark1"; return 1; }
  key cmd+0
  raw '{"id":1,"cmd":"debug.web","args":{"pane":"page","js":"(() => { scrollTo(0, 0); const r = document.createRange(); r.selectNodeContents(document.querySelector(\"p\")); getSelection().removeAllRanges(); getSelection().addRange(r); return 1; })()"}}' >/dev/null
  kanna focus page >/dev/null; sleep 0.3
  key cmd+return
  check wait_for 5 eval '[[ "$(s39_web | field box)" == new ]]' || { s39_web; return 1; }
  sleep 0.3; s39_shot s39-selection
  raw '{"id":1,"cmd":"debug.type","args":{"focused":true,"text":"Reword"}}' >/dev/null
  key cmd+return
  check wait_for 5 eval '[[ "$(s39_thread c4 status)" == sent ]]' || { s39_web; return 1; }
  check test "$(s39_thread c4 page.text)" == "A page to mark up." || return 1
  check wait_for 5 eval 'kanna read agent | sed "s/^IN //" | tr -d "\n " | grep -qF "[kannacommentc4·page.html:5]>thepage,thetext\"Apagetomarkup.\"inbody>p:nth-of-type(1)>box"' || { kanna read agent; return 1; }
  check wait_for 5 eval 's39_web | field marks | grep -q c4' || { s39_web; return 1; }
  local size
  size="$(raw '{"id":1,"cmd":"debug.web","args":{"pane":"page","js":"[innerWidth, innerHeight]"}}' | field value)"
  s39_click "\"command\":true,\"at\":[4,38],\"to\":$(python3 -c 'import json, sys; w, h = json.loads(sys.argv[1]); print(json.dumps([w - 4, h + 30]))' "$size")"
  check wait_for 5 eval '[[ "$(s39_web | field box)" == new ]]' || { s39_web; return 1; }
  raw '{"id":1,"cmd":"debug.type","args":{"focused":true,"text":"Too busy overall"}}' >/dev/null
  key cmd+return
  check wait_for 5 eval '[[ "$(s39_thread c5 status)" == sent ]]' || { s39_web; return 1; }
  key escape
  check wait_for 5 eval 's39_web | field areas | grep -q c5' || { s39_web; return 1; }
  check test "$(python3 -c 'import json, sys; x, y = json.loads(sys.argv[1]); print(x < 20 and y < 60)' "$(s39_web | field marks.c5)")" == True || { echo "the whole view's dot should sit at its top-left"; s39_web; return 1; }
  sleep 0.3; s39_shot s39-dots
  raw '{"id":1,"cmd":"debug.app","args":{"appearance":"dark"}}' >/dev/null
  sleep 0.5; s39_shot s39-dark
}

S40() {
  start_daemon && start_app || return 1
  kanna open --name s40shell >/dev/null || return 1
  kanna open --name s40 --tab --cmd "$repo_root/tools/mock-agent/mock-agent" >/dev/null || return 1
  s40_status() { raw '{"id":1,"cmd":"debug.status"}' | field "panes.$(pane s40 id).$1"; }
  check wait_for 5 is s40 status idle || return 1
  check test "$(pane s40shell status)" == None || return 1
  kanna focus s40shell >/dev/null || return 1
  kanna send s40 turn || return 1
  check wait_for 5 is s40 status working || return 1
  check wait_for 10 is s40 status needs-you || return 1
  kanna list | grep -E '^p[0-9]+ +s40 +term +running +needs-you ' >/dev/null || { echo "no STATUS column"; kanna list; return 1; }
  kanna status explain s40 | grep -qF 'rule: mock/needs-you/proceed' || { kanna status explain s40; return 1; }
  check wait_for 5 eval '[[ "$(s40_status status)" == needs-you ]]' || return 1
  kanna send s40 1 || return 1
  check wait_for 5 is s40 status working || return 1
  check wait_for 10 is s40 status idle || return 1
  check wait_for 5 eval '[[ "$(s40_status status)" == idle && "$(s40_status unread)" == True ]]' || { raw '{"id":1,"cmd":"debug.status"}'; return 1; }
  kanna focus s40 >/dev/null || return 1
  check wait_for 5 eval '[[ "$(s40_status unread)" == False ]]' || return 1
  kanna send s40 exit || return 1
  check wait_for 5 is s40 status exited || return 1
  check wait_for 5 eval '[[ "$(s40_status status)" == exited ]]'
}

S41() {
  PATH="$bin:$PATH" start_daemon && start_app || return 1
  local dir="$KANNA_STATE_DIR/s41"
  mkdir -p "$dir/sub" && touch "$dir/alpha-notes.md" "$dir/alpha.html" "$dir/alpha.txt"
  kanna open --name s41 --cwd "$dir" --cmd "zsh -f -i" >/dev/null || return 1
  kanna open --name s41-web --tab web "$dir/alpha.html" >/dev/null || return 1
  kanna open --name notes --tab md "$dir/alpha-notes.md" >/dev/null || return 1
  kanna focus s41 >/dev/null || return 1
  sleep 1
  kanna send s41 "PATH='$bin':\$PATH; stty cols 120; source <(kanna completions zsh); clear" || return 1
  sleep 1
  s41_tab() {
    raw "{\"id\":1,\"cmd\":\"debug.type\",\"args\":{\"focused\":true,\"text\":\"$1\"}}" >/dev/null
    sleep 0.3; key tab; [[ -n "$2" ]] && key tab; sleep 0.7
    s41_screen=$(kanna read s41)
    [[ -n "${4:-}" ]] && { raw "{\"id\":1,\"cmd\":\"debug.snapshot\",\"args\":{\"path\":\"$4.png\"}}" >/dev/null; cp "$KANNA_STATE_DIR/$4.png" "$out/$4.png" 2>/dev/null; }
    key ctrl+c; sleep 0.3
    printf "%s\n" "$s41_screen" | grep -qE -- "$3" || { echo "kanna $1⇥ didn't show $3:"; printf "%s\n" "$s41_screen" | tail -8; return 1; }
  }
  check s41_tab "kanna open md al" "" 'open md alpha-notes\.md *$' || return 1
  check s41_tab "kanna markup alpha." "" 'markup alpha\.html *$' || return 1
  check s41_tab "kanna markup al" 2 'alpha-notes\.md.*alpha\.html|alpha\.html.*alpha-notes\.md' || return 1
  check s41_tab "kanna nav" "" 'kanna navigate *$' || return 1
  check s41_tab "kanna open " 2 '^md +-- A markdown file' || return 1
  check test "$(printf "%s\n" "$s41_screen" | grep -oE '^(term|web|md|ios) ' | tr -d ' \n')" == termwebmdios || return 1
  check s41_tab "kanna close " 2 's41-web +-- p[0-9]+ · web · alpha\.html' s41-panes || return 1
  check s41_tab "kanna focus --tab " 2 't[0-9]+ +-- "' || return 1
  check s41_tab "kanna open --split " 2 'right' || return 1
  check s41_tab "kanna open md alpha-notes.md --sp" "" '--split *$' || return 1
  kanna open --name s41-login --tab --cwd "$dir" >/dev/null || return 1
  check wait_for 10 eval '[[ -n "$(kanna read s41-login | tr -d "[:space:]")" ]]' || return 1
  sleep 2
  kanna send s41-login 'print -r -- "s41:$_comps[kanna]:${ZDOTDIR-home}:${KANNA_ZSH_ZDOTDIR-gone}"' || return 1
  check wait_for 5 shows s41-login 's41:_kanna:home:gone'
}

S43() {
  start_daemon && start_app || return 1
  desk() { raw "{\"id\":1,\"cmd\":\"debug.desk\",\"args\":{\"window\":\"${2:-w1}\"}}" | field "$1"; }
  kanna open --name agent --dock --cwd "$repo_root" >/dev/null || return 1
  kanna open --name notes --tab >/dev/null || return 1
  check test "$(win w1 dock.pane)" == agent || return 1
  check test "$(desk desk)" == True || return 1
  check test "$(desk strip)" == True || return 1
  check test "$(desk title)" == agent || return 1
  check wait_for 5 eval '[[ "$(desk header)" == "${repo_root/#$HOME/~} · $(git -C "$repo_root" branch --show-current)" ]]' || { desk header; kanna read p1 | tail -5; kanna list --json | head -c 600; return 1; }
  expected="parked p1; w1 is an empty Desk"; says kanna task park agent || return 1
  check test "$(desk title)" == Shelf || return 1
  check test "$(desk empty)" == True || return 1
  check test "$(desk page)" == True || return 1
  check test "$(desk shelf.0.task)" == p1 || return 1
  check test "$(win w1 dock)" == None || return 1
  sleep 0.5; snap w1 s43-empty >/dev/null
  kanna send notes 'echo PARKED-$((6*7))' >/dev/null || return 1
  check wait_for 5 shows notes PARKED-42 || return 1
  raw '{"id":1,"cmd":"debug.click","args":{"shelf":"p1","window":"w1"}}' >/dev/null; sleep 0.3
  check wait_for 5 eval '[[ "$(win w1 dock.pane)" == agent ]]' || return 1
  check test "$(win w1 tabs.0.layout.pane)" == notes || return 1
  check test "$(desk empty)" == False || return 1
  kanna task park >/dev/null || return 1
  stop_app; start_app || return 1
  check wait_for 5 eval '[[ "$(desk shelf.0.task)" == p1 ]]' || return 1
  check test "$(pane notes state)" == running || return 1
  expected="pulled p1 onto w1"; says kanna task pull agent || return 1
  check wait_for 5 eval '[[ "$(win w1 tabs.0.layout.pane)" == notes ]]' || return 1
  sleep 0.5; check drawn "$(snap w1 s43-desk)" 2 || return 1
  raw '{"id":1,"cmd":"debug.click","args":{"park":"w1"}}' >/dev/null
  check wait_for 5 eval '[[ "$(desk empty)" == True ]]' || return 1
  raw '{"id":1,"cmd":"debug.click","args":{"shelf":"p1","window":"w1"}}' >/dev/null
  check wait_for 5 eval '[[ "$(win w1 dock.pane)" == agent ]]' || return 1
  kanna open --name second --dock --window new >/dev/null || return 1
  check wait_for 5 eval '[[ "$(win w2 key)" == True ]]' || return 1
  kanna task pull agent >/dev/null || return 1
  check wait_for 5 eval '[[ "$(win w1 key)" == True && "$(win w1 dock.pane)" == agent ]]' || return 1
  check test "$(win w2 dock.pane)" == second || return 1
  kanna task park agent >/dev/null || return 1
  check wait_for 5 eval '[[ "$(desk empty)" == True ]]' || return 1
  key cmd+1
  check wait_for 5 eval '[[ "$(win w1 dock.pane)" == agent ]]' || return 1
  check test "$(win w2 dock.pane)" == second || return 1
  key cmd+.
  check wait_for 5 eval '[[ "$(desk empty)" == True ]]' || return 1
  key cmd+.
  check wait_for 5 eval '[[ "$(win w1 dock.pane)" == agent ]]' || return 1
  kanna open --name asker --dock --window new --cwd "$repo_root" --cmd "$repo_root/tools/mock-agent/mock-agent" >/dev/null || return 1
  check wait_for 5 is asker status idle || return 1
  kanna send asker ask || return 1
  check wait_for 5 is asker status needs-you || return 1
  kanna task park asker >/dev/null || return 1
  kanna task pull agent >/dev/null || return 1
  check wait_for 5 eval '[[ "$(win w1 key)" == True && "$(desk shelf.0.task)" == "$(pane asker id)" ]]' || return 1
  key cmd+shift+a
  check wait_for 5 eval '[[ "$(win w1 dock.pane)" == asker ]]' || return 1
  key cmd+1
  check wait_for 5 eval '[[ "$(win w1 dock.pane)" == agent ]]' || return 1
  kanna task done asker >/dev/null || return 1
  kanna open --name aside --window w2 --tab >/dev/null || return 1
  KANNA_PANE=p1 kanna open --name side >/dev/null || return 1
  check wait_for 5 eval '[[ "$(win w1 tabs.1.layout.pane)" == side && "$(win w1 key)" == True ]]' || return 1
  kanna task park agent >/dev/null || return 1
  KANNA_PANE=p1 kanna open --name later >/dev/null || return 1
  check test "$(win w1 tabs)" == "[]" || return 1
  kanna task pull agent --window w1 >/dev/null || return 1
  check wait_for 5 eval '[[ "$(win w1 tabs.2.layout.pane)" == later ]]' || return 1
  key cmd+shift+w
  check wait_for 5 eval 'kanna task list | grep -q "p1 .*Shelf"' || { kanna task list; return 1; }
  check test "$(pane agent state)" == running || return 1
  check test "$(pane notes state)" == running || return 1
  kanna open --name ta --dock --window new --cwd "$repo_root/docs" >/dev/null || return 1
  kanna open --name tb --dock --window new --cwd "$repo_root" >/dev/null || return 1
  kanna open --name tc --dock --window new --cwd "$repo_root" 2>"$KANNA_STATE_DIR/s43-limit" && return 1
  grep -q "has 3 tasks on the Shelf already" "$KANNA_STATE_DIR/s43-limit" || { cat "$KANNA_STATE_DIR/s43-limit"; return 1; }
  kanna open --name elsewhere --dock --window new --cwd "$KANNA_STATE_DIR" >/dev/null || return 1
  ta=$(kanna task list | awk '$2 == "ta" { print $4 }')
  KANNA_PANE=ta kanna open --name ta-notes >/dev/null || return 1
  expected="ta is done: closed $(pane ta id), $(pane ta-notes id)"; says kanna task done ta || return 1
  check test "$(desk empty "$ta")" == True || return 1
  expected="agent is done: closed p1, $(pane notes id), $(pane side id), $(pane later id)"; says kanna task done agent || return 1
  kanna task list | grep -q "^p1 " && return 1
  kanna open --name tc --dock --window new --cwd "$repo_root" >/dev/null || return 1
  kanna task park tb >/dev/null || return 1
  tc=$(kanna task list | awk '$2 == "tc" { print $4 }')
  away=$(kanna task list | awk '$2 == "elsewhere" { print $4 }')
  check wait_for 5 eval '[[ "$(desk shelf.0.task "$tc")" == "$(pane tb id)" ]]' || return 1
  check test "$(desk shelf "$away")" == "[]" || return 1
}

S44() {
  start_daemon && start_app || { echo "S44 line $LINENO"; return 1; }
  local repo="$KANNA_STATE_DIR/repo" agent="$repo_root/tools/mock-agent/mock-agent"
  mkdir -p "$repo" && git -C "$repo" init -q -b main && git -C "$repo" -c user.email=e@e -c user.name=e commit -q --allow-empty -m first || { echo "S44 line $LINENO"; return 1; }
  repo="$(cd "$repo" && pwd -P)"
  in_repo() { (cd "$repo" && kanna "$@"); }
  expected="added t1 \"Fix it\" to the backlog of $repo"; says in_repo task new "Fix it" "say hi" --agent "$agent" || { echo "S44 line $LINENO"; return 1; }
  in_repo task new "Plain" >/dev/null || { echo "S44 line $LINENO"; return 1; }
  [[ "$(cd "$KANNA_STATE_DIR" && kanna task new "Nowhere" 2>&1)" == *"belongs to a git repo"* ]] || { echo "S44 line $LINENO"; return 1; }
  in_repo task list | grep -qE '^t1 +Fix it +Backlog +stopped +made' || { in_repo task list; return 1; }
  expected="took t2 off the backlog"; says kanna task done t2 || { echo "S44 line $LINENO"; return 1; }
  stop_app; stop_daemon; start_daemon && start_app || { echo "S44 line $LINENO"; return 1; }
  in_repo task list | grep -qE '^t1 +Fix it +Backlog' || { echo "the backlog didn't survive a restart"; in_repo task list; return 1; }
  local started wt; started="$(kanna task shelve t1)" || { echo "S44 line $LINENO"; return 1; }
  [[ "$started" =~ ^"moved t1 to the Shelf: p1 in $repo/.kanna-worktrees/"(task-[0-9a-f]{8})" ("(task-[0-9a-f]{8})")"$ && "${BASH_REMATCH[1]}" == "${BASH_REMATCH[2]}" ]] || { echo "S44 line $LINENO: $started"; return 1; }
  wt="$repo/.kanna-worktrees/${BASH_REMATCH[1]}"
  check wait_for 5 eval '[[ "$(pane "Fix it" id)" == p1 ]]' || { echo "S44 line $LINENO"; return 1; }
  check test "$(git -C "$wt" branch --show-current)" == "${wt##*/}" || { echo "S44 line $LINENO"; return 1; }
  check test -z "$(git -C "$repo" status --porcelain)" || { echo "S44 line $LINENO"; return 1; }
  in_repo task list | grep -qE '^p1 +Fix it +Shelf' || { in_repo task list; return 1; }
  in_repo task list | grep -q '^t1 ' && { echo "S44 line $LINENO"; return 1; }
  check wait_for 5 is "Fix it" status idle || { echo "S44 line $LINENO"; return 1; }
  expected='named p1 "Fixed"'; says kanna task name p1 Fixed || { echo "S44 line $LINENO"; return 1; }
  for n in 3 4 5; do in_repo task new "More $n" --agent "$agent" >/dev/null || return 1; done
  kanna task shelve t3 >/dev/null && kanna task shelve t4 >/dev/null || { echo "S44 line $LINENO"; return 1; }
  kanna task shelve t5 2>"$KANNA_STATE_DIR/s44-limit" && { echo "S44 line $LINENO"; return 1; }
  grep -q "has 3 tasks on the Shelf" "$KANNA_STATE_DIR/s44-limit" || { cat "$KANNA_STATE_DIR/s44-limit"; return 1; }
  in_repo task list | grep -q '^t5 .*Backlog' || { echo "S44 line $LINENO"; return 1; }
  check test "$(ls "$repo/.kanna-worktrees" | wc -l | tr -d ' ')" == 3 || { echo "S44 line $LINENO"; return 1; }
  in_repo open --name plain >/dev/null || { echo "S44 line $LINENO"; return 1; }
  in_repo move plain --dock 2>"$KANNA_STATE_DIR/s44-dock" && { echo "S44 line $LINENO"; return 1; }
  grep -q "plain isn't a task, and only a task's agent docks" "$KANNA_STATE_DIR/s44-dock" || { cat "$KANNA_STATE_DIR/s44-dock"; return 1; }
  in_repo task list | grep -q ' plain$' && { echo "S44 line $LINENO"; return 1; }
  kanna task pull Fixed >/dev/null || { echo "S44 line $LINENO"; return 1; }
  check test "$(kanna list --json | python3 -c 'import json, sys; print([w["dock"].get("frozen") for w in json.load(sys.stdin)["windows"] if w.get("dock")])')" == '[True]' || { kanna list; return 1; }
  kanna move Fixed --dock right 2>"$KANNA_STATE_DIR/s44-frozen" && { echo "S44 line $LINENO"; return 1; }
  grep -q "frozen: Fixed (p1) is frozen in its dock and doesn't move" "$KANNA_STATE_DIR/s44-frozen" || { cat "$KANNA_STATE_DIR/s44-frozen"; return 1; }
  kanna move plain --to Fixed 2>"$KANNA_STATE_DIR/s44-swap" && { echo "S44 line $LINENO"; return 1; }
  grep -q "Fixed (p1) is frozen in .*'s dock; nothing drops on it" "$KANNA_STATE_DIR/s44-swap" || { cat "$KANNA_STATE_DIR/s44-swap"; return 1; }
  in_repo task list | grep -qE '^p1 +Fixed ' || { in_repo task list; return 1; }
  kanna close plain >/dev/null || { echo "S44 line $LINENO"; return 1; }
  kanna task done Fixed >/dev/null || { echo "S44 line $LINENO"; return 1; }
  kanna task shelve t5 >/dev/null || { echo "S44 line $LINENO"; return 1; }
  check test -d "$wt" || { echo "S44 line $LINENO"; return 1; }
}

S45() {
  local repo="$KANNA_STATE_DIR/repo"
  mkdir -p "$repo" && git -C "$repo" init -q -b main && git -C "$repo" -c user.email=e@e -c user.name=e commit -q --allow-empty -m first || { echo "S45 line $LINENO"; return 1; }
  repo="$(cd "$repo" && pwd -P)"
  mkdir -p "$repo/.kanna" && printf '{"planning": {"context": 400000}}\n' >"$repo/.kanna/config.json"
  rm "$KANNA_AGENTS_HOME/.kanna-v4/agents.json"
  start_daemon || { echo "S45 line $LINENO"; return 1; }
  env -u KANNA_NO_INITIAL_WINDOW "$app" --bg >>"$out/app.log" 2>&1 &
  app_pid=$!
  wait_for 10 app_attached || { echo "the app did not attach"; return 1; }
  desk() { raw "{\"id\":1,\"cmd\":\"debug.desk\",\"args\":{\"window\":\"${2:-w1}\"}}" | field "$1"; }
  local planner; check wait_for 10 eval '[[ "$(desk planner)" == p* ]]' || { desk planner; kanna list; return 1; }
  planner="$(desk planner)"
  check test "$(desk page)" == True || { echo "S45 line $LINENO"; return 1; }
  check test "$(desk strip)" == False || { echo "S45 line $LINENO"; return 1; }
  check wait_for 10 shows "$planner" "Install Kanna's skills? \[Y/n\]" || { echo "S45: the planning terminal didn't ask to install Kanna's skills"; kanna read "$planner"; return 1; }
  check shows "$planner" "Kanna needs its skills installed" || { kanna read "$planner"; return 1; }
  kanna send "$planner" "y" >/dev/null || { echo "S45 line $LINENO"; return 1; }
  check wait_for 10 shows "$planner" "This terminal is for planning" || { kanna read "$planner"; return 1; }
  check shows "$planner" "Kanna's skills are installed" || { kanna read "$planner"; return 1; }
  check test -f "$KANNA_AGENTS_HOME/.agents/skills/kanna-tasks/SKILL.md" || { echo "S45: answering yes didn't install the skills"; return 1; }
  check grep -q "kanna-planning skill" "$KANNA_AGENTS_HOME/.claude/settings.json" || return 1
  check test "$(cd "$(pane "$planner" cwd)" && pwd -P)" == "$(cd ~ && pwd -P)" || { echo "S45: Kanna started from no folder should plan in ~"; return 1; }
  check test "$(kanna task list | grep -c "$planner")" == 0 || { echo "S45: the planning terminal is listed as a task"; kanna task list; return 1; }
  kanna send "$planner" "cd '$repo'" >/dev/null || { echo "S45 line $LINENO"; return 1; }
  check wait_for 10 eval '[[ "$(desk header)" == "Planning · "*"${repo#/private} · main" ]]' || { desk header; return 1; }
  kanna send "$planner" 'echo "planner=$KANNA_PLANNER context=$CLAUDE_CODE_AUTO_COMPACT_WINDOW"' >/dev/null || { echo "S45 line $LINENO"; return 1; }
  check wait_for 5 shows "$planner" "planner=1 context=300000" || { kanna read "$planner"; return 1; }
  (cd "$repo" && KANNA_PANE="$planner" kanna task new "First" "hello" && KANNA_PANE="$planner" kanna task new "Second") >/dev/null || { echo "S45 line $LINENO"; return 1; }
  check wait_for 5 eval '[[ "$(desk backlog)" == "[\"t1\", \"t2\"]" && "$(desk page)" == True ]]' || { desk backlog; return 1; }
  sleep 0.5; snap w1 s45-planner >/dev/null
  raw '{"id":1,"cmd":"debug.click","args":{"backlog":"t1","window":"w1"}}' >/dev/null
  check wait_for 10 eval '[[ "$(desk backlog)" == "[\"t2\"]" && "$(desk shelf.0.title)" == First ]]' || { desk shelf; return 1; }
  check test "$(desk running)" == 1 || { echo "S45: the planning terminal counts as a running task"; desk running; return 1; }
  check wait_for 5 shows First "mock-agent" || { echo "S45 line $LINENO"; return 1; }
  raw '{"id":1,"cmd":"debug.click","args":{"shelf":"'"$(pane First id)"'","window":"w1"}}' >/dev/null
  check wait_for 5 eval '[[ "$(desk window.dock.pane)" == "$(pane First id)" && "$(desk planner)" == None && "$(desk page)" == False && "$(desk strip)" == True ]]' || { raw '{"id":1,"cmd":"debug.desk","args":{"window":"w1"}}'; return 1; }
  check test "$(desk waiting)" == "1 in backlog" || { desk waiting; return 1; }
  is "$planner" state running || { echo "S45: the hidden planning terminal stopped"; return 1; }
  kanna task park First >/dev/null || { echo "S45 line $LINENO"; return 1; }
  check wait_for 5 eval '[[ "$(desk planner)" == "$planner" && "$(desk page)" == True && "$(desk strip)" == False ]]' || { raw '{"id":1,"cmd":"debug.desk","args":{"window":"w1"}}'; return 1; }
  check shows "$planner" "planner=1 context=300000" || { echo "S45: the planning terminal lost its scrollback"; return 1; }
  [[ "$(desk header)" == "Planning · "*"${repo#/private} · main" ]] || { desk header; return 1; }
  kanna task shelve t2 >/dev/null || { echo "S45 line $LINENO"; return 1; }
  (cd "$repo" && kanna task new "Third" && kanna task new "Fourth") >/dev/null || { echo "S45 line $LINENO"; return 1; }
  check wait_for 5 eval '[[ "$(desk running)" == 2 && "$(desk backlog)" == "[\"t3\", \"t4\"]" ]]' || { desk running; desk backlog; return 1; }
  key cmd+t; key cmd+d
  check test "$(desk window.tabs)" == "[]" || { echo "S45: the Shelf got a tab"; desk window.tabs; return 1; }
  raw '{"id":1,"cmd":"debug.click","args":{"backlog":"t3","window":"w1"}}' >/dev/null
  check wait_for 10 eval '[[ "$(desk running)" == 3 && "$(desk backlog)" == "[\"t4\"]" ]]' || { desk running; desk backlog; return 1; }
  check test "$(desk window.dock)" == None || { echo "S45: starting a backlog task docked it"; return 1; }
  raw '{"id":1,"cmd":"debug.click","args":{"backlog":"t4","window":"w1"}}' >/dev/null
  check wait_for 5 eval '[[ "$(desk note)" == *"3 tasks on the Shelf"* ]]' || { desk note; return 1; }
  stop_app; start_app || { echo "S45 line $LINENO"; return 1; }
  check wait_for 10 eval '[[ "$(desk planner)" == "$planner" ]]' || { echo "S45: the planning terminal didn't survive the app's restart"; kanna list; return 1; }
  local refused; refused="$(raw "{\"id\":1,\"cmd\":\"backlog\",\"args\":{\"action\":\"blank\",\"repo\":\"$repo\"}}")"
  [[ "$refused" == *'"ok":false'* && "$refused" == *"backlog needs an action"* ]] || { echo "S45: the backlog still starts blank tasks: $refused"; return 1; }
  key cmd+shift+w
  check wait_for 5 eval '! kanna list --json | grep -q "\"$planner\""' || { echo "S45: closing the window left its planning terminal running"; kanna list; return 1; }
}

S46() {
  local agent="$repo_root/tools/mock-agent/mock-agent" repo="$KANNA_STATE_DIR/repo" fresh="$KANNA_STATE_DIR/fresh"
  for dir in "$repo" "$fresh"; do mkdir -p "$dir" && git -C "$dir" init -q -b main || { echo "S46 line $LINENO"; return 1; }; done
  git -C "$repo" -c user.email=e@e -c user.name=e commit -q --allow-empty -m first || { echo "S46 line $LINENO"; return 1; }
  repo="$(cd "$repo" && pwd -P)"; fresh="$(cd "$fresh" && pwd -P)"
  mkdir -p "$repo/.kanna" && printf '{"planning": {"context": 400000}}\n' >"$repo/.kanna/config.json"
  start_daemon || { echo "S46 line $LINENO"; return 1; }
  env -u KANNA_NO_INITIAL_WINDOW KANNA_START_DIR="$repo" "$app" --bg >>"$out/app.log" 2>&1 &
  app_pid=$!
  wait_for 10 app_attached || { echo "the app did not attach"; return 1; }
  desk() { raw "{\"id\":1,\"cmd\":\"debug.desk\",\"args\":{\"window\":\"$2\"}}" | field "$1"; }
  check wait_for 10 eval '[[ "$(desk planner w1)" == p* ]]' || { echo "S46: no planning terminal"; kanna list; return 1; }
  local planner; planner="$(desk planner w1)"
  check test "$(cd "$(pane "$planner" cwd)" && pwd -P)" == "$repo" || { echo "S46: Kanna started in a folder should plan there"; kanna list --json; return 1; }
  kanna send "$planner" 'echo "context=$CLAUDE_CODE_AUTO_COMPACT_WINDOW"' >/dev/null || { echo "S46 line $LINENO"; return 1; }
  check wait_for 5 shows "$planner" "context=400000" || { echo "S46: the repo's .kanna/config.json didn't set the context"; kanna read "$planner"; return 1; }
  (cd "$repo" && kanna task new "Waiting" "hello" --agent "$agent") >/dev/null || { echo "S46 line $LINENO"; return 1; }
  local opened w; opened="$(cd / && kanna "$repo")" || { echo "S46 line $LINENO"; return 1; }
  [[ "$opened" =~ ^opened\ (w[0-9]+)\ on\ "$repo"$ ]] || { echo "S46 line $LINENO: $opened"; return 1; }
  w="${BASH_REMATCH[1]}"
  check wait_for 5 eval '[[ "$(desk empty "$w")" == True && "$(desk backlog "$w")" == "[\"t1\"]" && "$(desk running "$w")" == 0 ]]' || { desk backlog "$w"; desk running "$w"; return 1; }
  check wait_for 10 eval '[[ "$(desk planner "$w")" == p* ]]' || { echo "S46: no planning terminal"; kanna list; return 1; }
  check test "$(desk window.dock "$w")" == None || { echo "S46: a Desk on a project with tasks docked something"; kanna list; return 1; }
  planner="$(desk planner "$w")"
  check test "$(cd "$(pane "$planner" cwd)" && pwd -P)" == "$repo" || { echo "S46 line $LINENO"; return 1; }
  opened="$(kanna "$fresh")" && [[ "$opened" =~ ^opened\ (w[0-9]+) ]] || { echo "S46 line $LINENO: $opened"; return 1; }
  w="${BASH_REMATCH[1]}"
  check wait_for 10 eval '[[ "$(desk planner "$w")" == p* ]]' || { raw "{\"id\":1,\"cmd\":\"debug.desk\",\"args\":{\"window\":\"$w\"}}"; kanna list; return 1; }
  planner="$(desk planner "$w")"
  check test "$(cd "$(pane "$planner" cwd)" && pwd -P)" == "$fresh" || { echo "S46: the planning terminal isn't in the empty project"; kanna list --json; return 1; }
  check wait_for 10 shows "$planner" "This terminal is for planning" || { kanna read "$planner"; return 1; }
}

S47() {
  start_daemon && start_app || { echo "S47 line $LINENO"; return 1; }
  local repo="$KANNA_STATE_DIR/repo" agent="$repo_root/tools/mock-agent/mock-agent"
  mkdir -p "$repo" && git -C "$repo" init -q -b main && git -C "$repo" -c user.email=e@e -c user.name=e commit -q --allow-empty -m first || { echo "S47 line $LINENO"; return 1; }
  repo="$(cd "$repo" && pwd -P)"
  (cd "$repo" && kanna task new "Later" "say hi" --agent "$agent") >/dev/null || { echo "S47 line $LINENO"; return 1; }
  local started wt; started="$(kanna task shelve t1)" || { echo "S47 line $LINENO"; return 1; }
  [[ "$started" =~ " in "([^ ]+)" (" ]] && wt="${BASH_REMATCH[1]/#\~/$HOME}" || { echo "S47 line $LINENO: $started"; return 1; }
  check wait_for 5 eval '[[ "$(pane Later id)" == p1 ]]' || { echo "S47 line $LINENO"; return 1; }
  touch "$wt/work-in-progress"
  local pid; pid="$(pane Later pid)"
  expected="moved p1 to the backlog as t1, still running: shelve it to bring it back"; says kanna task defer Later || { echo "S47 line $LINENO"; return 1; }
  check test "$(pane Later pid)" == "$pid" || { echo "S47: the deferred agent stopped"; kanna list; return 1; }
  (cd "$repo" && kanna task list) | grep -qE '^t1 +Later +Backlog' || { (cd "$repo" && kanna task list); return 1; }
  (cd "$repo" && kanna task list) | grep -qE '^p1 ' && { echo "S47: the deferred task is still on the Shelf"; (cd "$repo" && kanna task list); return 1; }
  check test -f "$wt/work-in-progress" || { echo "S47 line $LINENO"; return 1; }
  started="$(kanna task shelve t1)" || { echo "S47 line $LINENO"; return 1; }
  [[ "$started" == "moved t1 to the Shelf: p1 in ${wt/#$HOME/\~} ("* ]] || { echo "S47: it didn't come back as it was: $started"; return 1; }
  check test "$(pane Later pid)" == "$pid" || { echo "S47: the agent was started again"; kanna list; return 1; }
  check test "$(ls "$repo/.kanna-worktrees" | wc -l | tr -d ' ')" == 1 || { echo "S47 line $LINENO"; return 1; }
  kanna open --name mine --dock --cwd "$repo" --cmd "exec $agent" >/dev/null || { echo "S47 line $LINENO"; return 1; }
  expected="moved $(pane mine id) to the backlog as t2, still running: shelve it to bring it back"; says kanna task defer mine || { echo "S47 line $LINENO"; return 1; }
  (cd "$repo" && kanna task done t2) >/dev/null || { echo "S47 line $LINENO"; return 1; }
  check wait_for 5 eval '! kanna list --json | grep -q "\"mine\""' || { echo "S47: Done left the backlog task's agent running"; kanna list; return 1; }
  [[ "$(kanna task defer nothing 2>&1)" == *'no running task "nothing"'* ]] || { echo "S47 line $LINENO"; return 1; }
}

S48() {
  start_daemon && start_app || { echo "S48 line $LINENO"; return 1; }
  local repo="$KANNA_STATE_DIR/repo" agent="$repo_root/tools/mock-agent/mock-agent"
  mkdir -p "$repo" && git -C "$repo" init -q -b main && git -C "$repo" -c user.email=e@e -c user.name=e commit -q --allow-empty -m first || { echo "S48 line $LINENO"; return 1; }
  repo="$(cd "$repo" && pwd -P)"
  local w
  desk() { raw "{\"id\":1,\"cmd\":\"debug.desk\",\"args\":{\"window\":\"$w\"}}" | field "$1"; }
  (cd "$repo" && kanna task new "Later" "say hi" --agent "$agent") >/dev/null || { echo "S48 line $LINENO"; return 1; }
  kanna task shelve t1 >/dev/null || { echo "S48 line $LINENO"; return 1; }
  check wait_for 5 eval '[[ "$(pane Later id)" == p* ]]' || { echo "S48 line $LINENO"; return 1; }
  kanna task pull Later --window new >/dev/null || { echo "S48 line $LINENO"; return 1; }
  w="$(kanna list --json | field windows.0.id)"
  check wait_for 10 eval '[[ "$(desk window.dock.pane)" == "$(pane Later id)" ]]' || { raw '{"id":1,"cmd":"debug.desk","args":{"window":"'"$w"'"}}'; return 1; }
  check wait_for 5 shows Later "mock-agent" || { echo "S48 line $LINENO"; return 1; }
  local task pid; task="$(pane Later id)" pid="$(pane Later pid)"
  KANNA_PANE="$task" kanna open --name side --cmd 'echo SIDE-READY; exec cat' >/dev/null || { echo "S48 line $LINENO"; return 1; }
  check wait_for 5 eval '[[ "$(kanna list --json | field windows.0.tabs)" == *side* ]]' || { echo "S48: the side terminal isn't on the task's Desk"; kanna list --json; return 1; }
  local tabs; tabs="$(kanna list --json | field windows.0.tabs)"
  raw '{"id":1,"cmd":"debug.menu","args":{"item":"Move to Backlog"}}' | grep -q '"ok":true' || { echo "S48: Move to Backlog didn't run"; return 1; }
  check wait_for 5 eval '[[ "$(desk empty)" == True && "$(desk backlog)" == "[\"t1\"]" && "$(desk shelf)" == "[]" && "$(desk running)" == 0 ]]' || { raw '{"id":1,"cmd":"debug.desk","args":{"window":"'"$w"'"}}'; return 1; }
  check test "$(desk window.dock)" == None || { echo "S48: the task is still on the Desk"; return 1; }
  check test "$(pane Later pid)" == "$pid" || { echo "S48: Move to Backlog stopped the agent"; return 1; }
  kanna send Later "work 1" >/dev/null || { echo "S48 line $LINENO"; return 1; }
  check wait_for 10 shows Later "WORK_DONE" || { echo "S48: the agent didn't keep working in the backlog"; kanna read Later; return 1; }
  is side state running || { echo "S48: the task's tab stopped in the backlog"; return 1; }
  stop_app; stop_daemon; start_daemon && start_app || { echo "S48 line $LINENO"; return 1; }
  check wait_for 10 eval '[[ "$(desk backlog)" == "[\"t1\"]" && "$(desk shelf)" == "[]" ]]' || { echo "S48: the backlog task didn't survive a restart"; raw '{"id":1,"cmd":"debug.desk","args":{"window":"'"$w"'"}}'; return 1; }
  check test "$(pane Later pid)" == "$pid" || { echo "S48: the restart stopped the agent"; return 1; }
  raw '{"id":1,"cmd":"debug.click","args":{"backlog":"t1","window":"'"$w"'"}}' >/dev/null
  check wait_for 10 eval '[[ "$(desk backlog)" == "[]" && "$(desk shelf.0.task)" == "$task" ]]' || { raw '{"id":1,"cmd":"debug.desk","args":{"window":"'"$w"'"}}'; return 1; }
  check test "$(pane Later pid)" == "$pid" || { echo "S48: Move to Shelf started the agent again"; return 1; }
  kanna task pull Later --window "$w" >/dev/null || { echo "S48 line $LINENO"; return 1; }
  check wait_for 5 eval '[[ "$(desk window.dock.pane)" == "$task" ]]' || { echo "S48 line $LINENO"; return 1; }
  check test "$(kanna list --json | field windows.0.tabs)" == "$tabs" || { echo "S48: the task's tabs didn't come back as they were"; kanna list --json | field windows.0.tabs; return 1; }
  check shows side "SIDE-READY" || { echo "S48: the side terminal lost its screen"; return 1; }
}

S49() {
  start_daemon && start_app || return 1
  local file="$KANNA_STATE_DIR/s48.rs"
  for i in $(seq 120); do echo "let line_$i = $i;"; done >"$file"
  code() { raw "{\"id\":1,\"cmd\":\"debug.code\",\"args\":{\"pane\":\"$1\"}}" | field "$2"; }
  kanna open code --name src "$file:80" >/dev/null || return 1
  check wait_for 5 eval '[[ "$(code src current)" == 80 ]]' || return 1
  check eval '(( $(code src top) > 50 && $(code src top) < 80 ))' || { echo "top $(code src top)"; return 1; }
  check is src line 80 || return 1
  check test "$(code src language)" == rust || return 1
  check wait_for 5 eval '(( $(code src spans) > 300 ))' || { echo "spans $(code src spans)"; return 1; }
  echo 'let s48_edit = 1;' >>"$file"
  check wait_for 5 eval 'code src text | grep -q s48_edit' || return 1
  check wait_for 5 eval '[[ "$(code src changed)" == 121 ]]' || { echo "changed $(code src changed)"; return 1; }
  raw '{"id":1,"cmd":"debug.click","args":{"pane":"src","at":[120,200]}}' >/dev/null; sleep 0.3
  key() { raw "{\"id\":1,\"cmd\":\"debug.key\",\"args\":{\"key\":\"$1\"}}" >/dev/null; }
  key cmd+shift+=; key cmd+shift+=
  key cmd+f
  check wait_for 5 eval '[[ "$(code src find_bar)" == 1 ]]' || return 1
  raw '{"id":1,"cmd":"debug.type","args":{"focused":true,"text":"line_77 "}}' >/dev/null; key return
  check wait_for 5 eval '[[ "$(code src selection_line)" == 77 && "$(code src selection_length)" == 8 ]]' || { echo "found at $(code src selection_line)"; return 1; }
  key escape
  check wait_for 5 eval '[[ "$(code src font_size)" == 14.5 ]]' || { echo "font $(code src font_size)"; return 1; }
  kanna navigate src "$file:10" >/dev/null || return 1
  check wait_for 5 eval '[[ "$(code src current)" == 10 ]]' || return 1
  kanna navigate src --back >/dev/null || return 1
  check wait_for 5 eval '[[ "$(code src current)" == 80 ]]' || return 1
  press() { raw "{\"id\":1,\"cmd\":\"debug.type\",\"args\":{\"focused\":true,\"text\":\"$1\"}}" >/dev/null; sleep 0.2; }
  press gg; check wait_for 5 eval '[[ "$(code src top)" == 1 ]]' || { echo "gg top $(code src top)"; return 1; }
  press 3j; press 5j; press k; check wait_for 5 eval '[[ "$(code src top)" == 8 ]]' || { echo "j k top $(code src top)"; return 1; }
  press G; check wait_for 5 eval '(( $(code src top) > 80 && $(code src top) < 100 ))' || { echo "G top $(code src top)"; return 1; }
  press b; check wait_for 5 eval '(( $(code src top) < 70 ))' || { echo "b top $(code src top)"; return 1; }
  check wait_for 5 eval '(( $(code src top) < 80 ))' || { echo "b top $(code src top)"; return 1; }
  [[ "$(kanna open code "$KANNA_STATE_DIR/gone.rs" 2>&1)" == *"No such file: $KANNA_STATE_DIR/gone.rs"* ]] || { echo "S49: a missing file"; return 1; }
  [[ "$(kanna open code "$KANNA_STATE_DIR" 2>&1)" == *"is a folder, not a file"* ]] || { echo "S49: a folder"; return 1; }
  local nested="$KANNA_STATE_DIR/s48-nested.rs"
  { echo 'fn outer() {'; echo '    loop {'; echo '        if ready() {'; for i in $(seq 4 60); do echo "            step($i);"; done; echo '        }'; echo '    }'; echo '}'; } >"$nested"
  kanna open code --name nested "$nested" --tab >/dev/null || return 1
  check wait_for 5 eval '(( $(code nested scopes) == 3 ))' || { echo "scopes $(code nested scopes)"; return 1; }
  raw '{"id":1,"cmd":"debug.code","args":{"pane":"nested","pick":40}}' >/dev/null
  check wait_for 5 eval '[[ "$(code nested top)" == 37 && "$(code nested pinned)" == 3 ]]' || { echo "top $(code nested top), pinned $(code nested pinned)"; return 1; }
  check test "$(code nested indent_unit)" == 4 || return 1
  raw '{"id":1,"cmd":"debug.click","args":{"pane":"nested","at":[300,300]}}' >/dev/null; sleep 0.3
  press gg; press ]]; press ]]; check wait_for 5 eval '[[ "$(code nested current)" == 3 ]]' || { echo "]] ]] current $(code nested current)"; return 1; }
  press [[; check wait_for 5 eval '[[ "$(code nested current)" == 2 ]]' || { echo "[[ current $(code nested current)"; return 1; }
  press G; press }; check wait_for 5 eval '[[ "$(code nested current)" == 63 ]]' || { echo "} current $(code nested current)"; return 1; }
  stop_app; start_app || return 1
  check wait_for 5 eval '[[ "$(code src current)" == 80 ]]' || return 1
  check eval 'code src text | grep -q s48_edit' || return 1
  check test "$(code src font_size)" == 14.5 || { echo "font after relaunch $(code src font_size)"; return 1; }
}

S50() {
  start_daemon && start_app || return 1
  local tour="$KANNA_STATE_DIR/s50.rs" other="$KANNA_STATE_DIR/s50-other.rs"
  { echo 'fn outer() {'; echo '    loop {'; echo '        if ready() {'; for i in $(seq 4 60); do echo "            step($i);"; done; echo '        }'; echo '    }'; echo '}'; } >"$tour"
  for i in $(seq 30); do echo "let line_$i = $i;"; done >"$other"
  code() { raw "{\"id\":1,\"cmd\":\"debug.code\",\"args\":{\"pane\":\"$1\"}}" | field "$2"; }
  local said id
  said="$(kanna comment "$tour:3" "Waits until it's ready.")" || return 1
  [[ "$said" == "opened p"*" · s50.rs:3–61" ]] || { echo "said: $said"; return 1; }
  id="$(awk '{print $2}' <<<"$said")"
  check wait_for 5 eval '[[ "$(code $id comment_first)" == 3 && "$(code $id comment_last)" == 61 ]]' || return 1
  check wait_for 5 eval '[[ "$(code $id comment_text)" == "Waits until it'"'"'s ready." ]]' || return 1
  check eval '(( $(code $id comment_gap) > 20 ))' || { echo "gap $(code $id comment_gap)"; return 1; }
  check wait_for 5 eval '[[ "$(code $id comment_box_visible)" == 1 ]]' || { echo "box at $(code $id comment_box_visible)"; return 1; }
  said="$(kanna comment "$tour:40-42" "Each step.")" || return 1
  [[ "$said" == "$id · s50.rs:40–42" ]] || { echo "again: $said"; return 1; }
  check wait_for 5 eval '[[ "$(code $id comment_first)" == 40 && "$(code $id comment_last)" == 42 ]]' || return 1
  check wait_for 5 eval '[[ "$(code $id comment_box_visible)" == 1 ]]' || return 1
  said="$(kanna comment "$other:7" "Line seven.")" || return 1
  [[ "$said" == "opened p"*" · s50-other.rs:7" && "$said" != *" $id "* ]] || { echo "other: $said"; return 1; }
  [[ "$(kanna comment "$KANNA_STATE_DIR/gone.rs:3" "Nothing." 2>&1)" == *"No such file: $KANNA_STATE_DIR/gone.rs"* ]] || { echo "S50: a missing file"; return 1; }
  [[ "$(kanna comment "$other:99" "Past the end." 2>&1)" == *"has 30 lines"* ]] || { echo "S50: past the end"; return 1; }
  local doc="$KANNA_STATE_DIR/s50.md" agent md
  printf '# Tabs\n\nA tab container holds **several** panes\nand shows one at a time.\n\n- Panes only: it holds panes.\n- One active: the rest keep running.\n' >"$doc"
  agent="$(kanna open --name s50agent --cmd cat | awk '{print $2}')"
  said="$(KANNA_PANE=$agent kanna comment "$doc:4" "The key idea: one pane shows at a time.")" || return 1
  [[ "$said" == "c1 · s50.md:3 in p"* ]] || { echo "doc: $said"; return 1; }
  md="${said##* in }"
  mdf() { raw "{\"id\":1,\"cmd\":\"debug.md\",\"args\":{\"pane\":\"$md\"}}" | field "$1"; }
  check wait_for 5 eval '[[ "$(mdf threads.0.messages.0)" == "agent: The key idea: one pane shows at a time." ]]' || return 1
  check wait_for 5 eval '[[ "$(mdf markup)" == True && "$(mdf agent)" == "$agent" ]]' || { echo "markup $(mdf markup) agent $(mdf agent)"; return 1; }
  check wait_for 5 eval 'raw "{\"id\":1,\"cmd\":\"debug.md\",\"args\":{\"pane\":\"$md\",\"mark\":\"c1\"}}" | grep -q "\"y\""' || { echo "c1 has no mark"; return 1; }
  said="$(KANNA_PANE=$agent kanna comment "$doc:7" "The others keep running.")" || return 1
  [[ "$said" == "c2 · s50.md:7 in $md" ]] || { echo "doc again: $said"; return 1; }
  check wait_for 5 eval '[[ "$(mdf active)" == c2 ]]' || { echo "active $(mdf active)"; return 1; }
  stop_app; start_app || return 1
  check wait_for 5 eval '[[ "$(code $id comment_first)" == 40 && "$(code $id comment_text)" == "Each step." ]]' || return 1
}

S52() {
  local repo="$KANNA_STATE_DIR/repo"
  mkdir -p "$repo" && git -C "$repo" init -q -b main && git -C "$repo" -c user.email=e@e -c user.name=e commit -q --allow-empty -m first || { echo "S52 line $LINENO"; return 1; }
  repo="$(cd "$repo" && pwd -P)"
  start_daemon && start_app || return 1
  kanna open --name plain >/dev/null || { echo "S52 line $LINENO"; return 1; }
  check wait_for 5 eval '[[ "$(win w1 key)" == True ]]' || { echo "S52 line $LINENO"; return 1; }
  desk() { raw "{\"id\":1,\"cmd\":\"debug.desk\",\"args\":{\"window\":\"$2\"}}" | field "$1"; }
  key cmd+n
  check wait_for 5 eval '[[ "$(state | field windows | count)" == 2 && "$(win w2 key)" == True ]]' || { echo "S52: ⌘N opened no window"; kanna list; return 1; }
  check test "$(desk desk w2)" == True || { echo "S52: ⌘N opened a window that isn't a Desk"; return 1; }
  check test "$(desk window.project w2)" == "$HOME" || { echo "S52: the new Desk isn't on ~"; desk window w2; return 1; }
  check wait_for 10 eval '[[ "$(desk planner w2)" == p* ]]' || { echo "S52: the new Desk has no planning terminal"; kanna list; return 1; }
  local planner; planner="$(desk planner w2)"
  check test "$(desk window.dock w2)" == None || { echo "S52: the new Desk docked something"; return 1; }
  check test "$(desk window.tabs w2)" == "[]" || { echo "S52: the new Desk has tabs"; return 1; }
  check wait_for 5 is "$planner" state running || { echo "S52: the planning terminal isn't running"; kanna list; return 1; }
  check test "$(cd "$(pane "$planner" cwd)" && pwd -P)" == "$(cd ~ && pwd -P)" || { echo "S52: the planning terminal isn't in ~"; return 1; }
  check wait_for 10 shows "$planner" "This terminal is for planning" || { kanna read "$planner"; return 1; }
  check test "$(kanna task list | grep -c "$planner")" == 0 || { echo "S52: the planning terminal is listed as a task"; return 1; }
  check test "$(raw '{"id":1,"cmd":"debug.stats"}' | field pid)" == "$app_pid" || { echo "S52: the new Desk is in another instance"; return 1; }
  kanna send "$planner" "cd '$repo'" >/dev/null || { echo "S52 line $LINENO"; return 1; }
  check wait_for 10 eval '[[ "$(desk header w2)" == "Planning · "*"${repo#/private} · main" ]]' || { desk header w2; return 1; }
  (cd "$repo" && KANNA_PANE="$planner" kanna task new "Here") >/dev/null || { echo "S52 line $LINENO"; return 1; }
  check wait_for 5 eval '[[ "$(desk backlog w2)" == "[\"t1\"]" ]]' || { echo "S52: the Shelf didn't follow the planning terminal"; desk backlog w2; return 1; }
  raw '{"id":1,"cmd":"debug.menu","args":{"item":"New Desk"}}' | grep -q '"ok":true' || { echo "S52: File > New Desk didn't run"; return 1; }
  check wait_for 5 eval '[[ "$(state | field windows | count)" == 3 && "$(desk desk w3)" == True ]]' || { echo "S52: New Desk opened no Desk"; kanna list; return 1; }
  check wait_for 10 eval '[[ "$(desk planner w3)" == p* ]]' || { echo "S52 line $LINENO"; return 1; }
  check test "$(cd "$(pane "$(desk planner w3)" cwd)" && pwd -P)" == "$(cd ~ && pwd -P)" || { echo "S52: the second new Desk isn't in ~"; return 1; }
}

png_size() { python3 -c 'import struct, sys
d = open(sys.argv[1], "rb").read(24)
assert d[:8] == b"\x89PNG\r\n\x1a\n"
print("%dx%d" % struct.unpack(">II", d[16:24]))' "$1"; }
png_ink() { python3 -c 'import struct, sys, zlib, collections
d = open(sys.argv[1], "rb").read(); i = 8; idat = b""
while i < len(d):
    n, kind = struct.unpack(">I4s", d[i:i + 8]); body = d[i + 8:i + 8 + n]; i += 12 + n
    if kind == b"IHDR": w, h, depth, color = struct.unpack(">IIBB", body[:10])
    if kind == b"IDAT": idat += body
assert depth == 8 and color in (2, 6)
bpp = 3 if color == 2 else 4; raw = zlib.decompress(idat); stride = w * bpp; prev = bytearray(stride); rows = []
for y in range(h):
    f = raw[y * (stride + 1)]; line = bytearray(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
    for x in range(stride):
        a = line[x - bpp] if x >= bpp else 0; b = prev[x]; c = prev[x - bpp] if x >= bpp else 0
        if f == 1: line[x] = (line[x] + a) & 255
        elif f == 2: line[x] = (line[x] + b) & 255
        elif f == 3: line[x] = (line[x] + (a + b) // 2) & 255
        elif f == 4:
            p = a + b - c; pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
            line[x] = (line[x] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
    rows.append(line); prev = line
px = [tuple(r[x:x + 3]) for r in rows[::2] for x in range(0, stride, bpp * 2)]
bg = collections.Counter(px).most_common(1)[0][0]
print(round(sum(1 for p in px if sum(abs(p[k] - bg[k]) for k in range(3)) > 60) / len(px), 4))' "$1"; }

S62() {
  start_daemon || return 1
  env -u KANNA_DEBUG -u KANNA_IGNORE_OCCLUSION "$app" --bg >>"$out/app.log" 2>&1 &
  app_pid=$!
  check wait_for 10 eval '[[ "$(kanna capabilities)" == *"Kanna.app: attached"* ]]' || return 1
  local shots="$KANNA_STATE_DIR/shots"; mkdir -p "$shots" && shots="$(cd "$shots" && pwd -P)"
  kanna open --name s62 --cmd 'for i in $(seq 80); do echo "S62 $i the quick brown fox jumps over the lazy dog"; done; exec cat' >/dev/null || return 1
  check wait_for 5 shows s62 "S62 80" || return 1
  printf '# S62\n\nA markdown pane.\n' >"$KANNA_STATE_DIR/s62.md"
  kanna open md "$KANNA_STATE_DIR/s62.md" --name doc --split right >/dev/null || return 1
  check wait_for 5 is doc state running || return 1
  raw '{"id":1,"cmd":"debug.app"}' | grep -q '"unknown command' || { echo "S62: the app answers debug commands"; return 1; }
  local window pane doc
  window="$(cd "$shots" && kanna screenshot --window w1 -o w1.png)" || { echo "S62 line $LINENO"; return 1; }
  check test "$window" == "$shots/w1.png" || return 1
  pane="$(cd "$shots" && kanna screenshot s62 --out s62.png)" || { echo "S62 line $LINENO"; return 1; }
  doc="$(kanna screenshot doc --json)" || { echo "S62 line $LINENO"; return 1; }
  cp "$shots"/*.png "$out/" 2>/dev/null
  local ww wh pw ph; IFS=x read -r ww wh <<<"$(png_size "$shots/w1.png")"; IFS=x read -r pw ph <<<"$(png_size "$shots/s62.png")"
  check test "$ww" -ge 800 -a "$wh" -ge 400 || return 1
  check test $((pw * 2 - ww)) -ge -4 -a $((pw * 2 - ww)) -le 4 || { echo "S62: pane $pw wide in a window $ww wide"; return 1; }
  check test "$ph" -lt "$wh" -a "$ph" -gt $((wh / 2)) || return 1
  check python3 -c 'import json, sys
r = json.loads(sys.argv[1]); assert r["pane"] == "p2" and r["window"] == "w1" and r["path"].endswith(".png") and r["width"] > 0, r' "$doc" || return 1
  check test "$(png_size "$(echo "$doc" | field path)")" == "$(echo "$doc" | field width)x$(echo "$doc" | field height)" || return 1
  local ink; ink="$(png_ink "$shots/s62.png")"
  check python3 -c 'import sys; assert 0.01 < float(sys.argv[1]) < 0.5, sys.argv[1]' "$ink" || { echo "S62: the terminal pane isn't legible (ink $ink)"; return 1; }
  (cd "$shots" && kanna screenshot s62 -o s62.png) 2>/dev/null && { echo "S62: it overwrote s62.png"; return 1; }
  (cd "$shots" && kanna screenshot s62 -o s62.png --force) >/dev/null || { echo "S62 line $LINENO"; return 1; }
  kanna open --tab --name other >/dev/null || return 1
  local hidden; hidden="$(kanna screenshot s62 2>&1)" && { echo "S62: shot a pane that isn't showing"; return 1; }
  [[ "$hidden" == *"is in tab t1, which isn't showing"* ]] || { echo "S62: $hidden"; return 1; }
  local caller; caller="$(KANNA_PANE="$(pane other id)" kanna screenshot --json | field window)" || return 1
  check test "$caller" == w1
}

S42() {
  SHELL=/bin/bash PATH="$bin:$PATH" start_daemon && start_app || return 1
  kanna open --name s42 >/dev/null || return 1
  check wait_for 10 eval '[[ -n "$(kanna read s42 | tr -d "[:space:]")" ]]' || return 1
  sleep 2
  kanna send s42 'echo "s42:$(complete -p kanna):${PROMPT_COMMAND-none}:${KANNA_BASH_INIT-gone}"' || return 1
  check wait_for 5 shows s42 's42:complete -o filenames -F _kanna kanna:'
  kanna read s42 | grep -E '^s42:' | grep -qF ':gone' || { kanna read s42 | tail -4; return 1; }
}

S51() {
  local agent="$repo_root/tools/mock-agent/mock-agent" repo="$KANNA_STATE_DIR/repo"
  mkdir -p "$repo" && git -C "$repo" init -q -b main && git -C "$repo" -c user.email=e@e -c user.name=e commit -q --allow-empty -m first || { echo "S51 line $LINENO"; return 1; }
  repo="$(cd "$repo" && pwd -P)"
  start_daemon || { echo "S51 line $LINENO"; return 1; }
  env -u KANNA_NO_INITIAL_WINDOW KANNA_START_DIR="$repo" "$app" --bg >>"$out/app.log" 2>&1 &
  app_pid=$!
  wait_for 10 app_attached || { echo "the app did not attach"; return 1; }
  desk() { raw "{\"id\":1,\"cmd\":\"debug.desk\",\"args\":{\"window\":\"w1\"${2:+,$2}}}" | field "$1"; }
  check wait_for 10 eval '[[ "$(desk planner)" == p* ]]' || { echo "S51: no planning terminal"; kanna list; return 1; }
  for i in $(seq 20); do (cd "$repo" && kanna task new "Task $i" "hello" --agent "$agent") >/dev/null || { echo "S51 line $LINENO"; return 1; }; done
  check wait_for 5 eval '[[ "$(desk backlog)" == *"t20"* && "$(desk page)" == True ]]' || { desk backlog; return 1; }
  desk page '"size":[760,420]' >/dev/null; sleep 0.3
  check eval '(( $(desk scroll.height) > $(desk scroll.visible) + 600 ))' || { desk scroll; return 1; }
  check test "$(desk scroll.top)" == 0 || { desk scroll; return 1; }
  [[ "$(desk scroll.backlog)" != *'"t20"'* ]] || { echo "S51: t20 fits a short window"; desk scroll; return 1; }
  raw '{"id":1,"cmd":"debug.click","args":{"backlog":"t20","window":"w1"}}' | grep -q 'scrolled out' || { echo "S51: clicked a row that isn't on screen"; return 1; }
  desk page '"scroll":100000' >/dev/null; sleep 0.3
  check eval '(( $(desk scroll.top) > 0 ))' || { desk scroll; return 1; }
  check eval '[[ "$(desk scroll.backlog)" == *"\"t20\""* ]]' || { echo "S51: t20 can't be scrolled to"; desk scroll; return 1; }
  raw '{"id":1,"cmd":"debug.app","args":{"appearance":"light"}}' >/dev/null; sleep 0.3; snap w1 s51-light >/dev/null
  raw '{"id":1,"cmd":"debug.app","args":{"appearance":"dark"}}' >/dev/null; sleep 0.3; snap w1 s51-dark >/dev/null
  local top; top="$(desk scroll.top)"
  raw '{"id":1,"cmd":"debug.click","args":{"backlog":"t20","window":"w1"}}' >/dev/null
  check wait_for 10 eval '[[ "$(desk backlog)" != *"t20"* && "$(desk shelf.0.title)" == "Task 20" ]]' || { desk backlog; desk shelf; return 1; }
  check eval '(( $(desk scroll.top) > 0 && $(desk scroll.top) <= top ))' || { echo "S51: the page lost its scroll position"; desk scroll; return 1; }
  check eval '(( $(desk scroll.top) + $(desk scroll.visible) <= $(desk scroll.height) ))' || { desk scroll; return 1; }
  (cd "$repo" && kanna task new "The Shelf strip shows every parked task's time beside its name" "hello" --agent "$agent" && kanna task new "Long titles truncate before the times and the strip never overflows" "hello" --agent "$agent") >/dev/null || { echo "S51 line $LINENO"; return 1; }
  kanna task shelve t21 >/dev/null && kanna task shelve t22 >/dev/null || { echo "S51 line $LINENO"; return 1; }
  check wait_for 10 eval '[[ "$(desk running)" == 3 ]]' || { desk running; return 1; }
  kanna task pull "Task 20" >/dev/null || { echo "S51 line $LINENO"; return 1; }
  check wait_for 10 eval '[[ "$(desk strip)" == True && "$(desk title)" == "Task 20" ]]' || { desk stripLayout; return 1; }
  fits() { desk stripLayout "$1" | python3 -c 'import json, sys; g = json.load(sys.stdin); items = g["items"]
sys.exit(not (len(items) == 2 and all(i["meta"].startswith("· parked ") for i in items) and all(i["right"] <= g["width"] for i in items)))'; }
  check eval 'fits "\"size\":[1400,420]"' || { desk stripLayout; return 1; }
  check test "$(desk waiting)" == "19 in backlog" || { desk waiting; return 1; }
  check eval 'fits "\"size\":[520,420]"' || { desk stripLayout; return 1; }
  check test "$(desk waiting)" == "19 in backlog" || { echo "S51: the backlog count gave way before the titles"; desk stripLayout; return 1; }
  check eval 'fits "\"size\":[400,420]"' || { desk stripLayout; return 1; }
  check test "$(desk waiting)" == "" || { echo "S51: the backlog count didn't give way on a narrow strip"; desk stripLayout; return 1; }
  sleep 0.3; snap w1 s51-strip >/dev/null
}

S53() {
  local agent="$repo_root/tools/mock-agent/mock-agent" repo="$KANNA_STATE_DIR/repo"
  mkdir -p "$repo" && git -C "$repo" init -q -b main && git -C "$repo" -c user.email=e@e -c user.name=e commit -q --allow-empty -m first || { echo "S53 line $LINENO"; return 1; }
  repo="$(cd "$repo" && pwd -P)"
  start_daemon || { echo "S53 line $LINENO"; return 1; }
  env -u KANNA_NO_INITIAL_WINDOW KANNA_START_DIR="$repo" "$app" --bg >>"$out/app.log" 2>&1 &
  app_pid=$!
  wait_for 10 app_attached || { echo "the app did not attach"; return 1; }
  desk() { raw '{"id":1,"cmd":"debug.desk","args":{"window":"w1"}}' | field "$1"; }
  search() { desk search."$1"; }
  type() { raw "{\"id\":1,\"cmd\":\"debug.type\",\"args\":{\"focused\":true,\"text\":\"$1\"}}" >/dev/null; sleep 0.3; }
  check wait_for 10 eval '[[ "$(desk planner)" == p* ]]' || { echo "S53: no planning terminal"; kanna list; return 1; }
  check test "$(search shown)" == False || { echo "S53: a search field over an empty backlog"; return 1; }
  (cd "$repo" && kanna task new "Fix flaky status test" "S40 waits for working; follow the status" --agent "$agent" \
    && kanna task new "Resume every agent's session" "Give codex its own session ID and resume it" --agent "$agent" \
    && kanna task new "Shelf strip shows parked time" "Show how long ago each item was parked" --agent "$agent" \
    && kanna task new "Start tasks past the Shelf" "Let a fourth task wait for a slot" --agent "$agent") >/dev/null || { echo "S53 line $LINENO"; return 1; }
  for i in $(seq 5 12); do (cd "$repo" && kanna task new "Task $i" "hello" --agent "$agent") >/dev/null || { echo "S53 line $LINENO"; return 1; }; done
  check wait_for 5 eval '[[ "$(desk backlog)" == *"t12"* && "$(search shown)" == True ]]' || { desk backlog; desk search; return 1; }
  key cmd+f
  check test "$(search focused)" == True || { echo "S53: ⌘F didn't reach the backlog search"; desk responder; return 1; }
  local at; at="$(search left),$(search right)"
  type shelf
  check test "$(search left),$(search right)" == "$at" || { echo "S53: the search field moved as the count changed"; desk search; return 1; }
  check test "$(search found)" == '["t3", "t4"]' || { desk search; return 1; }
  check test "$(search selected)" == 0 || { desk search; return 1; }
  type " parked"
  check test "$(search found)" == '["t3"]' || { desk search; return 1; }
  key escape
  check test "$(search query)" == "" || { echo "S53: esc didn't clear the search"; desk search; return 1; }
  check test "$(search found | grep -o '"t' | wc -l | tr -d ' ')" == 12 || { desk search; return 1; }
  check test "$(search focused)" == True || { desk search; return 1; }
  key escape
  check test "$(search focused)" == False || { echo "S53: a second esc stayed in the search"; desk search; return 1; }
  check test "$(desk responder)" == TerminalSurfaceView || { echo "S53: a second esc didn't go back to the planning terminal"; desk responder; return 1; }
  key cmd+f; type "RESUME codex"
  check test "$(search found)" == '["t2"]' || { echo "S53: no match on a first message, ignoring case"; desk search; return 1; }
  type " nothing"
  check test "$(search found)" == '[]' || { desk search; return 1; }
  key escape; type shelf
  raw '{"id":1,"cmd":"debug.app","args":{"appearance":"light"}}' >/dev/null; sleep 0.3; snap w1 s53-light >/dev/null
  raw '{"id":1,"cmd":"debug.app","args":{"appearance":"dark"}}' >/dev/null; sleep 0.3; snap w1 s53-dark >/dev/null
  key down
  check test "$(search selected)" == 1 || { desk search; return 1; }
  key down
  check test "$(search selected)" == 1 || { echo "S53: the selection ran past the last match"; desk search; return 1; }
  key up
  check test "$(search selected)" == 0 || { desk search; return 1; }
  key down; key return
  check wait_for 10 eval '[[ "$(desk backlog)" != *"t4"* && "$(desk shelf.0.title)" == "Start tasks past the Shelf" ]]' || { desk backlog; desk shelf; desk note; return 1; }
  check wait_for 5 eval '[[ "$(search query)" == "" ]]' || { echo "S53: the search stayed after starting its task"; desk search; return 1; }
  local listed; listed="$(cd "$repo" && kanna task list shelf)"
  check test "$(wc -l <<<"$listed" | tr -d ' ')" == 3 || { echo "$listed"; return 1; }
  [[ "$listed" == *"Start tasks past the Shelf"*"Shelf"* && "$listed" == *"t3"*"Backlog"* ]] || { echo "S53: kanna task list QUERY"; echo "$listed"; return 1; }
}

S54() {
  start_daemon && start_app || { echo "S54 line $LINENO"; return 1; }
  local repo="$KANNA_STATE_DIR/repo" agent="$KANNA_STATE_DIR/bin/claude"
  mkdir -p "$repo" "${agent%/*}" && ln -s "$repo_root/tools/mock-agent/mock-agent" "$agent" || { echo "S54 line $LINENO"; return 1; }
  git -C "$repo" init -q -b main && git -C "$repo" -c user.email=e@e -c user.name=e commit -q --allow-empty -m first || { echo "S54 line $LINENO"; return 1; }
  repo="$(cd "$repo" && pwd -P)"
  local n listed
  for n in 1 2 3 4 5; do (cd "$repo" && kanna task new "Task $n" "say $n" --agent "$agent") >/dev/null || { echo "S54 line $LINENO"; return 1; }; done
  for n in 1 2 3 4 5; do
    [[ "$(kanna task start t$n)" == "started t$n in the backlog: p"* ]] || { echo "S54: t$n didn't start in the backlog"; return 1; }
  done
  check wait_for 10 eval '[[ "$(cd "$repo" && kanna task list | grep -cE "^t[1-5] +Task [1-5] +Backlog +(idle|working|running)")" == 5 ]]' || { (cd "$repo" && kanna task list); return 1; }
  for n in 1 2 3 4 5; do check wait_for 5 shows "Task $n" "say $n" || { echo "S54: Task $n's agent didn't get its first message"; return 1; }; done
  [[ "$(kanna task start t1 2>&1)" == *"t1 is running"* ]] || { echo "S54 line $LINENO"; return 1; }
  for n in 1 2 3; do kanna task shelve t$n >/dev/null || { echo "S54: t$n didn't move to the Shelf"; return 1; }; done
  [[ "$(kanna task shelve t4 2>&1)" == *"has 3 tasks on the Shelf"* ]] || { echo "S54: a fourth task went to the Shelf"; return 1; }
  listed="$(cd "$repo" && kanna task list)"
  [[ "$(grep -cE ' Shelf ' <<<"$listed")" == 3 && "$(grep -cE ' Backlog ' <<<"$listed")" == 2 ]] || { echo "$listed"; return 1; }
  local task session; task="$(pane "Task 1" id)"
  session="$(kanna read "Task 1" | tr -d '\r\n' | sed -n 's/.*--session-id \([0-9a-f-]\{36\}\).*/\1/p')"
  [[ "$session" =~ ^[0-9a-f-]{36}$ ]] || { echo "S54: Task 1 started without a session"; kanna read "Task 1"; return 1; }
  KANNA_PANE="$task" kanna open --name side --cmd 'exec cat' >/dev/null || { echo "S54 line $LINENO"; return 1; }
  expected="stopped $task; \`kanna task start $task\` starts it again"; says kanna task stop "Task 1" || { echo "S54 line $LINENO"; return 1; }
  check is "Task 1" state exited || { echo "S54: Task 1's agent is still running"; return 1; }
  kanna list --json | grep -q '"side"' && { echo "S54: Task 1's tab is still open"; return 1; }
  (cd "$repo" && kanna task list) | grep -qE "^$task +Task 1 +Shelf +stopped" || { echo "S54: Task 1 left the Shelf"; (cd "$repo" && kanna task list); return 1; }
  [[ "$(kanna task stop "Task 1" 2>&1)" == *"is stopped"* ]] || { echo "S54 line $LINENO"; return 1; }
  [[ "$(kanna task shelve t4 2>&1)" == *"has 3 tasks on the Shelf"* ]] || { echo "S54: a stopped task stopped counting on the Shelf"; return 1; }
  expected="started $task again: $task"; says kanna task start "Task 1" || { echo "S54 line $LINENO"; return 1; }
  check wait_for 5 eval '[[ "$(kanna read "Task 1" | tr -d "\r\n")" == *"--resume $session"* ]]' || { echo "S54: Task 1 didn't resume its conversation"; kanna read "Task 1"; return 1; }
  (cd "$repo" && kanna task list) | grep -qE "^$task +Task 1 +Shelf +(idle|working|running)" || { (cd "$repo" && kanna task list); return 1; }
  [[ "$(kanna task stop t5)" == "stopped t5 in the backlog; "* ]] || { echo "S54 line $LINENO"; return 1; }
  (cd "$repo" && kanna task list) | grep -qE '^t5 +Task 5 +Backlog +stopped' || { (cd "$repo" && kanna task list); return 1; }
  [[ "$(kanna task start t5)" == "started t5 again in the backlog: p"* ]] || { echo "S54 line $LINENO"; return 1; }
  check wait_for 5 shows "Task 5" "--resume" || { echo "S54: t5 didn't resume"; kanna read "Task 5"; return 1; }
  stop_app; stop_daemon; start_daemon && start_app || { echo "S54 line $LINENO"; return 1; }
  kanna task stop t4 >/dev/null || { echo "S54 line $LINENO"; return 1; }
  stop_app; stop_daemon; start_daemon && start_app || { echo "S54 line $LINENO"; return 1; }
  (cd "$repo" && kanna task list) | grep -qE '^t4 +Task 4 +Backlog +stopped' || { echo "S54: a stopped task didn't survive a restart"; (cd "$repo" && kanna task list); return 1; }
  kanna task start t4 >/dev/null && check wait_for 5 shows "Task 4" "--resume" || { echo "S54: t4 didn't resume after a restart"; return 1; }
}

S55() {
  start_daemon && start_app || return 1
  local file="$KANNA_STATE_DIR/s55.rs"
  nested() { echo 'fn outer() {'; echo '    loop {'; echo "        if $1() {"; for i in $(seq 4 60); do echo "            step($i);"; done; echo '        }'; echo '    }'; echo '}'; }
  nested ready >"$file"
  code() { raw "{\"id\":1,\"cmd\":\"debug.code\",\"args\":{\"pane\":\"$1\"}}" | field "$2"; }
  key() { raw "{\"id\":1,\"cmd\":\"debug.key\",\"args\":{\"key\":\"$1\"}}" >/dev/null; }
  kanna open code --name src "$file" >/dev/null || return 1
  check wait_for 5 eval '(( $(code src scopes) == 3 ))' || { echo "scopes $(code src scopes)"; return 1; }
  nested ready_now >"$file"
  check wait_for 5 eval '[[ "$(code src changed)" == 3 ]]' || { echo "changed $(code src changed)"; return 1; }
  raw '{"id":1,"cmd":"debug.click","args":{"pane":"src","at":[300,300]}}' >/dev/null; sleep 0.3
  key cmd+f
  check wait_for 5 eval '[[ "$(code src find_bar)" == 1 ]]' || return 1
  raw '{"id":1,"cmd":"debug.type","args":{"focused":true,"text":"ready"}}' >/dev/null
  raw '{"id":1,"cmd":"debug.code","args":{"pane":"src","select":[3,11,5]}}' >/dev/null
  check wait_for 5 eval '[[ "$(code src selection_line)" == 3 && "$(code src selection_length)" == 5 ]]' || return 1
  raw '{"id":1,"cmd":"debug.code","args":{"pane":"src","pick":40}}' >/dev/null
  check wait_for 5 eval '[[ "$(code src pinned)" == 3 && "$(code src pinned_rows.2.line)" == 3 ]]' || { echo "pinned $(code src pinned_rows)"; return 1; }
  check wait_for 5 eval '[[ "$(code src pinned_rows.2.matches)" == "[[11, 5]]" ]]' || { echo "matches $(code src pinned_rows)"; return 1; }
  check test "$(code src pinned_rows.2.selection)" == "[[11, 5]]" || { echo "selection $(code src pinned_rows)"; return 1; }
  check test "$(code src pinned_rows.2.changed)" == True || { echo "changed $(code src pinned_rows)"; return 1; }
  for row in 0 1; do
    check test "$(code src pinned_rows.$row.selection)$(code src pinned_rows.$row.matches)$(code src pinned_rows.$row.changed)" == "[][]False" || { echo "row $row $(code src pinned_rows)"; return 1; }
  done
  raw '{"id":1,"cmd":"debug.code","args":{"pane":"src","select":[2,4,4]}}' >/dev/null
  check wait_for 5 eval '[[ "$(code src pinned_rows.1.selection)" == "[[4, 4]]" && "$(code src pinned_rows.2.selection)" == "[]" ]]' || { echo "reselect $(code src pinned_rows)"; return 1; }
  key escape
  check wait_for 5 eval '[[ "$(code src pinned_rows.2.matches)" == "[]" ]]' || { echo "after find $(code src pinned_rows)"; return 1; }
}

S56() {
  start_daemon || return 1
  kanna open --name bytes --cmd 'stty raw -echo; printf "READY\r\n"; while :; do printf "%s " "$(dd bs=1 count=1 2>/dev/null | od -An -tx1 | tr -d " \n")"; done' >/dev/null || return 1
  check wait_for 5 shows bytes READY || return 1
  kanna send-key bytes up ctrl+c return esc shift+tab || return 1
  check wait_for 5 shows bytes "1b 5b 41 03 0d 1b 1b 5b 5a" || { kanna read bytes; return 1; }
  kanna open --name agent --cmd "$KANNA_AGENT" >/dev/null || return 1
  check wait_for 5 shows agent "type turn" || return 1
  kanna send agent ask || return 1
  check wait_for 5 shows agent "proceed?" || return 1
  kanna send-key agent down return || return 1
  check wait_for 5 shows agent "Declined." || { kanna read agent; return 1; }
  kanna send agent turn || return 1
  check wait_for 10 shows agent "proceed?" || return 1
  kanna send-key agent down up return || return 1
  check wait_for 5 shows agent TURN_DONE || { kanna read agent; return 1; }
  kanna send agent ask && check wait_for 5 shows agent "proceed?" || return 1
  kanna send agent 2 || return 1
  check wait_for 5 shows agent "Declined." || { kanna read agent; return 1; }
  ! shows agent "not in application cursor mode" || { echo "an arrow was sent in normal cursor mode"; kanna read agent; return 1; }
  [[ "$(kanna send-key agent cmd+c 2>&1)" == *"cmd+c only means something to Kanna.app"* ]] || { echo "cmd+c wasn't refused"; return 1; }
}

S57() {
  local claude="${KANNA_E2E_CLAUDE:-$(command -v claude)}" codex="${KANNA_E2E_CODEX:-$(command -v codex)}"
  [[ -x "$claude" ]] || { echo "S57: claude isn't installed (or set KANNA_E2E_CLAUDE)"; return 1; }
  [[ -x "$codex" ]] || { echo "S57: codex isn't installed (or set KANNA_E2E_CODEX)"; return 1; }
  start_daemon && start_app || { echo "S57 line $LINENO"; return 1; }
  local agents="$KANNA_STATE_DIR/agents" repo="$KANNA_STATE_DIR/repo" harness="$repo_root/tools/agent-harness"
  mkdir -p "$agents" "$repo" && git -C "$repo" init -q -b main && git -C "$repo" -c user.email=e@e -c user.name=e commit -q --allow-empty -m first || { echo "S57 line $LINENO"; return 1; }
  repo="$(cd "$repo" && pwd -P)"
  python3 -I "$harness/mock_server.py" --log "$agents/mock.jsonl" >"$agents/port" 2>"$agents/mock.log" &
  check wait_for 10 eval '[[ -s "$agents/port" ]]' || { echo "S57: the mock model didn't start"; return 1; }
  local port; port="$(cat "$agents/port")"
  local agent real
  for agent in claude codex; do
    [[ $agent == claude ]] && real="$claude" || real="$codex"
    python3 -I "$harness/isolate.py" --agent $agent --real "$real" --home "$agents/$agent-home" --port "$port" --trust "$repo" --out "$agents/bin/$agent" || { echo "S57 line $LINENO"; return 1; }
  done
  asked() { python3 -c 'import json, sys
lines = [json.loads(l) for l in open(sys.argv[1])]
print(sum(1 for l in lines if l.get("turn") and sys.argv[2] in l["prompt"] and (len(sys.argv) < 4 or any(sys.argv[3] in h for h in l["history"]))))' "$agents/mock.jsonl" "$@"; }
  local id task first second
  for agent in claude codex; do
    first="RESUME_FIRST $agent" second="RESUME_SECOND $agent"
    id="$(cd "$repo" && kanna task new "$agent task" "$first" --agent "$agents/bin/$agent" | sed -n 's/^added \(t[0-9]*\).*/\1/p')"
    [[ "$id" =~ ^t[0-9]+$ ]] || { echo "S57: $agent's task wasn't added"; return 1; }
    kanna task start "$id" >/dev/null || { echo "S57: $agent's task didn't start"; return 1; }
    check wait_for 60 eval '[[ "$(asked "$first")" == 1 ]]' || { echo "S57: $agent's first message didn't reach the mock model"; kanna read "$agent task"; return 1; }
    check wait_for 30 shows "$agent task" "Agent status probe" || { echo "S57: $agent didn't show the mock's answer"; kanna read "$agent task"; return 1; }
    check wait_for 10 grep -rqs "Agent status probe" "$agents/$agent-home" || { echo "S57: $agent didn't save its answer"; return 1; }
    kanna task shelve "$id" >/dev/null || { echo "S57: $agent's task didn't move to the Shelf"; return 1; }
    task="$(pane "$agent task" id)"
    [[ "$(kanna task defer "$task")" == *"to the backlog as $id"* ]] || { echo "S57: $agent's task didn't move back to the backlog as $id"; return 1; }
    [[ "$(kanna task stop "$id")" == "stopped $id in the backlog; "* ]] || { echo "S57: $agent's task didn't stop"; return 1; }
    check is "$agent task" state exited || { echo "S57: $agent is still running"; return 1; }
    [[ "$(kanna task start "$id")" == "started $id again in the backlog: "* ]] || { echo "S57: $agent's task didn't start again"; return 1; }
    check wait_for 60 eval 'shows "$agent task" "$first" && shows "$agent task" "Agent status probe"' || { echo "S57: $agent didn't come back to its conversation"; kanna read "$agent task"; return 1; }
    sleep 3
    [[ "$(asked "$first")" == 1 ]] || { echo "S57: $agent started again with its first message instead of resuming"; return 1; }
    kanna send "$agent task" "$second" || { echo "S57 line $LINENO"; return 1; }
    kanna send-key "$agent task" return || { echo "S57 line $LINENO"; return 1; }
    check wait_for 60 eval '[[ "$(asked "$second" "$first")" -ge 1 ]]' || { echo "S57: $agent's next message didn't carry its conversation"; kanna read "$agent task"; tail -3 "$agents/mock.jsonl" | cut -c1-300; return 1; }
  done
  stop_agents
}

S58() {
  local agent="$repo_root/tools/mock-agent/mock-agent" repo="$KANNA_STATE_DIR/repo" planner task shot
  mkdir -p "$repo" && git -C "$repo" init -q -b main && git -C "$repo" -c user.email=e@e -c user.name=e commit -q --allow-empty -m first || { echo "S58 line $LINENO"; return 1; }
  repo="$(cd "$repo" && pwd -P)"
  start_daemon || { echo "S58 line $LINENO"; return 1; }
  env -u KANNA_NO_INITIAL_WINDOW KANNA_START_DIR="$repo" "$app" --bg >>"$out/app.log" 2>&1 &
  app_pid=$!
  wait_for 10 app_attached || { echo "the app did not attach"; return 1; }
  desk() { raw "{\"id\":1,\"cmd\":\"debug.desk\",\"args\":{\"window\":\"w1\"}}" | field "$1"; }
  check wait_for 10 eval '[[ "$(desk planner)" == p* ]]' || { echo "S58: no planning terminal"; kanna list; return 1; }
  planner="$(desk planner)"
  (cd "$repo" && kanna task new "Zoomed task" "hello" --agent "$agent") >/dev/null && kanna task shelve t1 >/dev/null || { echo "S58 line $LINENO"; return 1; }
  kanna task pull "Zoomed task" >/dev/null || { echo "S58 line $LINENO"; return 1; }
  task="$(win w1 dock.pane)"
  KANNA_PANE="$task" kanna open --name log --cmd "printf 'GET / 200\n'; exec sleep 600" >/dev/null || { echo "S58 line $LINENO"; return 1; }
  kanna open --name tests --split right --cmd "printf 'test result: ok\n'; exec sleep 600" >/dev/null || { echo "S58 line $LINENO"; return 1; }
  kanna focus log >/dev/null && key cmd+shift+return || { echo "S58 line $LINENO"; return 1; }
  check test "$(win w1 zoomed)" == log || { echo "S58: ⇧⌘↩ didn't zoom the task's pane"; kanna list; return 1; }
  shown_alone() { shot="$(snap w1 "$1")"; python3 -c 'import json, sys; s = json.loads(sys.argv[1]); p = s["panes"]; assert len(p) == 1 and p[0]["width"] == s["width"], s' "$shot"; }
  sleep 0.5; check shown_alone s58-task-zoomed || { echo "S58: the zoomed pane doesn't fill the window"; return 1; }
  key "cmd+."
  check wait_for 5 eval '[[ "$(win w1 dock)" == None ]]' || { echo "S58: ⌘. didn't park the task"; return 1; }
  check test "$(win w1 focused):$(win w1 zoomed)" == "$planner:None" || { echo "S58: the Shelf view came up zoomed"; kanna list; return 1; }
  check test "$(desk page)" == True || { echo "S58: no full Shelf beside the planner"; return 1; }
  key cmd+shift+return
  check test "$(win w1 zoomed)" == "$planner" || { echo "S58: ⇧⌘↩ didn't zoom the planner"; kanna list; return 1; }
  sleep 0.5; check shown_alone s58-planner-zoomed || { echo "S58: the zoomed planner doesn't fill the window"; return 1; }
  key "cmd+."
  check wait_for 5 eval '[[ "$(win w1 dock.pane)" == "$task" ]]' || { echo "S58: ⌘. didn't pull the task back"; return 1; }
  check test "$(win w1 focused):$(win w1 zoomed)" == "log:log" || { echo "S58: the task didn't come back zoomed where it was"; kanna list; return 1; }
  key "cmd+."
  check wait_for 5 eval '[[ "$(win w1 dock)" == None ]]' || { echo "S58 line $LINENO"; return 1; }
  check test "$(win w1 zoomed)" == "$planner" || { echo "S58: the Shelf view didn't come back zoomed"; kanna list; return 1; }
  key "cmd+."
  check wait_for 5 eval '[[ "$(win w1 dock.pane)" == "$task" ]]' || { echo "S58 line $LINENO"; return 1; }
  stop_app; start_app || { echo "S58 line $LINENO"; return 1; }
  check wait_for 10 eval '[[ "$(win w1 zoomed)" == log ]]' || { echo "S58: the task's zoom didn't survive a relaunch"; kanna list; return 1; }
  sleep 0.5; check shown_alone s58-task-relaunched || { echo "S58 line $LINENO"; return 1; }
  key "cmd+."
  check wait_for 5 eval '[[ "$(win w1 dock)" == None ]]' || { echo "S58 line $LINENO"; return 1; }
  check test "$(win w1 zoomed)" == "$planner" || { echo "S58: the Shelf's zoom didn't survive a relaunch"; kanna list; return 1; }
  kanna zoom "$planner" >/dev/null || { echo "S58 line $LINENO"; return 1; }
  check test "$(win w1 zoomed)" == None || { echo "S58: kanna zoom didn't turn the Shelf's zoom off"; return 1; }
  sleep 0.5; check drawn "$(snap w1 s58-shelf)" 1 || return 1
  key "cmd+."
  check wait_for 5 eval '[[ "$(win w1 zoomed)" == log ]]' || { echo "S58: the task lost its zoom to the Shelf's"; kanna list; return 1; }
  stop_agents
}

S59() {
  local repo="$KANNA_STATE_DIR/repo" agent="$KANNA_STATE_DIR/bin/claude" state="$KANNA_STATE_DIR"
  mkdir -p "$repo" "${agent%/*}" && ln -s "$repo_root/tools/mock-agent/mock-agent" "$agent" || { echo "S59 line $LINENO"; return 1; }
  git -C "$repo" init -q -b main && git -C "$repo" -c user.email=e@e -c user.name=e commit -q --allow-empty -m first || { echo "S59 line $LINENO"; return 1; }
  repo="$(cd "$repo" && pwd -P)"
  printf '{"counter": 3, "tasks": [{"id": "t3", "name": "From JSON", "message": "hi", "repo": "%s", "madeAt": 1700000000, "agent": "%s"}], "running": []}' "$repo" "$agent" >"$state/backlog.json"
  start_daemon && start_app || { echo "S59 line $LINENO"; return 1; }
  [[ -f "$state/kanna.db" && -f "$state/backlog.json.imported" && ! -e "$state/backlog.json" ]] || { echo "S59: backlog.json wasn't imported"; ls "$state"; return 1; }
  (cd "$repo" && kanna task list) | grep -qE '^t3 +From JSON +Backlog' || { echo "S59: the imported task isn't in the backlog"; (cd "$repo" && kanna task list); return 1; }
  events() { raw "{\"id\":1,\"cmd\":\"backlog\",\"args\":{\"action\":\"events\",\"task\":\"$1\"}}" | python3 -c 'import json, sys; print(" ".join(e["what"] + ("@" + e["by"] if e["by"] else "") for e in json.load(sys.stdin)["events"]))'; }
  [[ "$(events t3)" == "made" ]] || { echo "S59: t3's events are $(events t3)"; return 1; }
  (cd "$repo" && kanna task new "Fresh" "say fresh" --agent "$agent") | grep -q 't4' || { echo "S59: a new task didn't follow the imported counter"; return 1; }
  kanna task shelve t4 >/dev/null || { echo "S59 line $LINENO"; return 1; }
  local task window; task="$(pane Fresh id)"
  window="$(KANNA_PANE="$task" kanna task pull "$task")" && window="${window##* onto }" && [[ "$window" =~ ^w[0-9]+$ ]] || { echo "S59: pull said $window"; return 1; }
  raw "{\"id\":1,\"cmd\":\"debug.click\",\"args\":{\"park\":\"$window\"}}" >/dev/null
  check wait_for 5 eval '[[ "$(events t4)" == *"parked@user" ]]' || { echo "S59: the app's park wasn't recorded: $(events t4)"; return 1; }
  kanna task stop "$task" >/dev/null && kanna task start "$task" >/dev/null || { echo "S59 line $LINENO"; return 1; }
  kanna task name "$task" "Fresher" >/dev/null || { echo "S59 line $LINENO"; return 1; }
  check wait_for 5 eval '[[ "$(events t4)" == *renamed ]]' || { echo "S59: the rename wasn't recorded: $(events t4)"; return 1; }
  kanna task defer "$task" >/dev/null && kanna task done t4 >/dev/null || { echo "S59 line $LINENO"; return 1; }
  local want="made@user shelved@user pulled@$task parked@user stopped@user started@user renamed deferred@user done@user"
  [[ "$(events t4)" == "$want" ]] || { echo "S59: t4's events are \"$(events t4)\", not \"$want\""; return 1; }
  [[ "$(sqlite3 -readonly "$state/kanna.db" "select place from tasks where id = 't4'")" == done ]] || { echo "S59: a done task's row is gone or not done"; return 1; }
  stop_app; stop_daemon
  local lines; lines="$(wc -l <"$out/daemon.log")"
  rm -f "$state/kanna.db-wal" "$state/kanna.db-shm"
  printf 'this is not a database %.0s' {1..200} >"$state/kanna.db"
  start_daemon && start_app || { echo "S59 line $LINENO"; return 1; }
  tail -n +"$((lines + 1))" "$out/daemon.log" | grep -q "kanna.db can't be used" || { echo "S59: the daemon didn't say the db was bad"; return 1; }
  tail -n +"$((lines + 1))" "$out/daemon.log" | grep -q "restored .*kanna.db from" || { echo "S59: the daemon didn't say it restored the db"; return 1; }
  (cd "$repo" && kanna task list) | grep -qE '^t3 +From JSON +Backlog' || { echo "S59: a backlog task was lost with the db"; (cd "$repo" && kanna task list); return 1; }
  ls "$state" | grep -q '^kanna.db.bad-' || { echo "S59: the bad db wasn't kept"; return 1; }
}

S60() {
  local repo="$KANNA_STATE_DIR/repo" state="$KANNA_STATE_DIR"
  mkdir -p "$repo/src" && printf 'one\ntwo\nthree\n' >"$repo/src/a.rs" || { echo "S60 line $LINENO"; return 1; }
  git -C "$repo" init -q -b main && git -C "$repo" add . && git -C "$repo" -c user.email=e@e -c user.name=e commit -q -m first || { echo "S60 line $LINENO"; return 1; }
  repo="$(cd "$repo" && pwd -P)"
  local head; head="$(git -C "$repo" rev-parse main)"
  start_daemon || { echo "S60 line $LINENO"; return 1; }
  quiz() { (cd "$repo/src" && kanna quiz "$@"); }
  [[ "$(quiz history)" == "no quiz rounds in this repo yet" ]] || { echo "S60: a new repo has history: $(quiz history)"; return 1; }
  [[ "$(quiz start --json '{"commits": ["first"]}')" == r1 ]] || { echo "S60: the first round isn't r1"; return 1; }
  [[ "$(quiz answer r1 --kind why --file a.rs:2 --wrong --json '{"question": "Why two?", "pick": 3, "answer": 1}')" == "a1 in r1" ]] || { echo "S60: the miss wasn't saved"; return 1; }
  quiz answer r1 --kind architecture --file "$repo/src/a.rs:1-3" --right >/dev/null || { echo "S60 line $LINENO"; return 1; }
  quiz answer r1 --kind trivia --file a.rs:1 --right >/dev/null 2>&1 && { echo "S60: an unknown kind was saved"; return 1; }
  stop_daemon; start_daemon || { echo "S60 line $LINENO"; return 1; }
  local misses; misses="$(quiz history --misses --json | python3 -c '
import json, sys
rounds = json.load(sys.stdin)["rounds"]
answers = [a for r in rounds for a in r["answers"]]
print(" ".join(r["round"] + "@" + r["head"] for r in rounds), " ".join(":".join([a["answer"], a["kind"], a["file"], str(a["doc"]["line"]), a["doc"]["question"]]) for a in answers))')"
  [[ "$misses" == "r1@$head a1:why:src/a.rs:2:Why two?" ]] || { echo "S60: the misses read back as \"$misses\""; return 1; }
  [[ "$(quiz start)" == r2 ]] || { echo "S60 line $LINENO"; return 1; }
  quiz answer r2 --kind why --file a.rs:2 --right --again a1 >/dev/null || { echo "S60 line $LINENO"; return 1; }
  [[ "$(quiz history --misses)" == "no misses to bring back" ]] || { echo "S60: a miss asked again is still a miss: $(quiz history --misses)"; return 1; }
  quiz history | grep -qE '^  a2 +✓ architecture +src/a.rs:1$' || { echo "S60: the history lacks the right answer"; quiz history; return 1; }
  [[ "$(sqlite3 -readonly "$state/kanna.db" "select count(*) from quiz_rounds; select group_concat(right) from quiz_answers")" == $'2\n0,1,1' ]] || { echo "S60: kanna.db doesn't hold the rounds and answers"; return 1; }
}

S61() {
  local old="$KANNA_STATE_DIR/old"
  mkdir -p "$old" && cp "$bin/kanna-daemon" "$old/kanna-daemon" || { echo "S61 line $LINENO"; return 1; }
  codesign -f -s - -i dev.kanna.e2e.old-daemon "$old/kanna-daemon" 2>/dev/null || { echo "S61: can't re-sign a copy of the daemon as an older build"; return 1; }
  KANNA_KEEPER="$bin/kanna-keeper" "$old/kanna-daemon" >>"$out/daemon.log" 2>&1 &
  daemon_pid=$!
  wait_for 5 test -S "$KANNA_SOCKET" || { echo "the old daemon did not open its socket"; return 1; }
  local new_build old_build
  new_build="$(shasum -a 256 "${app%/MacOS/Kanna}/Helpers/kanna-daemon" | cut -d' ' -f1)"
  old_build="$(raw '{"id":1,"cmd":"capabilities"}' | field build)"
  [[ "$old_build" == "$(shasum -a 256 "$old/kanna-daemon" | cut -d' ' -f1)" && "$old_build" != "$new_build" ]] || { echo "S61: the old daemon's build is $old_build"; return 1; }
  kanna open --name s61 --cmd 'echo S61-BEFORE; exec cat' >/dev/null || return 1
  check wait_for 5 shows s61 S61-BEFORE || return 1
  local pid; pid="$(pane s61 pid)"
  [[ "$(kanna send nosuch x 2>&1)" == *"from another build than this kanna"* ]] || { echo "S61: the CLI didn't note the old daemon"; return 1; }
  "$app" --bg >>"$out/app.log" 2>&1 &
  app_pid=$!
  wait_for 10 eval "! kill -0 $daemon_pid" || { echo "S61: the old daemon didn't stop for the new app"; return 1; }
  wait "$daemon_pid" 2>/dev/null; daemon_pid=""
  wait_for 15 app_attached || { echo "S61: the app did not start its daemon and attach"; return 1; }
  check grep -q "Kanna.app/Contents/Helpers/kanna-daemon" <(ps -o command= -p "$(cat "$KANNA_STATE_DIR/daemon.pid")") || return 1
  check test "$(raw '{"id":1,"cmd":"capabilities"}' | field build)" == "$new_build" || return 1
  check is s61 pid "$pid" || return 1
  kanna send s61 'S61-AFTER' || return 1
  check wait_for 5 shows s61 S61-AFTER || return 1
  kanna open --name s61b --split right >/dev/null || return 1
  check app_text s61 S61-BEFORE || return 1
  [[ "$(kanna send nosuch x 2>&1)" == *"another build"* ]] && { echo "S61: the CLI notes a daemon of its own build"; return 1; }
  return 0
}

due() {
  case "$1" in S1|S2) echo M0 ;; S3|S4|S5|S9) echo M1 ;; S6|S7) echo M2 ;; S8|S10) echo M3 ;; S11|S12|S13|S20|S21|S22|S3[0-9]|S4[0-9]|S5[0-9]|S6[0-9]) echo M4 ;; esac
}
[[ ${#only[@]} -gt 0 ]] && scenarios=("${only[@]}") || scenarios=(S1 S2 S3 S4 S5 S6 S7 S8 S9 S10 S11 S12 S13 S20 S21 S22 S30 S31 S32 S33 S34 S35 S36 S37 S38 S39 S40 S41 S42 S43 S44 S45 S46 S47 S48 S49 S50 S51 S52 S53 S54 S55 S56 S57 S58 S59 S60 S61 S62)
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
