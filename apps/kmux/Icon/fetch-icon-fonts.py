#!/usr/bin/env python3 -I
"""Fetches the fonts tried for the kmux app icon (make-icon.swift --font-sheet).

    python3 -I apps/kmux/Icon/fetch-icon-fonts.py [OUT]     # OUT defaults to target/icon-fonts

Downloads JetBrains Mono, Hack, VT323 and every Nerd Fonts family (latest release, .tar.xz
assets) into OUT, keeps one face per family -- Bold, or the heaviest regular-width weight
when there is no Bold -- plus its licence file, and deletes each archive after extracting.
Archives are untrusted: members are read by name and written to flat paths we choose, never
extracted as-is. Writes OUT/fonts.tsv (label, font path, licence path, note) for make-icon.swift.
Fonts are not installed; make-icon.swift registers them for its own process only.
"""
import io, json, os, re, subprocess, sys, tarfile, zipfile

OUT = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "target/icon-fonts")
LIMIT = 2_000_000_000  # stop if downloads pass ~2 GB

FONT = re.compile(r"\.(ttf|otf)$", re.I)
NOT_REGULAR_WIDTH = re.compile(r"italic|oblique|condensed|narrow|expanded|extended|wide|compressed|slanted|retina", re.I)
LICENCE = re.compile(r"(^|/)(licen[cs]e|ofl|copying)[^/]*(\.txt|\.md)?$", re.I)
# Heaviest first; "Bold" itself is preferred over all of these.
WEIGHTS = ["black", "heavy", "ultrabold", "extrabold", "semibold", "demibold", "medium", "regular", "book"]

downloaded = 0


def fetch(url):
    global downloaded
    data = subprocess.run(["curl", "-sSfL", "--retry", "4", "--retry-all-errors", url], check=True, capture_output=True).stdout
    downloaded += len(data)
    if downloaded > LIMIT:
        sys.exit(f"stopped: downloads passed {LIMIT / 1e9:.1f} GB")
    return data


def style(name):
    base = os.path.splitext(os.path.basename(name))[0]
    return base.rsplit("-", 1)[1].lower() if "-" in base else "regular"


def pick(names):
    """The face to keep from an archive's font files, and a note on which weight it is."""
    fonts = [n for n in names if FONT.search(n) and not NOT_REGULAR_WIDTH.search(os.path.basename(n))]
    if not fonts:
        return None, "no regular-width faces"
    # Nerd Fonts ship NerdFont (normal), NerdFontMono and NerdFontPropo; prefer the normal one.
    def variant(n):
        b = os.path.basename(n)
        return 2 if "NerdFontPropo" in b else 1 if "NerdFontMono" in b else 0
    best = min(variant(n) for n in fonts)
    fonts = sorted(n for n in fonts if variant(n) == best)
    bold = [n for n in fonts if style(n) == "bold"]
    if bold:
        return bold[0], "Bold"
    for w in WEIGHTS:
        hit = [n for n in fonts if style(n) == w]
        if hit:
            return hit[0], f"no Bold; used {style(hit[0])}"
    return fonts[0], f"no Bold; used {style(fonts[0])}"


def save(family, member_name, data, kind):
    d = os.path.join(OUT, family)
    os.makedirs(d, exist_ok=True)
    ext = os.path.splitext(member_name)[1].lower() if kind == "font" else ".txt"
    path = os.path.join(d, ("font" if kind == "font" else "LICENSE") + ext)
    with open(path, "wb") as f:
        f.write(data)
    return path


def from_archive(family, blob, kind):
    if kind == "zip":
        z = zipfile.ZipFile(io.BytesIO(blob))
        names = [i.filename for i in z.infolist() if not i.is_dir()]
        read = z.read
    else:
        t = tarfile.open(fileobj=io.BytesIO(blob), mode="r:xz")
        members = {m.name: m for m in t.getmembers() if m.isfile()}
        names = list(members)
        read = lambda n: t.extractfile(members[n]).read()
    font, note = pick(names)
    if not font:
        return None, None, note
    lic = sorted((n for n in names if LICENCE.search(n)), key=len)
    lic_path = save(family, lic[0], read(lic[0]), "licence") if lic else ""
    return save(family, font, read(font), "font"), lic_path, f"{os.path.basename(font)}: {note}"


def main():
    os.makedirs(OUT, exist_ok=True)
    rows = []

    def add(family, path, lic, note):
        rows.append((family, path or "", lic or "", note))
        print(f"{family:28} {note}")

    # JetBrains Mono: GitHub release zip.
    rel = json.loads(fetch("https://api.github.com/repos/JetBrains/JetBrainsMono/releases/latest"))
    zipurl = next(a["browser_download_url"] for a in rel["assets"] if a["name"].endswith(".zip"))
    add("JetBrains Mono", *from_archive("JetBrains Mono", fetch(zipurl), "zip"))
    # Hack: source-foundry release, ttf zip.
    rel = json.loads(fetch("https://api.github.com/repos/source-foundry/Hack/releases/latest"))
    zipurl = next(a["browser_download_url"] for a in rel["assets"] if a["name"].endswith("-ttf.zip"))
    add("Hack", *from_archive("Hack", fetch(zipurl), "zip"))
    # VT323: Google Fonts repo (one weight only).
    raw = "https://raw.githubusercontent.com/google/fonts/main/ofl/vt323/"
    add("VT323", save("VT323", "VT323-Regular.ttf", fetch(raw + "VT323-Regular.ttf"), "font"),
        save("VT323", "OFL.txt", fetch(raw + "OFL.txt"), "licence"), "VT323-Regular.ttf: no Bold; used regular")

    # Nerd Fonts: every .tar.xz asset of the latest release.
    rel = json.loads(fetch("https://api.github.com/repos/ryanoasis/nerd-fonts/releases/latest"))
    for a in sorted(rel["assets"], key=lambda a: a["name"].lower()):
        if not a["name"].endswith(".tar.xz"):
            continue
        family = "NF " + a["name"][: -len(".tar.xz")]
        try:
            add(family, *from_archive(family, fetch(a["browser_download_url"]), "tar"))
        except Exception as e:  # keep going; the sheet lists the failure
            add(family, "", "", f"failed: {e}")

    with open(os.path.join(OUT, "fonts.tsv"), "w") as f:
        for r in rows:
            f.write("\t".join(r) + "\n")
    print(f"{len(rows)} families, {downloaded / 1e6:.0f} MB downloaded; list in {OUT}/fonts.tsv")


main()
