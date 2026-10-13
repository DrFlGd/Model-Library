#!/usr/bin/env python3
"""Prepare the desktop app's front end for Tauri.

    python3 tools/build_desktop.py --out build/desktop [--fetch-fonts]

Copies web/ to <out>/ui (tauri.conf.json's frontendDist), with CHANGELOG.md,
THIRD_PARTY.md and build-info.json (date, commit, vendored library versions)
for the About page. The app works offline,
so the Google Fonts lines are dropped from index.html: with --fetch-fonts (CI)
the skins' fonts (all SIL OFL, from google/fonts) are bundled instead, otherwise
system fonts are used. A font file is only read when a skin in use needs it.
"""
import argparse
import datetime
import json
import os
import re
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ap = argparse.ArgumentParser()
ap.add_argument("--out", type=Path, default=ROOT / "build/desktop")
ap.add_argument("--fetch-fonts", action="store_true",
                help="bundle the skins' fonts (SIL OFL, from google/fonts) instead of loading them from Google Fonts")
a = ap.parse_args()

ui = a.out / "ui"
if ui.exists():
    shutil.rmtree(ui)
shutil.copytree(ROOT / "web", ui)

# Tauri infers some MIME types from content before considering extensions.
# Keep the PDF magic marker out of JavaScript, including comments, so WebKitGTK
# never treats an ES module as application/pdf. Scan the whole small JS asset:
# a marker crossing a 1 KiB sniff-window boundary must also be caught.
for script in ui.rglob("*"):
    if script.is_file() and script.suffix.lower() in (".js", ".mjs") and b"%PDF" in script.read_bytes():
        raise SystemExit(
            f"{script.relative_to(ui)} contains the PDF magic marker; "
            "Tauri can serve this JavaScript module as application/pdf."
        )

# The About page reads these next to index.html.
for doc in ("CHANGELOG.md", "THIRD_PARTY.md"):
    shutil.copy2(ROOT / doc, ui / doc)


def commit():
    sha = os.environ.get("GITHUB_SHA")
    if not sha:
        try:
            sha = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=True).stdout.strip()
        except (OSError, subprocess.CalledProcessError):
            return None
    return sha[:7] or None


def found(path, pattern):
    m = re.search(pattern, (ROOT / path).read_text(encoding="utf-8", errors="replace"))
    return m.group(1) if m else None


parts = []
for name, path, pattern, fmt in (
    ("Preact", "web/vendor/PREACT_NOTICE.txt", r"Preact (\d+\.\d+\.\d+)", "{}"),
    ("htm", "web/vendor/PREACT_NOTICE.txt", r"htm \(\S+ @ ([0-9a-f]{7})", "@ {}"),
    ("three.js", "web/vendor/three/three.core.js", r"REVISION = '(\d+)'", "r{}"),
    ("7z reader (sevenz-rust2)", "desktop/core/Cargo.toml", r'sevenz-rust2 = \{ version = "([\d.]+)"', "{}"),
    ("RAR reader (unrar)", "desktop/core/Cargo.toml", r'\nunrar = "([\d.]+)"', "{}"),
):
    v = found(path, pattern)
    if v:
        parts.append({"name": name, "version": fmt.format(v)})
(ui / "build-info.json").write_text(json.dumps({
    "date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%d"),
    "commit": commit(),
    "parts": parts,
}, indent=2) + "\n", encoding="utf-8")

FONT_URL = "https://raw.githubusercontent.com/google/fonts/main/ofl/"
# The skins' fonts (docs/PLAN.md, "Skins design"): (family, folder in google/fonts,
# [(file, weights)], widths). A family whose folder has no OFL.txt gets the
# licence's text with the copyright line from its METADATA.pb.
FONTS = [
    ("Archivo", "archivo", [("Archivo%5Bwdth%2Cwght%5D.ttf", "100 900")], "62% 125%"),
    ("Overpass", "overpass", [("Overpass%5Bwght%5D.ttf", "100 900")], None),
    ("Overpass Mono", "overpassmono", [("OverpassMono%5Bwght%5D.ttf", "300 700")], None),
    ("Sofia Sans", "sofiasans", [("SofiaSans%5Bwght%5D.ttf", "1 1000")], None),
    ("Sofia Sans Condensed", "sofiasanscondensed", [("SofiaSansCondensed%5Bwght%5D.ttf", "1 1000")], None),
    ("Sofia Sans Extra Condensed", "sofiasansextracondensed", [("SofiaSansExtraCondensed%5Bwght%5D.ttf", "1 1000")], None),
    ("Atkinson Hyperlegible", "atkinsonhyperlegible", [("AtkinsonHyperlegible-Regular.ttf", "400 600"), ("AtkinsonHyperlegible-Bold.ttf", "700 900")], None),
    # Layer Line's headings: one heavy weight for all of them, cut to Latin
    # letters and the app's symbols when fontTools is there (3.6 MB to ~100 KB).
    ("M PLUS Rounded 1c", "mplusrounded1c", [("MPLUSRounded1c-ExtraBold.ttf", "100 1000")], None),
]
SUBSET = "U+0000-024F,U+02B0-02FF,U+2000-206F,U+2070-209F,U+20A0-20CF,U+2100-214F,U+2190-21FF,U+2200-22FF,U+2300-23FF,U+25A0-25FF,U+2600-27BF"
index = ui / "index.html"
html = index.read_text(encoding="utf-8")
lines = [l for l in html.splitlines() if "fonts.googleapis.com" not in l and "fonts.gstatic.com" not in l]
font_css = ""
if a.fetch_fonts:
    import io, re, urllib.request
    get = lambda path: urllib.request.urlopen(FONT_URL + path, timeout=60).read()
    fonts = ui / "vendor/fonts"
    fonts.mkdir(parents=True, exist_ok=True)
    faces = []
    for family, folder, files, widths in FONTS:
        name = family.replace(" ", "")
        for remote, weights in files:
            data = get(folder + "/" + remote)
            if family == "M PLUS Rounded 1c":
                try:
                    from fontTools import subset
                    from fontTools.ttLib import TTFont
                    font = TTFont(io.BytesIO(data))
                    sub = subset.Subsetter(subset.Options(layout_features=["*"], name_IDs=["*"], notdef_outline=True))
                    sub.populate(unicodes=subset.parse_unicodes(SUBSET))
                    sub.subset(font)
                    out = io.BytesIO()
                    font.save(out)
                    data = out.getvalue()
                except ImportError:
                    print(f"fontTools isn't installed: {family} is bundled whole")
            local = name + "-Variable.ttf" if "%5B" in remote else remote
            (fonts / local).write_bytes(data)
            faces.append(f"@font-face {{ font-family: '{family}'; src: url('{local}') format('truetype');\n"
                         f"  font-weight: {weights};{f' font-stretch: {widths};' if widths else ''} font-display: swap; }}\n")
        try:
            licence = get(folder + "/OFL.txt")
        except Exception:
            meta = get(folder + "/METADATA.pb").decode("utf-8")
            owner = re.search(r'copyright: "([^"]+)"', meta).group(1)
            body = get("atkinsonhyperlegible/OFL.txt").decode("utf-8")
            licence = (owner + "\n\n" + body[body.index("This Font Software is licensed"):]).encode("utf-8")
        (fonts / (name + "-OFL.txt")).write_bytes(licence)
    (fonts / "fonts.css").write_text("".join(faces))
    font_css = '  <link rel="stylesheet" href="vendor/fonts/fonts.css">'
out_lines = []
for l in lines:
    if font_css and 'href="styles.css"' in l:
        out_lines.append(font_css)
    out_lines.append(l)
index.write_text("\n".join(out_lines) + "\n", encoding="utf-8")

size = sum(f.stat().st_size for f in ui.rglob("*") if f.is_file())
print(f"ui {size / 1e6:.1f} MB -> {ui}")
