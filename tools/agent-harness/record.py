#!/usr/bin/env python3
import argparse
import base64
import fcntl
import json
import os
import pty
import re
import select
import signal
import struct
import termios
import time
from pathlib import Path

KEYS = {"enter": b"\r", "esc": b"\x1b", "ctrl_c": b"\x03", "up": b"\x1b[A", "down": b"\x1b[B", "tab": b"\t", "focus_in": b"\x1b[I", "focus_out": b"\x1b[O"}
STRIP = re.compile(rb"\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07\x1b]*(?:\x07|\x1b\\)|P[^\x1b]*\x1b\\|[()][0-9A-Za-z]|[=>78MDE])")


class Recorder:
    def __init__(self, argv, cwd, env, out, rows, cols, version):
        self.start = time.monotonic()
        self.out = open(out, "w")
        self.output = bytearray()
        self.since = 0
        self.rows, self.cols = rows, cols
        self.version = version
        self.focus_reporting = False
        self.note("start", {"argv": argv, "cwd": cwd, "env": env, "rows": rows, "cols": cols, "wall": time.time()})
        self.pid, self.master = pty.fork()
        if self.pid == 0:
            os.chdir(cwd)
            child = {k: v for k, v in os.environ.items() if not k.startswith(("CLAUDE", "CODEX", "ANTHROPIC", "OPENAI", "KANNA", "KMUX", "GHOSTTY", "TERM", "COLORTERM", "ITERM", "__CF"))}
            for key, value in env.items():
                if value is None:
                    child.pop(key, None)
                else:
                    child[key] = value
            os.execvpe(argv[0], argv, child)
        fcntl.ioctl(self.master, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))

    def now(self):
        return round(time.monotonic() - self.start, 4)

    def note(self, kind, data):
        self.out.write(json.dumps({"t": self.now(), "k": kind, **({"d": data} if not isinstance(data, bytes) else {"b": base64.b64encode(data).decode()})}) + "\n")
        self.out.flush()

    def answer(self, data):
        replies = []
        if b"\x1b[c" in data or b"\x1b[0c" in data:
            replies.append(b"\x1b[?62;22c")
        if b"\x1b[6n" in data:
            replies.append(f"\x1b[{self.rows};1R".encode())
        if b"\x1b[>q" in data or b"\x1b[>0q" in data:
            replies.append(f"\x1bP>|ghostty {self.version}\x1b\\".encode())
        if b"\x1b[?u" in data:
            replies.append(b"\x1b[?0u")
        for mode in re.findall(rb"\x1b\[\?(\d+)\$p", data):
            replies.append(b"\x1b[?" + mode + b";2$y")
        if b"\x1b[?996n" in data:
            replies.append(b"\x1b[?997;1n")
        for code in re.findall(rb"\x1b\](1[01]);\?(?:\x07|\x1b\\)", data):
            replies.append(b"\x1b]" + code + (b";rgb:ffff/ffff/ffff" if code == b"10" else b";rgb:0000/0000/0000") + b"\x1b\\")
        if b"\x1b[14t" in data:
            replies.append(f"\x1b[4;{self.rows * 20};{self.cols * 10}t".encode())
        if b"\x1b[16t" in data:
            replies.append(b"\x1b[6;20;10t")
        if b"\x1b[18t" in data:
            replies.append(f"\x1b[8;{self.rows};{self.cols}t".encode())
        if b"\x1b[?1004h" in data:
            self.focus_reporting = True
        for reply in replies:
            self.send(reply, "answer")

    def pump(self, timeout):
        if self.pid is None:
            time.sleep(timeout)
            return
        try:
            readable, _, _ = select.select([self.master], [], [], max(0, timeout))
            if readable:
                data = os.read(self.master, 65536)
                if not data:
                    raise OSError
                self.output.extend(data)
                self.note("o", data)
                self.answer(data)
        except OSError:
            self.reap()

    def drain(self, seconds):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            self.pump(min(0.1, end - time.monotonic()))

    def reap(self):
        if self.pid is None:
            return
        try:
            pid, status = os.waitpid(self.pid, 0)
            self.note("exit", {"status": status})
        except ChildProcessError:
            pass
        self.pid = None

    def send(self, data, kind="i"):
        try:
            os.write(self.master, data)
            self.note(kind, data)
        except OSError:
            pass

    def text(self):
        return re.sub(r"\s+", "", STRIP.sub(b"", bytes(self.output[self.since:])).decode("utf-8", "replace"))

    def run(self, step):
        kind = step["do"]
        if kind == "mark":
            self.note("mark", step["name"])
        elif kind == "wait":
            self.drain(step["s"])
        elif kind == "expect":
            pattern = re.compile(re.sub(r"\s+", "", step["re"]), re.I)
            end = time.monotonic() + step.get("timeout", 30)
            while time.monotonic() < end and self.pid is not None:
                if pattern.search(self.text()):
                    self.note("matched", step["re"])
                    return True
                self.pump(0.1)
            found = bool(pattern.search(self.text()))
            self.note("matched" if found else "timeout", step["re"])
            return found
        elif kind == "type":
            self.since = len(self.output)
            for ch in step["text"]:
                self.send(ch.encode())
                self.drain(step.get("delay", 0.05))
            self.drain(0.8)
            self.send(b"\r")
        elif kind == "key":
            self.send(KEYS.get(step["key"], step["key"].encode()))
        elif kind == "focus_out":
            self.send(KEYS["focus_out"])
        elif kind == "focus_in":
            self.send(KEYS["focus_in"])
        return None

    def stop(self):
        if self.pid is not None:
            try:
                os.killpg(os.getpgid(self.pid), signal.SIGTERM)
            except OSError:
                pass
            end = time.monotonic() + 3
            while time.monotonic() < end and self.pid is not None:
                self.pump(0.05)
                if self.pid is None:
                    break
                try:
                    pid, status = os.waitpid(self.pid, os.WNOHANG)
                    if pid:
                        self.note("exit", {"status": status})
                        self.pid = None
                except ChildProcessError:
                    self.pid = None
            if self.pid is not None:
                try:
                    os.killpg(os.getpgid(self.pid), signal.SIGKILL)
                    os.waitpid(self.pid, 0)
                except OSError:
                    pass
                self.pid = None
        self.note("end", {"focus_reporting": self.focus_reporting})
        self.out.close()
        os.close(self.master)


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--argv", required=True)
    p.add_argument("--cwd", required=True)
    p.add_argument("--env", default="{}")
    p.add_argument("--script", required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--rows", type=int, default=40)
    p.add_argument("--cols", type=int, default=120)
    p.add_argument("--version", default="1.3.2")
    a = p.parse_args()
    rec = Recorder(json.loads(a.argv), a.cwd, json.loads(a.env), a.out, a.rows, a.cols, a.version)
    try:
        for step in json.loads(Path(a.script).read_text()):
            ok = rec.run(step)
            if ok is False and step.get("required"):
                rec.note("abort", step["re"])
                break
    finally:
        rec.stop()


if __name__ == "__main__":
    main()
