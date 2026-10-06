#!/usr/bin/env python3
"""Acceptance test: the desktop app's page against its real backend
(`modlib-cli serve`, the same command table as the Tauri app), in Chromium.

    python3 tests/desktop_page.py --cli desktop/target/release/modlib-cli --ui build/desktop/ui \\
        --home /tmp/home --out shots [--chromium <path>]

Checks (Phase 0): the app starts with a new library (its _library/ folder and
Unsorted/), the menu's places, renaming the library, opening a second library
and switching back from the recent list, a folder that already holds models is
left as it was, the theme follows the setting, a library from a newer app opens
read-only, and the library opens the same after being moved (library-info).
Writes <out>/library-info.json and <out>/library.zip for the cross-platform check.
"""
import argparse
import asyncio
import json
import shutil
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

from playwright.async_api import async_playwright

ap = argparse.ArgumentParser()
ap.add_argument("--cli", required=True)
ap.add_argument("--ui", required=True)
ap.add_argument("--home", required=True)
ap.add_argument("--out", default="shots")
ap.add_argument("--port", type=int, default=8790)
ap.add_argument("--chromium", help="a Chromium to use instead of Playwright's own")
a = ap.parse_args()
out = Path(a.out)
out.mkdir(parents=True, exist_ok=True)
home = Path(a.home).resolve()
shutil.rmtree(home, ignore_errors=True)
library = home / "My Models"
B = f"http://127.0.0.1:{a.port}/"
SHIM = Path(__file__).with_name("tauri_shim.js")
results, errors = [], []


def check(name, ok, detail=""):
    results.append((name, bool(ok)))
    print(f"{'PASS' if ok else 'FAIL'} {name}{' :: ' + str(detail) if detail != '' else ''}", flush=True)


def start_server():
    log = open(out / "serve.log", "w")
    p = subprocess.Popen([a.cli, "serve", "--ui", a.ui, "--home", str(home), "--library", str(library), "--port", str(a.port)], stdout=log, stderr=log)
    for _ in range(100):
        try:
            urllib.request.urlopen(B, timeout=1)
            return p
        except Exception:
            time.sleep(0.2)
    raise SystemExit("the server didn't start; see serve.log")


def pick(path):
    """The answer to the next folder dialog."""
    req = urllib.request.Request(B + "test/pick", data=json.dumps({"path": str(path)}).encode(), method="POST", headers={"content-type": "application/json"})
    urllib.request.urlopen(req, timeout=5)


def library_info(path):
    r = subprocess.run([a.cli, "library-info", "--library", str(path)], capture_output=True, text=True)
    if r.returncode:
        print(r.stderr)
    return json.loads(r.stdout)


async def main():
    server = start_server()
    try:
        async with async_playwright() as p:
            b = await p.chromium.launch(executable_path=a.chromium)
            ctx = await b.new_context(viewport={"width": 1400, "height": 900})
            await ctx.add_init_script(path=str(SHIM))
            pg = await ctx.new_page()
            pg.on("pageerror", lambda e: errors.append(str(e)))
            pg.on("console", lambda m: errors.append(f"{m.text} ({m.location.get('url', '')})") if m.type == "error" and "net::ERR_" not in m.text else None)
            await pg.goto(B)
            await pg.wait_for_selector(".home h1", timeout=30000)

            # 1. a new library, made on first start
            title = await pg.inner_text(".home h1")
            made = (library / "_library/library.json").is_file() and (library / "Unsorted").is_dir() and (library / "_library/schemas").is_dir()
            status = await pg.inner_text(".statusbar")
            check("a new library is made on first start", title == "My Models" and made and str(library) in status, (title, made, status))
            await pg.screenshot(path=str(out / "00-home.png"))

            # 2. the menu's places
            labels = await pg.eval_on_selector_all(".sidebar .nav-label", "els => els.map(e => e.textContent.trim())")
            await pg.click('.sidebar a[href="#/browse/unsorted"]')
            await pg.wait_for_selector(".browse-empty h1")
            unsorted = await pg.inner_text(".browse-empty h1")
            active = await pg.inner_text(".sidebar .nav-link.active")
            check("the menu's places open", labels == ["Home", "All models", "Unsorted", "Favourites", "Settings"] and unsorted == "Unsorted" and active == "Unsorted", (labels, unsorted, active))

            # 3. renaming the library
            await pg.click('.sidebar a[href="#/settings"]')
            await pg.wait_for_selector("#library-name")
            await pg.fill("#library-name", "Minis")
            await pg.click(".inline-form button[type=submit]")
            await pg.wait_for_function("() => document.querySelector('.statusbar').textContent.includes('Minis')")
            stored = json.loads((library / "_library/library.json").read_text())["name"]
            check("the library is renamed", stored == "Minis", stored)
            await pg.screenshot(path=str(out / "01-settings.png"), full_page=True)

            # 4. opening another library (a folder that already holds models), then switching back
            other = home / "Old models"
            (other / "Wargames/Tyranid").mkdir(parents=True)
            (other / "Wargames/Tyranid/hive tyrant.stl").write_text("solid x\nendsolid x\n")
            pick(other)
            await pg.click("#open-library")
            await pg.wait_for_function("() => document.querySelector('#library-path')?.textContent.includes('Old models')")
            kept = (other / "Wargames/Tyranid/hive tyrant.stl").read_text().startswith("solid x")
            top = sorted(x.name for x in other.iterdir())
            check("opening a folder of models adds only the app's folders", kept and top == ["Unsorted", "Wargames", "_library"], top)
            await pg.click("#recent-libraries button")
            await pg.wait_for_function("() => document.querySelector('#library-path')?.textContent.endsWith('My Models')")
            name = await pg.input_value("#library-name")
            check("a library opened before opens from the list", name == "Minis", name)

            # 5. the theme
            await pg.select_option("#theme-select", "night")
            theme = await pg.evaluate("() => document.documentElement.dataset.theme")
            await pg.wait_for_timeout(500)  # preferences are saved after a short pause
            saved = json.loads((home / "config/prefs.json").read_text()).get("ml-ui", {}).get("theme")
            check("the theme follows the setting and is kept", theme == "night" and saved == "night", (theme, saved))
            await pg.screenshot(path=str(out / "02-night.png"))
            await pg.select_option("#theme-select", "system")

            # 6. a library made by a newer app opens read-only
            future = home / "Future"
            (future / "_library").mkdir(parents=True)
            (future / "_library/library.json").write_text(json.dumps({"format": 99, "id": "x", "name": "Future"}))
            pick(future)
            await pg.click("#open-library")
            await pg.wait_for_function("() => document.querySelector('#library-path')?.textContent.endsWith('Future')")
            disabled = await pg.is_disabled("#library-name")
            await pg.goto(B + "#/")
            await pg.wait_for_selector(".home .warn-note")
            note = await pg.inner_text(".home .warn-note")
            check("a newer library opens read-only", disabled and "read-only" in note and not (future / "Unsorted").exists(), note)
            await pg.screenshot(path=str(out / "03-read-only.png"))

            # 7. narrow window: the menu becomes a drawer
            await pg.set_viewport_size({"width": 700, "height": 800})
            await pg.wait_for_timeout(300)  # the drawer slides
            hidden = await pg.evaluate("() => document.querySelector('.sidebar').getBoundingClientRect().right <= 0")
            await pg.click(".nav-burger")
            await pg.wait_for_selector(".sidebar.open")
            await pg.wait_for_timeout(300)
            shown = await pg.evaluate("() => document.querySelector('.sidebar').getBoundingClientRect().left >= 0")
            check("the menu is a drawer in a narrow window", hidden and shown, (hidden, shown))
            await pg.screenshot(path=str(out / "04-narrow.png"))
            await b.close()
    finally:
        server.terminate()
        server.wait(timeout=20)

    # 8. portable: the library opens the same after moving it
    moved = home / "moved" / "My Models"
    shutil.copytree(library, moved)
    i1, i2 = library_info(library), library_info(moved)
    same = {k: i1[k] for k in ("id", "name", "format")} == {k: i2[k] for k in ("id", "name", "format")}
    check("library opens the same after moving", same, (i1["name"], i2["name"]))
    (out / "library-info.json").write_text(json.dumps({k: i1[k] for k in ("id", "name", "format")}, indent=1))
    shutil.make_archive(str(out / "library"), "zip", library)

    for e in errors:
        print("PAGE ERROR:", e)
    failed = [n for n, ok in results if not ok]
    print(f"{len(results) - len(failed)} of {len(results)} checks passed")
    return 1 if failed or errors else 0


sys.exit(asyncio.run(main()))
