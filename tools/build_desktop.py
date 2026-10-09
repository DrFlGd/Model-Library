#!/usr/bin/env python3
"""Prepare the desktop app's front end for Tauri.

    python3 tools/build_desktop.py --out build/desktop [--fetch-fonts]

Copies web/ to <out>/ui (tauri.conf.json's frontendDist). The app works offline,
so the Google Fonts lines are dropped from index.html: with --fetch-fonts (CI)
the Archivo font (SIL OFL) is bundled instead, otherwise system fonts are used.
"""
import argparse
import shutil
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
