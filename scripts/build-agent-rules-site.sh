#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: scripts/build-agent-rules-site.sh OUT

Writes the rules site into OUT/agent-rules: every agent-rules/<agent>.json
and an index.json listing each one's agent, file, version and sha256, as
Kanna's daemon reads it (docs/specs/agent-status.md). GitHub Pages serves
OUT; .github/workflows/agent-rules.yml runs this after the corpus passes.
USAGE
}

[[ $# -eq 1 && "$1" != -* ]] || { usage >&2; exit 2; }
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out="$1/agent-rules"
mkdir -p "$out"
python3 - "$repo_root/agent-rules" "$out" <<'PY'
import hashlib, json, pathlib, re, shutil, sys
source, out = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
entries = []
for path in sorted(source.glob("*.json")):
    if path.stem == "index":
        continue
    if not re.fullmatch(r"[a-z0-9-]{1,32}", path.stem):
        sys.exit(f"{path.name}: rule files are named <agent>.json, a-z, 0-9 and -")
    data = path.read_bytes()
    rules = json.loads(data)
    if rules.get("agent") != path.stem:
        sys.exit(f"{path.name} is for agent {rules.get('agent')}")
    shutil.copyfile(path, out / path.name)
    entries.append({"agent": path.stem, "url": path.name, "version": rules["version"], "sha256": hashlib.sha256(data).hexdigest()})
(out / "index.json").write_text(json.dumps({"schema": 1, "rules": entries}, indent=2) + "\n")
print(f"{len(entries)} rule files in {out}")
PY
