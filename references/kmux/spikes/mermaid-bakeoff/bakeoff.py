#!/usr/bin/env python3
"""Mermaid renderer bake-off: renders the corpus with each native candidate,
rasterizes the mermaid.js reference, measures and builds side-by-side composites.

  bakeoff.py render CHROME   # all tools -> target/bakeoff/out/<tool>/NAME.png|.svg|.error
  bakeoff.py metrics         # -> target/bakeoff/out/results.json (+ printed summary)
  bakeoff.py composites CHROME  # -> target/bakeoff/out/composites/*.png
"""
import html, json, os, re, shutil, statistics, struct, subprocess, sys, time, pathlib

here = pathlib.Path(__file__).resolve().parent
root = here.parent.parent
bake = root / "target/bakeoff"
out = bake / "out"
corpus = sorted((here / "corpus").glob("*.mmd"))
TOOLS = ["merman", "mmdr", "selkie", "beautifulmermaid"]
MERMAN = bake / "merman-rel/merman-cli-aarch64-apple-darwin/merman-cli"
MMDR = bake / "tools/bin/mmdr"
SELKIE = bake / "tools/bin/selkie"
BM = here / "bm-cli/.build/release/bm-cli"
TRIVIAL = "flowchart LR\n    A --> B\n"


def cli(tool, src, dst, fmt):
    """The command line that renders file `src` to `dst` in `fmt` (png or svg)."""
    if tool == "merman":
        cmd = [MERMAN, "render", src, "-o", dst, "-f", fmt, "-q"]
        return cmd + (["--scale", "2"] if fmt == "png" else ["--svg-pipeline", "resvg-safe"])
    if tool == "mmdr":
        return [MMDR, "-i", src, "-o", dst, "-e", fmt]
    if tool == "selkie":
        return [SELKIE, "render", src, "-o", dst, "-e", fmt, "-b", "white"]
    raise ValueError(tool)


def run(cmd):
    t = time.perf_counter()
    p = subprocess.run([str(c) for c in cmd], capture_output=True, text=True, timeout=120)
    return time.perf_counter() - t, p


def best_of(cmd, n):
    times = []
    for _ in range(n):
        dt, p = run(cmd)
        if p.returncode != 0:
            return None, p
        times.append(dt)
    return min(times), p


def png_size(path):
    with open(path, "rb") as f:
        head = f.read(24)
    return struct.unpack(">II", head[16:24]) if head[:8] == b"\x89PNG\r\n\x1a\n" else None


def svg_size(text):
    m = re.search(r'viewBox="[-\d.]+ [-\d.]+ ([\d.]+) ([\d.]+)"', text)
    return (float(m.group(1)), float(m.group(2))) if m else None


def render(chrome):
    tmp = bake / "tmp"
    tmp.mkdir(parents=True, exist_ok=True)
    trivial = tmp / "trivial.mmd"
    trivial.write_text(TRIVIAL)
    for tool in TOOLS[:3]:
        d = out / tool
        shutil.rmtree(d, ignore_errors=True)
        d.mkdir(parents=True)
        # Process start-up and a minimal render: subtracted from per-diagram wall times.
        base, _ = best_of(cli(tool, trivial, tmp / f"trivial-{tool}.png", "png"), 7)
        svg_base, _ = best_of(cli(tool, trivial, tmp / f"trivial-{tool}.svg", "svg"), 7)
        times = {}
        for f in corpus:
            png, svg = d / f"{f.stem}.png", d / f"{f.stem}.svg"
            wall, p = best_of(cli(tool, f, png, "png"), 3)
            if wall is None or not png.exists():
                (d / f"{f.stem}.error").write_text((p.stderr or p.stdout or f"exit {p.returncode}").strip()[:2000])
                png.unlink(missing_ok=True)
                continue
            times[f.stem] = {"wall_ms": round(wall * 1000, 2), "net_ms": round(max(0, wall - base) * 1000, 2)}
            svg_wall, _ = best_of(cli(tool, f, svg, "svg"), 3)
            if svg_wall is not None:
                times[f.stem]["svg_net_ms"] = round(max(0, svg_wall - svg_base) * 1000, 2)
        (d / "timings.json").write_text(json.dumps({"startup_ms": round(base * 1000, 2), "diagrams": times}, indent=1))
        print(f"{tool}: {len(times)}/{len(corpus)} rendered, start-up+trivial {base*1000:.1f} ms")
    # BeautifulMermaid: in-process, warm (bm-cli times a second render of each diagram).
    d = out / "beautifulmermaid"
    shutil.rmtree(d, ignore_errors=True)
    p = subprocess.run([BM, here / "corpus", d], capture_output=True, text=True, timeout=600)
    print(p.stdout.strip() or p.stderr.strip()[-500:])
    # mermaid.js reference SVG -> PNG at 2x in headless Chromium.
    d = out / "mermaidjs"
    for f in corpus:
        svg = d / f"{f.stem}.svg"
        if not svg.exists():
            continue
        w, h = svg_size(svg.read_text()) or (800, 600)
        w, h = int(w + 0.999), int(h + 0.999)
        page = tmp / f"{f.stem}.html"
        # Inline, as mermaid.js output is HTML-flavoured (e.g. <br> in labels), not always well-formed XML.
        markup = re.sub(r'style="max-width:[^"]*"', '', svg.read_text(), count=1)
        markup = re.sub(r'<svg ', f'<svg width="{w}" height="{h}" ', markup, count=1)
        page.write_text(f'<!doctype html><style>html,body{{margin:0;background:#fff}}svg{{display:block}}</style>{markup}')
        subprocess.run([chrome, "--headless", "--disable-gpu", "--hide-scrollbars", "--allow-file-access-from-files",
                        f"--window-size={w},{h}", "--force-device-scale-factor=2", f"--screenshot={d / (f.stem + '.png')}",
                        page.as_uri()], capture_output=True, timeout=120)
    print("mermaid.js PNGs:", len(list(d.glob("*.png"))))


def labels(svg_text):
    """Visible label words in an SVG: <text>/<tspan> and HTML labels in foreignObject."""
    s = re.sub(r"<style.*?</style>", " ", svg_text, flags=re.S)
    s = re.sub(r"<title.*?</title>|<desc.*?</desc>", " ", s, flags=re.S)
    words = set()
    for chunk in re.findall(r">([^<>]+)<", s):
        for w in re.findall(r"[A-Za-z0-9][A-Za-z0-9_.\-/]*", html.unescape(chunk)):
            if len(w) > 1:
                words.add(w.lower())
    return words


def metrics():
    ref = out / "mermaidjs"
    results = {}
    for tool in ["mermaidjs"] + TOOLS:
        d = out / tool
        t = json.loads((d / "timings.json").read_text()) if (d / "timings.json").exists() else {}
        per = {}
        for f in corpus:
            name, kind = f.stem, f.stem.split("-")[0]
            dtype = f.read_text().split()[0]
            r = {"type": dtype, "set": kind}
            err = d / f"{name}.error"
            png = d / f"{name}.png"
            if err.exists() or not png.exists():
                r["error"] = err.read_text()[:300] if err.exists() else "no output"
            else:
                size = png_size(png)
                scale = 1 if tool in ("mmdr", "selkie") else 2
                r["size"] = [size[0] / scale, size[1] / scale] if size else None
                if tool == "mermaidjs":
                    r["ms"] = t.get(name)
                elif tool == "beautifulmermaid":
                    r["ms"] = t.get(name)
                    draw = d / "draw-timings.json"
                    r["svg_ms"] = json.loads(draw.read_text()).get(name) if draw.exists() else None
                else:
                    r["ms"] = t.get("diagrams", {}).get(name, {}).get("net_ms")
                    r["wall_ms"] = t.get("diagrams", {}).get(name, {}).get("wall_ms")
                    r["svg_ms"] = t.get("diagrams", {}).get(name, {}).get("svg_net_ms")
                ref_svg = ref / f"{name}.svg"
                cand_svg = d / f"{name}.svg"
                if tool != "mermaidjs" and ref_svg.exists():
                    want = labels(ref_svg.read_text())
                    if cand_svg.exists():
                        got = labels(cand_svg.read_text())
                        r["label_recall"] = round(len(want & got) / len(want), 3) if want else 1.0
                        r["missing"] = sorted(want - got)[:12]
            per[name] = r
        results[tool] = per
    (out / "results.json").write_text(json.dumps(results, indent=1))
    ref_sizes = {n: r.get("size") for n, r in results["mermaidjs"].items()}
    print(f"{'tool':18} {'ours ok':>8} {'types ok':>8} {'all %':>6} {'png ms':>8} {'vec ms':>8} {'labels':>7} {'aspect±':>8}")
    for tool, per in results.items():
        ok = [n for n, r in per.items() if "error" not in r]
        ours = [n for n in ok if per[n]["set"] == "ours"]
        types = [n for n in ok if per[n]["set"] == "types"]
        ms = [per[n]["ms"] for n in ok if per[n].get("ms") is not None]
        vec = [per[n]["svg_ms"] for n in ok if per[n].get("svg_ms") is not None]
        rec = [per[n]["label_recall"] for n in ok if "label_recall" in per[n]]
        asp = []
        for n in ok:
            a, b = per[n].get("size"), ref_sizes.get(n)
            if a and b and a[1] and b[1]:
                asp.append(abs((a[0] / a[1]) / (b[0] / b[1]) - 1))
        print(f"{tool:18} {len(ours):>5}/38 {len(types):>5}/12 {100*len(ok)/len(corpus):>5.0f}% "
              f"{(statistics.median(ms) if ms else float('nan')):>8.2f} {(statistics.median(vec) if vec else float('nan')):>8.2f} "
              f"{(statistics.mean(rec) if rec else float('nan')):>7.2f} {(statistics.median(asp) if asp else float('nan')):>8.2f}")


def composites(chrome):
    d = out / "composites"
    shutil.rmtree(d, ignore_errors=True)
    d.mkdir(parents=True)
    cols = ["mermaidjs"] + TOOLS
    names = [f.stem for f in corpus]
    for i in range(0, len(names), 4):
        batch = names[i:i + 4]
        rows = []
        for n in batch:
            cells = []
            for tool in cols:
                png, err = out / tool / f"{n}.png", out / tool / f"{n}.error"
                cell = (f'<img src="{png.as_uri()}">' if png.exists()
                        else f'<div class=err>{html.escape(err.read_text()[:160]) if err.exists() else "no output"}</div>')
                cells.append(f"<td>{cell}</td>")
            rows.append(f'<tr><th colspan={len(cols)}>{n}</th></tr><tr>{"".join(cells)}</tr>')
        head = "".join(f"<th>{c}</th>" for c in cols)
        page = bake / "tmp" / f"composite-{i//4:02d}.html"
        page.write_text(f"""<!doctype html><style>
body{{margin:0;font:13px -apple-system,sans-serif;background:#fff}}table{{border-collapse:collapse;table-layout:fixed;width:1500px}}
td,th{{border:1px solid #ccc;padding:4px;vertical-align:top;width:300px}}th{{background:#f3f3f3;text-align:left}}
img{{max-width:290px;max-height:330px;display:block;margin:auto}}.err{{color:#b00;font:11px monospace;white-space:pre-wrap}}
</style><table><tr>{head}</tr>{''.join(rows)}</table>""")
        subprocess.run([chrome, "--headless", "--disable-gpu", "--hide-scrollbars", "--allow-file-access-from-files",
                        "--window-size=1500,1600", f"--screenshot={d / f'composite-{i//4:02d}.png'}", page.as_uri()],
                       capture_output=True, timeout=120)
    print("composites:", len(list(d.glob("*.png"))))


if __name__ == "__main__":
    {"render": lambda: render(sys.argv[2]), "metrics": metrics, "composites": lambda: composites(sys.argv[2])}[sys.argv[1]]()
