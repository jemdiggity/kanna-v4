#!/usr/bin/env python3
import argparse
import base64
import json
import re
import sys
from pathlib import Path


def load(path):
    stream, times, marks, inputs = bytearray(), [], [], []
    for line in Path(path).read_text().splitlines():
        e = json.loads(line)
        if e["k"] == "o":
            data = base64.b64decode(e["b"])
            times.append((len(stream), e["t"]))
            stream.extend(data)
        elif e["k"] == "mark":
            marks.append((e["t"], e["d"]))
        elif e["k"] == "i":
            inputs.append((e["t"], base64.b64decode(e["b"])))
    return bytes(stream), times, marks, inputs


def time_at(times, offset):
    t = 0.0
    for start, at in times:
        if start > offset:
            break
        t = at
    return t


def phase_at(marks, t):
    name = "-"
    for at, mark in marks:
        if at <= t:
            name = mark
    return name


def classify(body):
    code, _, rest = body.partition(b";")
    if code in (b"0", b"1", b"2"):
        return f"title(OSC {code.decode()})"
    if code == b"9":
        if rest.startswith(b"4;"):
            return "progress(OSC 9;4)"
        return "notify(OSC 9)"
    if code == b"777":
        return "notify(OSC 777)"
    if code == b"99":
        return "notify(OSC 99)"
    if code == b"1337":
        return "iterm(OSC 1337)"
    return f"osc {code.decode('latin1')}"


def scan(stream):
    i, n = 0, len(stream)
    while i < n:
        b = stream[i]
        if b == 0x07:
            yield i, "bell(BEL)", b"\x07"
            i += 1
        elif b == 0x1b and i + 1 < n and stream[i + 1] in (0x5d, 0x50, 0x5f, 0x5e):
            kind = stream[i + 1]
            j = i + 2
            while j < n:
                if kind == 0x5d and stream[j] == 0x07:
                    end = j + 1
                    break
                if stream[j] == 0x1b and j + 1 < n and stream[j + 1] == 0x5c:
                    end = j + 2
                    break
                j += 1
            else:
                end = n
            if kind == 0x5d:
                body = stream[i + 2:end].rstrip(b"\x07").removesuffix(b"\x1b\\")
                yield i, classify(body), stream[i:end]
            elif stream[i + 2:i + 8] == b"tmux;\x1b":
                yield i, "tmux passthrough", stream[i:end]
            i = end
        elif stream.startswith(b"\x1b[?1004h", i):
            yield i, "focus reporting on", b"\x1b[?1004h"
            i += 8
        else:
            i += 1


def report(path, include_titles=True):
    stream, times, marks, inputs = load(path)
    rows = []
    for offset, kind, raw in scan(stream):
        if kind.startswith("osc ") and kind not in ("osc 9", "osc 777"):
            continue
        if kind.startswith("title") and not include_titles:
            continue
        t = time_at(times, offset)
        rows.append({"t": t, "phase": phase_at(marks, t), "kind": kind, "raw": raw.decode("utf-8", "replace").encode("unicode_escape").decode()[:160]})
    return rows, marks


def collapse(rows):
    out = []
    for row in rows:
        if out and out[-1]["kind"] == row["kind"] and out[-1]["raw"] == row["raw"] and out[-1]["phase"] == row["phase"]:
            out[-1]["n"] += 1
            out[-1]["t_last"] = row["t"]
        else:
            out.append({**row, "n": 1, "t_last": row["t"]})
    return out


def main():
    p = argparse.ArgumentParser()
    p.add_argument("recordings", nargs="+")
    p.add_argument("--json", action="store_true")
    p.add_argument("--no-titles", action="store_true")
    p.add_argument("--text", type=int, metavar="CHARS", help="print the last CHARS of the output as plain text instead")
    a = p.parse_args()
    for path in a.recordings:
        if a.text:
            stream = load(path)[0]
            plain = re.sub(rb"\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07\x1b]*(?:\x07|\x1b\\\\)|P[^\x1b]*\x1b\\\\|_[^\x1b]*\x1b\\\\|[()][0-9A-Za-z]|[=>78MDE])", b" ", stream)
            print(f"== {path}\n" + re.sub(r"\s+", " ", plain.decode("utf-8", "replace"))[-a.text:])
            continue
        rows, marks = report(path, not a.no_titles)
        if a.json:
            print(json.dumps({"recording": path, "marks": marks, "signals": rows}))
            continue
        print(f"== {path}")
        print("   marks: " + ", ".join(f"{m}@{t:.1f}" for t, m in marks))
        for r in collapse(rows):
            span = f"{r['t']:7.2f}" + (f"-{r['t_last']:.2f} x{r['n']}" if r["n"] > 1 else "")
            print(f"   {span:22} {r['phase']:16} {r['kind']:20} {r['raw']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
