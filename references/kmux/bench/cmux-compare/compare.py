#!/usr/bin/env python3
"""Quick-and-dirty performance comparison of kmux and cmux (docs/research/cmux-perf.md).

Black-box only: both apps are driven through their control sockets and
measured from the outside (proc_pid_rusage for memory and CPU, the process
table for helpers). Each run launches a fresh copy of the app in the
background and hidden (`open -g -j -n`) on a private socket, so it never
touches a kmux or cmux the user is running. cmux also gets a throwaway home
directory, so it doesn't restore or overwrite the user's cmux session.

Per run:
  startup   launch -> socket answers -> first pane runs a command
  memory    app footprint with its first window, then with --panes more idle shell tabs
  open      new tab -> `printf` typed into the new shell shows its output (--panes times)
  idle CPU  CPU time of the app and everything it started, over --idle seconds
  cat       time for `cat` of a --cat-mb text file inside the shell (zsh $EPOCHREALTIME)
  send      control-socket text -> echoed on screen (not typing latency; see the doc)

usage: compare.py [--apps kmux,cmux] [--runs 3] [--panes 10] [--idle 30] [--cat-mb 20] [--visible]
                  [--allow-cmux-front] [--out FILE]
Needs to run outside the Claude Code sandbox (it uses `open`).
"""

import argparse, ctypes, json, os, re, shutil, socket, statistics, subprocess, sys, tempfile, time, uuid

KMUX_APP = os.environ.get("KMUX_APP", os.path.expanduser("~/work/kmux/target/kmux.app"))
CMUX_APP = os.environ.get("CMUX_APP", "/Applications/cmux.app")

# ---------------------------------------------------------------- process stats

libproc = ctypes.CDLL("/usr/lib/libproc.dylib")
libc = ctypes.CDLL("/usr/lib/libSystem.dylib")


class RusageInfoV2(ctypes.Structure):
    _fields_ = [("uuid", ctypes.c_uint8 * 16)] + [(n, ctypes.c_uint64) for n in (
        "user_time", "system_time", "pkg_idle_wkups", "interrupt_wkups", "pageins", "wired_size",
        "resident_size", "phys_footprint", "proc_start_abstime", "proc_exit_abstime", "child_user_time",
        "child_system_time", "child_pkg_idle_wkups", "child_interrupt_wkups", "child_pageins",
        "child_elapsed_abstime", "diskio_bytesread", "diskio_byteswritten")]


class Timebase(ctypes.Structure):
    _fields_ = [("numer", ctypes.c_uint32), ("denom", ctypes.c_uint32)]


_tb = Timebase()
libc.mach_timebase_info(ctypes.byref(_tb))


def rusage(pid):
    info = RusageInfoV2()
    if libproc.proc_pid_rusage(pid, 2, ctypes.byref(info)) != 0:
        return None
    cpu_ns = (info.user_time + info.system_time) * _tb.numer / _tb.denom
    return {"footprint": info.phys_footprint, "cpu_s": cpu_ns / 1e9, "wakeups": info.pkg_idle_wkups + info.interrupt_wkups}


def processes():
    """{pid: (ppid, command)} for every process."""
    out = subprocess.run(["ps", "-axo", "pid=,ppid=,comm="], capture_output=True, text=True).stdout
    table = {}
    for line in out.splitlines():
        parts = line.split(None, 2)
        if len(parts) == 3:
            table[int(parts[0])] = (int(parts[1]), parts[2])
    return table


SHELLS = re.compile(r"(^|/)-?(login|zsh|bash|sh|fish|cat|seq)$")


def family(app_pid, bundle, before):
    """The app's helpers: its descendants, plus new processes from its bundle or
    new WebKit processes that appeared after launch (XPC services aren't children)."""
    table = processes()
    children = {}
    for pid, (ppid, _) in table.items():
        children.setdefault(ppid, []).append(pid)
    tree, todo = set(), [app_pid]
    while todo:
        pid = todo.pop()
        for c in children.get(pid, []):
            if c not in tree:
                tree.add(c)
                todo.append(c)
    extra = {pid for pid, (_, cmd) in table.items()
             if pid not in before and pid != app_pid and pid not in tree
             and (cmd.startswith(bundle) or "com.apple.WebKit" in cmd)}
    members = []
    for pid in sorted(tree | extra):
        cmd = table[pid][1]
        members.append({"pid": pid, "cmd": cmd, "shell": bool(SHELLS.search(cmd)), "descendant": pid in tree})
    return members


def measure_memory(app_pid, bundle, before):
    app = rusage(app_pid) or {"footprint": 0}
    members = family(app_pid, bundle, before)
    helpers = [m for m in members if not m["shell"]]
    shells = [m for m in members if m["shell"]]
    fp = lambda ms: sum((rusage(m["pid"]) or {"footprint": 0})["footprint"] for m in ms)
    return {"appMB": mb(app["footprint"]), "helpersMB": mb(fp(helpers)), "shellsMB": mb(fp(shells)),
            "helpers": sorted({os.path.basename(m["cmd"]) for m in helpers}), "helperCount": len(helpers),
            "shellCount": len(shells)}


def cpu_seconds(app_pid, bundle, before):
    total = (rusage(app_pid) or {"cpu_s": 0})["cpu_s"]
    per = {}
    for m in family(app_pid, bundle, before):
        r = rusage(m["pid"])
        if r:
            per[m["pid"]] = r["cpu_s"]
    return total, per


def mb(b):
    return round(b / 1048576, 1)


# ---------------------------------------------------------------- drivers

class Socket:
    """Newline-delimited JSON over a Unix socket."""

    def __init__(self, path):
        self.s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.s.connect(path)
        self.buf = b""
        self.n = 0

    def ask(self, obj):
        self.s.sendall((json.dumps(obj) + "\n").encode())
        while b"\n" not in self.buf:
            chunk = self.s.recv(1 << 20)
            if not chunk:
                raise RuntimeError("socket closed")
            self.buf += chunk
        line, self.buf = self.buf.split(b"\n", 1)
        return json.loads(line)


HIDDEN = True


def launch(bundle, env, app_args=()):
    """`open -g -j -n` (background, hidden, new instance; no -j with --visible); returns the new PID."""
    binary = os.path.join(bundle, "Contents/MacOS", os.path.basename(bundle)[:-4])
    before = processes()
    args = ["open", "-g", "-n"] + (["-j"] if HIDDEN else [])
    for k, v in env.items():
        args += ["--env", f"{k}={v}"]
    subprocess.run(args + [bundle] + (["--args", *app_args] if app_args else []), check=True)
    deadline = time.time() + 20
    while time.time() < deadline:
        new = [pid for pid, (_, cmd) in processes().items() if cmd == binary and pid not in before]
        if new:
            return new[0], set(before)
        time.sleep(0.02)
    raise RuntimeError(f"{bundle} did not start")


class Kmux:
    name = "kmux"
    bundle = KMUX_APP

    def start(self):
        self.sock_path = os.path.join(tempfile.gettempdir(), f"kc-{uuid.uuid4().hex[:8]}.sock")
        self.pid, self.before = launch(self.bundle, {"KMUX_SOCKET": self.sock_path, "KMUX_BG": "1"})
        self.c = None
        while self.c is None:
            try:
                self.c = Socket(self.sock_path)
            except OSError:
                time.sleep(0.005)
        self.n = 0

    def call(self, cmd, **args):
        self.n += 1
        r = self.c.ask({"id": self.n, "cmd": cmd, "args": args})
        if not r.get("ok"):
            raise RuntimeError(f"kmux {cmd}: {r}")
        return r

    def ready(self):
        self.call("list")

    def first_pane(self):
        """The pane of the app's own first window (opens one if the app started without)."""
        def panes(node):
            if isinstance(node, dict):
                if node.get("type") == "term" and "id" in node:
                    yield node["id"]
                for v in node.values():
                    yield from panes(v)
            elif isinstance(node, list):
                for v in node:
                    yield from panes(v)
        while True:
            found = list(panes(self.call("list")))
            if found:
                return found[0]
            time.sleep(0.005)

    def open_tab(self):
        return self.call("open", type="term", tab=True)["pane"]["id"]

    def send(self, pane, text):
        self.call("send", pane=pane, text=text)

    def read(self, pane):
        return self.call("debug.text", pane=pane)["text"]

    def stop(self):
        os.kill(self.pid, 15)
        try:
            os.remove(self.sock_path)
        except OSError:
            pass


class Cmux:
    name = "cmux"
    bundle = CMUX_APP
    prefs = os.path.expanduser("~/Library/Preferences/com.cmuxterm.app.plist")

    def start(self):
        if any(cmd.startswith(self.bundle) for _, cmd in processes().values()):
            raise RuntimeError("cmux is already running; not touching it")
        # Throwaway home: no restore of (or writes to) the user's cmux session, state or config.
        # UserDefaults still go to the real ~/Library/Preferences, so that plist is saved and put back.
        self.home = tempfile.mkdtemp(prefix="cmux-home-")
        self.saved = self.prefs + ".cmux-compare-backup"
        if os.path.exists(self.prefs):
            shutil.copy2(self.prefs, self.saved)
        self.sock_path = os.path.join(self.home, "cmux.sock")
        env = {"CMUX_SOCKET_PATH": self.sock_path, "CMUX_ALLOW_SOCKET_OVERRIDE": "1", "CMUX_SOCKET_MODE": "allowAll",
               "CMUX_SOCKET_ENABLE": "1", "CMUX_DISABLE_SESSION_RESTORE": "1",
               "HOME": self.home, "CFFIXED_USER_HOME": self.home, "XDG_CONFIG_HOME": os.path.join(self.home, ".config")}
        args = ["-SUEnableAutomaticChecks", "NO", "-sendAnonymousTelemetry", "NO", "-cmuxWelcomeShown", "YES"]
        self.pid, self.before = launch(self.bundle, env, args)
        self.c = None
        while self.c is None:
            try:
                self.c = Socket(self.sock_path)
            except OSError:
                time.sleep(0.005)
        self.n = 0

    def call(self, method, **params):
        self.n += 1
        try:
            r = self.c.ask({"id": self.n, "method": method, "params": params})
        except (OSError, RuntimeError):
            # cmux drops a control connection that sits idle; reconnect once and retry.
            self.c = Socket(self.sock_path)
            r = self.c.ask({"id": self.n, "method": method, "params": params})
        if not r.get("ok"):
            raise RuntimeError(f"cmux {method}: {r.get('error')}")
        return r.get("result") or {}

    def ready(self):
        self.call("system.ping")

    def first_pane(self):
        while True:
            focused = self.call("system.identify").get("focused") or {}
            if focused.get("surface_id"):
                return focused["surface_id"]
            time.sleep(0.005)

    def open_tab(self):
        return self.call("surface.create")["surface_id"]

    def send(self, pane, text):
        self.call("surface.send_text", surface_id=pane, text=text + "\n")

    def read(self, pane):
        try:
            return self.call("surface.read_text", surface_id=pane).get("text", "")
        except RuntimeError:
            return ""  # not running yet

    def stop(self):
        os.kill(self.pid, 15)
        for _ in range(50):
            try:
                os.kill(self.pid, 0)
                time.sleep(0.1)
            except OSError:
                break
        else:
            os.kill(self.pid, 9)
        shutil.rmtree(self.home, ignore_errors=True)
        if os.path.exists(self.saved):
            shutil.move(self.saved, self.prefs)
        elif os.path.exists(self.prefs):
            os.remove(self.prefs)



# ---------------------------------------------------------------- measures


def wait_text(app, pane, pattern, timeout=20):
    deadline = time.perf_counter() + timeout
    rx = re.compile(pattern)
    while True:
        m = rx.search(app.read(pane))
        if m:
            return time.perf_counter(), m
        if time.perf_counter() > deadline:
            raise RuntimeError(f"{app.name}: timed out waiting for {pattern!r}: {app.read(pane)[-300:]!r}")


def marker_cmd(tag):
    # The typed line shows "M%sK"; only the output shows "M<tag>K".
    return f"printf 'M%sK\\n' {tag}", rf"M{tag}K"


def run_once(app, panes, idle, cat_file):
    out = {}
    t0 = time.perf_counter()
    app.start()
    t_sock = time.perf_counter()
    first = app.first_pane()
    cmd, pat = marker_cmd("0")
    app.send(first, cmd)
    t_first, _ = wait_text(app, first, pat)
    out["startup"] = {"socketMs": ms(t0, t_sock), "firstCommandMs": ms(t0, t_first)}

    time.sleep(3)
    out["memStart"] = measure_memory(app.pid, app.bundle, app.before)

    opens, running = [], []
    tabs = []
    for i in range(1, panes + 1):
        cmd, pat = marker_cmd(str(100 + i))
        t = time.perf_counter()
        pane = app.open_tab()
        t_run = time.perf_counter()
        while True:  # a pane may refuse input until its shell is running
            try:
                app.send(pane, cmd)
                break
            except RuntimeError:
                time.sleep(0.002)
        t_done, _ = wait_text(app, pane, pat)
        running.append(ms(t, t_run))
        opens.append(ms(t, t_done))
        tabs.append(pane)
    out["open"] = {"returnedMs": running, "commandOutputMs": opens}

    time.sleep(5)
    out["memPanes"] = measure_memory(app.pid, app.bundle, app.before)
    out["perPaneMB"] = round((out["memPanes"]["appMB"] - out["memStart"]["appMB"]) / panes, 1)

    c0, p0 = cpu_seconds(app.pid, app.bundle, app.before)
    time.sleep(idle)
    c1, p1 = cpu_seconds(app.pid, app.bundle, app.before)
    helpers = sum(p1[p] - p0.get(p, 0) for p in p1)
    out["idleCpuPct"] = {"app": round((c1 - c0) / idle * 100, 2), "helpersAndShells": round(helpers / idle * 100, 2)}

    # Throughput, in the first pane (cat's own wall time, measured by the shell).
    cats, walls = [], []
    for i in range(3):
        tag = f"{i}{int(time.time()) % 1000}"
        t = time.perf_counter()
        app.send(first, f"S=$EPOCHREALTIME; /bin/cat {cat_file}; printf 'C%sD\\n' $(( ($EPOCHREALTIME - S) * 1000 )); printf 'Z%sZ\\n' {tag}")
        t_done, _ = wait_text(app, first, rf"Z{tag}Z", timeout=120)
        m = re.search(r"C([0-9.]+)D", app.read(first))
        cats.append(round(float(m.group(1)), 1) if m else None)
        walls.append(ms(t, t_done))
        time.sleep(1)
    out["cat"] = {"shellMs": cats, "wallMs": walls}

    # Socket "send" -> echoed text visible (a cat in a fresh tab echoes the line back).
    pane = tabs[0]
    app.send(pane, "exec /bin/cat")
    time.sleep(0.5)
    sends = []
    for i in range(30):
        word = f"q{i:03d}x"
        t = time.perf_counter()
        app.send(pane, word)
        t_done, _ = wait_text(app, pane, rf"{word}\s*\n\s*{word}")
        sends.append(ms(t, t_done))
    out["sendEchoMs"] = sends
    app.stop()
    return out


def ms(a, b):
    return round((b - a) * 1000, 1)


def med(xs):
    xs = [x for x in xs if x is not None]
    return round(statistics.median(xs), 1) if xs else None


def p95(xs):
    xs = sorted(x for x in xs if x is not None)
    return round(xs[int(round((len(xs) - 1) * 0.95))], 1) if xs else None


def summarize(runs):
    allopen = [x for r in runs for x in r["open"]["commandOutputMs"]]
    allsend = [x for r in runs for x in r["sendEchoMs"]]
    return {
        "runs": len(runs),
        "startupFirstCommandMs": med([r["startup"]["firstCommandMs"] for r in runs]),
        "startupSocketMs": med([r["startup"]["socketMs"] for r in runs]),
        "memStartAppMB": med([r["memStart"]["appMB"] for r in runs]),
        "memStartHelpersMB": med([r["memStart"]["helpersMB"] for r in runs]),
        "helperCount": med([r["memStart"]["helperCount"] for r in runs]),
        "helpers": runs[0]["memStart"]["helpers"],
        "memPanesAppMB": med([r["memPanes"]["appMB"] for r in runs]),
        "memPanesHelpersMB": med([r["memPanes"]["helpersMB"] for r in runs]),
        "perPaneMB": med([r["perPaneMB"] for r in runs]),
        "openReturnedP50Ms": med([x for r in runs for x in r["open"]["returnedMs"]]),
        "openCommandP50Ms": med(allopen),
        "openCommandP95Ms": p95(allopen),
        "idleCpuAppPct": med([r["idleCpuPct"]["app"] for r in runs]),
        "idleCpuHelpersPct": med([r["idleCpuPct"]["helpersAndShells"] for r in runs]),
        "catShellMs": med([x for r in runs for x in r["cat"]["shellMs"]]),
        "catWallMs": med([x for r in runs for x in r["cat"]["wallMs"]]),
        "sendEchoP50Ms": med(allsend),
        "sendEchoP95Ms": p95(allsend),
    }


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--apps", default="kmux,cmux")
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--panes", type=int, default=10)
    ap.add_argument("--idle", type=int, default=30)
    ap.add_argument("--cat-mb", type=int, default=20)
    ap.add_argument("--visible", action="store_true", help="launch without -j: windows are shown (behind the front app)")
    ap.add_argument("--allow-cmux-front", action="store_true",
                    help="cmux un-hides and orders its first window front at launch; only use when nobody is working on the screen")
    ap.add_argument("--out", default=os.path.join(os.path.dirname(os.path.abspath(__file__)), "results.json"))
    a = ap.parse_args()

    cat_file = os.path.join(tempfile.gettempdir(), f"cmux-compare-{a.cat_mb}mb.txt")
    if not os.path.exists(cat_file):
        line = "The quick brown fox jumps over the lazy dog 0123456789 \x1b[32mgreen\x1b[0m and plain text again\n"
        with open(cat_file, "w") as f:
            f.write(line * (a.cat_mb * 1048576 // len(line)))

    global HIDDEN
    HIDDEN = not a.visible
    if "cmux" in a.apps and not a.allow_cmux_front:
        sys.exit("cmux brings its first window to the front at launch, even with open -g -j. "
                 "Re-run with --allow-cmux-front when nobody is using the screen.")
    drivers = {"kmux": Kmux, "cmux": Cmux}
    results = {"hidden": HIDDEN, "when": time.strftime("%Y-%m-%d %H:%M"), "machine": machine(), "options": vars(a), "apps": {}}
    for name in a.apps.split(","):
        app = drivers[name]()
        runs = []
        for r in range(a.runs):
            print(f"{name} run {r + 1}/{a.runs} ...", file=sys.stderr)
            try:
                runs.append(run_once(app, a.panes, a.idle, cat_file))
            except Exception:
                try:
                    app.stop()
                except Exception:
                    pass
                raise
            time.sleep(2)
        results["apps"][name] = {"bundleMB": bundle_mb(app.bundle), "version": version(app.bundle),
                                 "summary": summarize(runs), "runs": runs}
    with open(a.out, "w") as f:
        json.dump(results, f, indent=2)
    print(json.dumps({k: {"bundleMB": v["bundleMB"], **v["summary"]} for k, v in results["apps"].items()}, indent=2))


def bundle_mb(path):
    return round(int(subprocess.run(["du", "-sk", path], capture_output=True, text=True).stdout.split()[0]) / 1024, 1)


def version(bundle):
    r = subprocess.run(["defaults", "read", os.path.join(bundle, "Contents/Info"), "CFBundleShortVersionString"],
                       capture_output=True, text=True)
    return r.stdout.strip()


def machine():
    sysctl = lambda k: subprocess.run(["sysctl", "-n", k], capture_output=True, text=True).stdout.strip()
    return {"cpu": sysctl("machdep.cpu.brand_string"), "memGB": int(sysctl("hw.memsize")) // 2**30,
            "macos": subprocess.run(["sw_vers", "-productVersion"], capture_output=True, text=True).stdout.strip()}


if __name__ == "__main__":
    main()
