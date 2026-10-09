#!/usr/bin/env python3
"""kmux's native diagrams (KmuxDiagram, via apps/kmux kmux-diagram) against the
mermaid.js reference: label recall, size ratio, and side-by-side composites.

  compare-native.py CHROME [KMUX_DIAGRAM]
    -> target/diagram/out/NAME.png, report.json, compare.json
    -> target/diagram/composites/compare-NN.png  (mermaid.js | native)
Run spikes/mermaid-bakeoff/run.sh first for the mermaid.js reference.
"""
import html, json, re, shutil, statistics, subprocess, sys, pathlib
import bakeoff

root = bakeoff.root
out = root / "target/diagram/out"
comp = root / "target/diagram/composites"
ref = bakeoff.out / "mermaidjs"
chrome = sys.argv[1]
tool = sys.argv[2] if len(sys.argv) > 2 else str(root / "apps/kmux/.build/release/kmux-diagram")

shutil.rmtree(out, ignore_errors=True)
subprocess.run([tool, *map(str, bakeoff.corpus), "--out", str(out)], check=True)
report = json.loads((out / "report.json").read_text())

rows = {}
for f in bakeoff.corpus:
    name, r = f.stem, report.get(f.stem, {})
    svg = (ref / f"{name}.svg").read_text() if (ref / f"{name}.svg").exists() else ""
    if "error" in r or not svg:
        rows[name] = {"drawn": False, "error": r.get("error", "no reference")[:120]}
        continue
    want = bakeoff.labels(svg)
    have = set()
    for label in r["labels"]:
        have |= {w.lower() for w in re.findall(r"[A-Za-z0-9][A-Za-z0-9_.\-/]*", html.unescape(label)) if len(w) > 1}
    missing = sorted(want - have)
    w0, h0 = bakeoff.svg_size(svg) or (1, 1)
    rows[name] = {"drawn": True, "recall": round(1 - len(missing) / max(len(want), 1), 3), "missing": missing[:8],
                  "width_ratio": round(r["width"] / w0, 2), "height_ratio": round(r["height"] / h0, 2),
                  "layout_ms": round(r["layout_ms"], 2), "draw_ms": round(r["draw_ms"], 2)}
(out / "compare.json").write_text(json.dumps(rows, indent=1))

drawn = [r for r in rows.values() if r["drawn"]]
print(f"drawn {len(drawn)}/{len(rows)}; label recall {statistics.mean(r['recall'] for r in drawn):.3f} "
      f"(perfect in {sum(r['recall'] == 1 for r in drawn)}); size within 25% in "
      f"{sum(0.75 <= r['width_ratio'] <= 1.33 and 0.75 <= r['height_ratio'] <= 1.33 for r in drawn)}; "
      f"median layout {statistics.median(r['layout_ms'] for r in drawn):.2f} ms, draw {statistics.median(r['draw_ms'] for r in drawn):.2f} ms")
for name, r in rows.items():
    if r["drawn"] and (r["recall"] < 1 or not (0.75 <= r["width_ratio"] <= 1.33 and 0.75 <= r["height_ratio"] <= 1.33)):
        print(f"  {name}: recall {r['recall']} missing {r['missing']} size {r['width_ratio']}x{r['height_ratio']}")

shutil.rmtree(comp, ignore_errors=True)
comp.mkdir(parents=True)
names = [n for n in rows if n.startswith("ours-") or rows[n]["drawn"]]
for i in range(0, len(names), 3):
    trs = "".join(f'<tr><th colspan=2>{n}</th></tr><tr><td><img src="{(ref / (n + ".png")).as_uri()}"></td>'
                  f'<td><img src="{(out / (n + ".png")).as_uri()}"></td></tr>' for n in names[i:i + 3])
    page = bakeoff.bake / "tmp" / f"compare-{i // 3:02d}.html"
    page.write_text(f"""<!doctype html><style>body{{margin:0;font:13px -apple-system,sans-serif;background:#fff}}
table{{border-collapse:collapse;table-layout:fixed;width:1380px}}td,th{{border:1px solid #ccc;padding:4px;vertical-align:top;width:680px;overflow:hidden}}
th{{background:#f3f3f3;text-align:left}}img{{width:100%;height:auto;max-height:520px;object-fit:contain;display:block}}</style>
<table><tr><th>mermaid.js 12.1.0</th><th>kmux native (merman layout)</th></tr>{trs}</table>""")
    subprocess.run([chrome, "--headless", "--disable-gpu", "--hide-scrollbars", "--allow-file-access-from-files", "--window-size=1400,1700",
                    f"--screenshot={comp / f'compare-{i // 3:02d}.png'}", page.as_uri()], capture_output=True, timeout=120)
print("composites:", len(list(comp.glob("*.png"))), "in", comp)
