#!/usr/bin/env python3
import argparse
import itertools
import json
import os
import signal
import time
import shutil
import subprocess
import sys
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

ROOT = Path(__file__).resolve().parent
GHOSTTY_VERSION = "1.3.2-dev+4ec3122"

TERMINALS = {
    "kanna": {"TERM": "xterm-ghostty", "COLORTERM": "truecolor", "TERM_PROGRAM": "ghostty", "TERM_PROGRAM_VERSION": GHOSTTY_VERSION, "GHOSTTY_SHELL_FEATURES": "cursor,path,title"},
    "noprogram": {"TERM": "xterm-ghostty", "COLORTERM": "truecolor", "GHOSTTY_SHELL_FEATURES": "cursor,path,title"},
    "xterm": {"TERM": "xterm-256color", "COLORTERM": "truecolor"},
}

SETTINGS = {
    "claude": {
        "default": {},
        "bell": {"preferredNotifChannel": "terminal_bell"},
        "ghostty": {"preferredNotifChannel": "ghostty"},
        "iterm2_with_bell": {"preferredNotifChannel": "iterm2_with_bell"},
    },
    "codex": {
        "default": {},
        "always": {"notifications": True, "notification_condition": "always"},
        "always_bel": {"notifications": True, "notification_condition": "always", "notification_method": "bel"},
        "on": {"notifications": True},
    },
}

READY = {"claude": r"shift\+tab|for shortcuts", "codex": r"context left|send|›"}
APPROVAL = {"claude": r"Do you want|proceed\?", "codex": r"Would you like|approve|Yes, proceed|Allow"}


def toml_value(v):
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, list):
        return "[" + ", ".join(toml_value(x) for x in v) + "]"
    return json.dumps(v)


def prepare(agent, base, port, settings):
    home, work = base / "home", base / "work"
    shutil.rmtree(base, ignore_errors=True)
    home.mkdir(parents=True)
    work.mkdir(parents=True)
    subprocess.run(["git", "init", "-q", str(work)], check=True)
    env = {"HOME": str(home), "XDG_CONFIG_HOME": str(home / ".config"), "XDG_DATA_HOME": str(home / ".local/share"), "XDG_STATE_HOME": str(home / ".local/state"), "XDG_CACHE_HOME": str(home / ".cache"), "LANG": "en_US.UTF-8"}
    if agent == "claude":
        config = {"hasCompletedOnboarding": True, "lastOnboardingVersion": "2.1.296", "theme": "dark", "numStartups": 5, "customApiKeyResponses": {"approved": ["mock"], "rejected": []}, "projects": {str(work): {"hasTrustDialogAccepted": True, "hasCompletedProjectOnboarding": True, "allowedTools": []}}}
        (home / ".claude").mkdir()
        (home / ".claude.json").write_text(json.dumps(config, indent=2))
        (home / ".claude/.claude.json").write_text(json.dumps(config, indent=2))
        (home / ".claude/settings.json").write_text(json.dumps({"theme": "dark", **settings}, indent=2))
        env.update({"ANTHROPIC_BASE_URL": f"http://127.0.0.1:{port}", "ANTHROPIC_API_KEY": "mock", "CLAUDE_CONFIG_DIR": str(home / ".claude"), "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1", "DISABLE_AUTOUPDATER": "1"})
        argv = [shutil.which("claude") or "claude", "--model", "mock-model", "--permission-mode", "default"]
    else:
        codex_home = home / ".codex"
        codex_home.mkdir()
        lines = ['model = "mock-model"', 'model_provider = "mock"', 'approval_policy = "on-request"', 'sandbox_mode = "workspace-write"', "check_for_update_on_startup = false", "", "[model_providers.mock]", 'name = "mock"', f'base_url = "http://127.0.0.1:{port}/v1"', 'wire_api = "responses"', 'env_key = "MOCK_API_KEY"', "", f'[projects."{work}"]', 'trust_level = "trusted"', ""]
        if settings:
            lines += ["[tui]"] + [f"{k} = {toml_value(v)}" for k, v in settings.items()] + [""]
        (codex_home / "config.toml").write_text("\n".join(lines))
        env.update({"CODEX_HOME": str(codex_home), "MOCK_API_KEY": "mock"})
        argv = [shutil.which("codex") or "codex"]
    return argv, work, env


def script(agent, focus):
    away = [{"do": "focus_out"}] if focus == "away" else []
    back = [{"do": "focus_in"}] if focus == "away" else []
    return [
        {"do": "mark", "name": "startup"},
        {"do": "wait", "s": 3},
        {"do": "expect", "re": READY[agent], "timeout": 60, "required": True},
        {"do": "wait", "s": 3},
        {"do": "focus_in"},
        {"do": "wait", "s": 1},
        {"do": "mark", "name": "plain"},
        {"do": "type", "text": "PLAIN_TURN: answer in a few lines"},
        *away,
        {"do": "expect", "re": r"Line 12 of", "timeout": 60},
        {"do": "mark", "name": "plain_end"},
        {"do": "wait", "s": 12},
        *back,
        {"do": "mark", "name": "tool"},
        {"do": "type", "text": "TOOL_TURN: create the probe file"},
        *away,
        {"do": "expect", "re": APPROVAL[agent], "timeout": 60},
        {"do": "mark", "name": "asking"},
        {"do": "wait", "s": 25},
        *back,
        {"do": "mark", "name": "approve"},
        {"do": "key", "key": "enter"},
        *away,
        {"do": "expect", "re": r"TOOL_DONE", "timeout": 60},
        {"do": "mark", "name": "idle"},
        {"do": "wait", "s": 80},
        {"do": "mark", "name": "end"},
    ]


def stop_leftovers(home):
    listing = subprocess.run(["ps", "-axo", "pid=,command="], capture_output=True, text=True).stdout
    pids = [int(line.split(None, 1)[0]) for line in listing.splitlines() if str(home) + "/" in line]
    for sig in (signal.SIGTERM, signal.SIGKILL):
        for pid in pids:
            try:
                os.kill(pid, sig)
            except OSError:
                pass
        time.sleep(2)


def run_one(out, port, size, agent, terminal, setting, focus):
    cols, rows = size
    name = f"{agent}-{terminal}-{setting}-{focus}" + ("" if size == (120, 40) else f"-{cols}x{rows}")
    base = out / "runs" / name
    argv, work, env = prepare(agent, base, port, SETTINGS[agent][setting])
    env.update(TERMINALS[terminal])
    if terminal != "kanna":
        env.update({"TERM_PROGRAM": None, "TERM_PROGRAM_VERSION": None})
    script_path = base / "script.json"
    script_path.write_text(json.dumps(script(agent, focus), indent=2))
    rec = out / f"{name}.jsonl"
    subprocess.run([sys.executable, str(ROOT / "record.py"), "--argv", json.dumps(argv), "--cwd", str(work), "--env", json.dumps(env), "--script", str(script_path), "--out", str(rec), "--version", GHOSTTY_VERSION.split("-")[0], "--cols", str(cols), "--rows", str(rows)], check=False)
    stop_leftovers(base / "home")
    return rec


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--out", default=str(ROOT / "out"))
    p.add_argument("--agents", default="claude,codex")
    p.add_argument("--terminals", default="kanna,noprogram")
    p.add_argument("--settings", default="default")
    p.add_argument("--focus", default="here")
    p.add_argument("--jobs", type=int, default=4)
    p.add_argument("--size", default="120x40", help="COLSxROWS of the terminal")
    a = p.parse_args()
    out = Path(a.out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    log = out / "mock-requests.jsonl"
    mock = subprocess.Popen([sys.executable, str(ROOT / "mock_server.py"), "--log", str(log)], stdout=subprocess.PIPE, text=True)
    try:
        port = int(mock.stdout.readline())
        urllib.request.urlopen(f"http://127.0.0.1:{port}/health", timeout=3).read()
        combos = [c for c in itertools.product(a.agents.split(","), a.terminals.split(","), a.settings.split(","), a.focus.split(",")) if c[2] in SETTINGS[c[0]]]
        with ThreadPoolExecutor(a.jobs) as pool:
            size = tuple(int(n) for n in a.size.lower().split("x"))
            recs = list(pool.map(lambda c: run_one(out, port, size, *c), combos))
    finally:
        mock.terminate()
        mock.wait(5)
    subprocess.run([sys.executable, str(ROOT / "signals.py"), *map(str, recs)])


if __name__ == "__main__":
    main()
