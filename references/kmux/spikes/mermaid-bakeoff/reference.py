#!/usr/bin/env python3
"""Renders every corpus diagram with mermaid.js in headless Chromium: the reference.
Writes target/bakeoff/out/mermaidjs/NAME.svg (or NAME.error) and timings.json."""
import html, json, os, re, subprocess, sys, pathlib
here = pathlib.Path(__file__).parent
root = here.parent.parent
out = root / "target/bakeoff/out/mermaidjs"
out.mkdir(parents=True, exist_ok=True)
chrome = sys.argv[1]
corpus = {p.stem: p.read_text() for p in sorted((here / "corpus").glob("*.mmd"))}
page = root / "target/bakeoff/reference.html"
page.write_text(f"""<!doctype html><meta charset=utf-8>
<script src="mermaid-12.1.0.min.js"></script>
<body><pre id=result></pre><script>
const corpus = {json.dumps(corpus)};
mermaid.initialize({{ startOnLoad: false, securityLevel: 'strict' }});
(async () => {{
  const result = {{}};
  let i = 0;
  for (const [name, src] of Object.entries(corpus)) {{
    try {{
      await mermaid.render('w' + i++, src);          // warm-up
      const t = performance.now();
      const {{ svg }} = await mermaid.render('d' + i++, src);
      result[name] = {{ ms: performance.now() - t, svg }};
    }} catch (e) {{ result[name] = {{ error: String(e.message || e) }}; }}
  }}
  document.getElementById('result').textContent = JSON.stringify(result);
  document.title = 'done';
}})();
</script>""")
dom = subprocess.run([chrome, "--headless", "--disable-gpu", "--allow-file-access-from-files",
                      "--virtual-time-budget=600000", "--run-all-compositor-stages-before-draw",
                      "--dump-dom", page.as_uri()], capture_output=True, text=True, timeout=600).stdout
data = json.loads(html.unescape(re.search(r'<pre id="result">(.*?)</pre>', dom, re.S).group(1)))
times = {}
for name, r in data.items():
    for old in out.glob(name + ".*"): old.unlink()
    if "svg" in r:
        (out / f"{name}.svg").write_text(r["svg"]); times[name] = round(r["ms"], 2)
    else:
        (out / f"{name}.error").write_text(r["error"])
(out / "timings.json").write_text(json.dumps(times, indent=1))
print(f"mermaid.js: {len(times)}/{len(data)} rendered")
