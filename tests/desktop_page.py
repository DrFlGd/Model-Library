#!/usr/bin/env python3
"""Acceptance test: the desktop app's page against its real backend
(`modlib-cli serve`, the same command table as the Tauri app), in Chromium.

    python3 tests/desktop_page.py --cli desktop/target/release/modlib-cli --ui build/desktop/ui \\
        --home /tmp/home --out shots [--chromium <path>]

Checks (Phase 0): the first start asks where the library goes and makes it
there (its _library/ folder and Unsorted/), the menu's places, renaming the
library, opening a second library and switching back from the recent list, a
folder that already holds models is left as it was, the theme follows the
setting, a library from a newer app opens read-only, and the library opens the
same after being moved (library-info).
Phase 1: making a schema, hand-made model folders listed under their categories,
search with typed filters, a model's details and files, editing details into
model.json, starring, and 10,000 generated models read and searched in time.
Writes <out>/library-info.json and <out>/library.zip for the cross-platform check.
"""
import argparse
import asyncio
import base64
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
ap.add_argument("--big", type=int, default=10000, help="models in the generated library for the timing check (0: skip)")
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
    p = subprocess.Popen([a.cli, "serve", "--ui", a.ui, "--home", str(home), "--port", str(a.port)], stdout=log, stderr=log)
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


PNG = base64.b64decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==")


def model_folder(rel, files):
    d = library / rel
    for name, body in files.items():
        (d / name).parent.mkdir(parents=True, exist_ok=True)
        (d / name).write_bytes(body if isinstance(body, bytes) else body.encode())
    return d


async def count(pg):
    await pg.wait_for_function("() => /\\d/.test(document.querySelector('#browse-count')?.textContent || '')")
    await pg.wait_for_timeout(250)  # the search waits for typing to pause
    return await pg.inner_text("#browse-count")


async def search(pg, q):
    """Search this place; the count once the results for `q` are shown."""
    await pg.fill("#search", q)
    await pg.wait_for_function("q => document.querySelector('#browse-count')?.dataset.q === q", arg=q)
    return await pg.inner_text("#browse-count")


async def phase1(pg):
    # 9. a new schema, from the menu
    await pg.click("#new-schema")
    await pg.fill("#schema-name", "Wargames")
    levels = pg.locator(".schema-level")
    await levels.nth(0).fill("Game")
    await levels.nth(1).fill("Faction")
    await pg.click("#add-field")
    await pg.locator(".schema-field").nth(0).fill("Scale")
    await pg.locator(".schema-field-type").nth(0).select_option("choice")
    await pg.locator(".schema-field-choices").nth(0).fill("28mm, 32mm")
    preview = await pg.inner_text("#schema-preview")
    head = await pg.inner_text(".sidebar .nav-head")
    dtitle = await pg.inner_text("#schema-dialog h2")
    await pg.screenshot(path=str(out / "05-new-schema.png"))
    await pg.click("#schema-dialog button[type=submit]")
    await pg.wait_for_selector("#schema-dialog", state="detached")
    await pg.wait_for_function("() => location.hash === '#/browse/schema/wargames'")
    schema = json.loads((library / "_library/schemas/wargames.json").read_text())
    check("a category (schema) is made from the menu", (library / "Wargames").is_dir() and [l["label"] for l in schema["levels"]] == ["Game", "Faction"]
          and schema["fields"][0]["choices"] == ["28mm", "32mm"] and preview == "Wargames / <Game> / <Faction> / Hive Tyrant (Jo Smith)"
          and head.lower() == "categories" and dtitle == "New category", (preview, head, dtitle, schema))

    # 10. model folders made by hand are found under their categories
    model_folder("Wargames/Warhammer 40k/Tyranid/Hive Tyrant (Jo Smith)", {"body.stl": "solid x", "Arms/left.stl": "solid x", "Arms/right.stl": "solid x", "_media/cover.png": PNG, "readme.pdf": "%PDF"})
    model_folder("Wargames/Warhammer 40k/Tyranid/Carnifex (Al Jones)", {"carnifex.3mf": "PK"})
    model_folder("Wargames/Warhammer 40k/Space Marines/Captain (Jo Smith)", {"captain.zip": "PK"})
    model_folder("Unsorted/Benchy", {"benchy.stl": "solid x"})
    await pg.goto(B + "#/")
    await pg.click("#rescan")
    await pg.wait_for_function("() => document.querySelector('#home-counts')?.textContent.includes('4 models')")
    await pg.click('.sidebar a[href="#/browse/schema/wargames/Warhammer%2040k"]')
    tree = await pg.eval_on_selector_all("#schema-tree .nav-link", "els => els.map(e => e.textContent.trim())")
    await pg.goto(B + "#/browse/schema/wargames/Warhammer%2040k/Tyranid")
    n = await count(pg)
    names = await pg.eval_on_selector_all(".card .card-name", "els => els.map(e => e.textContent)")
    crumbs = await pg.inner_text(".crumbs")
    check("model folders are listed under their categories", tree == ["Wargames3", "Warhammer 40k3"] and n == "2 models" and names == ["Carnifex", "Hive Tyrant"] and "Tyranid" in crumbs, (tree, n, names, crumbs))
    await pg.screenshot(path=str(out / "06-browse.png"))

    # 11. search, with typed filters
    await pg.goto(B + "#/browse/all")
    found = [await search(pg, "tyrant"), await search(pg, "author:\"jo smith\""), await search(pg, "schema:wargames carni"), await search(pg, "faction:space")]
    await search(pg, "nothing-like-this")
    none = await pg.is_visible("#no-results")
    check("search finds models by words and filters", found == ["1 model", "2 models", "1 model", "1 model"] and none, (found, none))
    await search(pg, "")

    # 12. a model's details and files
    await pg.click(".card:has-text('Hive Tyrant')")
    await pg.wait_for_selector("#details-files")
    files = await pg.eval_on_selector_all("#details-files li span:first-child", "els => els.map(e => e.textContent)")
    by = await pg.inner_text("#details-authors")
    cover = await pg.evaluate("() => { const i = document.querySelector('.card[aria-selected=true] img'); return !!i && i.complete && i.naturalWidth > 0; }")
    check("a model shows its details, parts and cover", files == ["Arms/left.stl", "Arms/right.stl", "_media/cover.png", "body.stl", "readme.pdf"] and by == "by Jo Smith" and cover, (files, by, cover))

    # 13. editing details writes model.json (the folder keeps its name)
    await pg.click("#details-edit")
    await pg.fill("#edit-tags", "monster, big")
    await pg.fill("#edit-source", "https://example.com/tyrant")
    await pg.select_option("#field-scale", "32mm")
    await pg.screenshot(path=str(out / "07-edit.png"))
    await pg.click("#details-dialog button[type=submit]")
    await pg.wait_for_selector("#details-dialog", state="detached")
    await pg.wait_for_selector("#details-tags")
    d = library / "Wargames/Warhammer 40k/Tyranid/Hive Tyrant (Jo Smith)"
    side = json.loads((d / "model.json").read_text())
    scale = await pg.inner_text('#details-list dd[data-field="scale"]')
    check("details are saved to model.json", side.get("tags") == ["monster", "big"] and side.get("fields") == {"scale": "32mm"} and side.get("authors") == [{"name": "Jo Smith"}]
          and side.get("source") == {"url": "https://example.com/tyrant"} and side.get("id", "").startswith("m") and scale == "32mm", side)
    found = [await search(pg, "scale:32mm"), await search(pg, "tag:monster")]
    await search(pg, "")

    # 14. starring
    await pg.click(".card:has-text('Carnifex')")
    await pg.wait_for_selector("#details-name:has-text('Carnifex')")
    await pg.click("#details-star")
    await pg.wait_for_selector('#details-star[aria-pressed="true"]')
    favs = json.loads((library / "_library/library.json").read_text()).get("favourites", [])
    carni = json.loads((library / "Wargames/Warhammer 40k/Tyranid/Carnifex (Al Jones)/model.json").read_text())
    await pg.click('.sidebar a[href="#/browse/favs"]')
    starred = await count(pg)
    check("filters find edited details; a star lasts", found == ["1 model", "1 model"] and favs == [carni["id"]] and starred == "1 model", (found, favs, starred))
    await pg.screenshot(path=str(out / "08-favourites.png"))


def big_library():
    """10,000 generated models: read, read again from the cache, searched."""
    big = home / "Big"
    subprocess.run([a.cli, "make-test-library", "--out", str(big), "--models", str(a.big)], check=True)
    r = subprocess.run([a.cli, "library-scan", "--library", str(big), "--cache", str(home / "big-index.json")], capture_output=True, text=True, check=True)
    t = json.loads(r.stdout)
    (out / "big-library.json").write_text(json.dumps(t, indent=1))
    check(f"{a.big} models open in seconds and search in under 100 ms", t["models"] == a.big and t["first_ms"] < 20000 and t["reopen_ms"] < 10000 and t["search_us"] < 100000, t)
    shutil.rmtree(big, ignore_errors=True)


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
            await pg.wait_for_selector("#first-run", timeout=30000)

            # 1. the first start asks where the library goes, then makes it there
            asked = await pg.inner_text("#first-run h1")
            menu_off = await pg.is_visible("#new-schema")
            await pg.screenshot(path=str(out / "00-first-run.png"))
            pick(library)
            await pg.click("#choose-library")
            await pg.wait_for_selector(".home h1:not(:has-text('Where'))")
            await pg.wait_for_function("() => document.querySelector('.home h1').textContent === 'My Models'")
            title = await pg.inner_text(".home h1")
            made = (library / "_library/library.json").is_file() and (library / "Unsorted").is_dir() and (library / "_library/schemas").is_dir()
            status = await pg.inner_text(".statusbar")
            check("the first start asks where the library goes", "Where" in asked and not menu_off and title == "My Models" and made and str(library) in status, (asked, title, made, status))
            await pg.screenshot(path=str(out / "00-home.png"))

            # 2. the menu's places
            labels = await pg.eval_on_selector_all(".sidebar .nav-label", "els => els.map(e => e.textContent.trim())")
            await pg.click('.sidebar a[href="#/browse/unsorted"]')
            await pg.wait_for_selector("#no-results")
            unsorted = await pg.inner_text(".browse-title h1")
            active = await pg.inner_text(".sidebar .nav-link.active .nav-label")
            check("the menu's places open", labels == ["Home", "All models", "Unsorted", "Favourites", "Settings"] and unsorted == "Unsorted" and active == "Unsorted", (labels, unsorted, active))

            await phase1(pg)

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
    if a.big:
        big_library()

    for e in errors:
        print("PAGE ERROR:", e)
    failed = [n for n, ok in results if not ok]
    print(f"{len(results) - len(failed)} of {len(results)} checks passed")
    return 1 if failed or errors else 0


sys.exit(asyncio.run(main()))
