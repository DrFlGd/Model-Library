#!/usr/bin/env python3
"""Prepare the desktop app's front end for Tauri.

    python3 tools/build_desktop.py --out build/desktop [--fetch-fonts]

Copies web/ to <out>/ui (tauri.conf.json's frontendDist), with CHANGELOG.md,
THIRD_PARTY.md and build-info.json (date, commit, vendored library versions)
for the About page. The app works offline,
so the Google Fonts lines are dropped from index.html: with --fetch-fonts (CI)
the Archivo font (SIL OFL) is bundled instead, otherwise system fonts are used.
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
                help="bundle the Archivo UI font (SIL OFL, from google/fonts) instead of loading it from Google Fonts")
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

FONT_URL = "https://raw.githubusercontent.com/google/fonts/main/ofl/archivo/"
index = ui / "index.html"
html = index.read_text(encoding="utf-8")
lines = [l for l in html.splitlines() if "fonts.googleapis.com" not in l and "fonts.gstatic.com" not in l]
font_css = ""
if a.fetch_fonts:
    import urllib.request
    fonts = ui / "vendor/fonts"
    fonts.mkdir(parents=True, exist_ok=True)
    for remote, local in (("Archivo%5Bwdth%2Cwght%5D.ttf", "Archivo-Variable.ttf"), ("OFL.txt", "Archivo-OFL.txt")):
        (fonts / local).write_bytes(urllib.request.urlopen(FONT_URL + remote, timeout=60).read())
    (fonts / "archivo.css").write_text(
        "@font-face { font-family: 'Archivo'; src: url('Archivo-Variable.ttf') format('truetype');\n"
        "  font-weight: 100 900; font-stretch: 62% 125%; font-display: swap; }\n")
    font_css = '  <link rel="stylesheet" href="vendor/fonts/archivo.css">'
out_lines = []
for l in lines:
    if font_css and 'href="styles.css"' in l:
        out_lines.append(font_css)
    out_lines.append(l)
index.write_text("\n".join(out_lines) + "\n", encoding="utf-8")

size = sum(f.stat().st_size for f in ui.rglob("*") if f.is_file())
print(f"ui {size / 1e6:.1f} MB -> {ui}")
