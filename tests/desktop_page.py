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
Phase 2: sorting a messy folder through the Import page (proposals, warnings,
setting categories for several rows, move with a checked copy), adding a model
folder by copy, Move to category for one and several models, and sorting a loose
folder found inside the library.
Phase 3: a preview drawn on import, variant folder names set in Settings, a model's page with its 3D view, parts as a
tree and a Presupported/Unsupported switch, a ZIP's entries shown from inside
it, a readme rendered safely, pictures and "Use as cover", ranged reads of
library files (video seeking), and Make previews from Home.
Phase 4: renaming a faction with a preview, undoing it from Home, merging two
factions, editing a category (a level's label; a new top folder), and editing
several models' details at once.
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
    await pg.wait_for_selector(".browse-title h1:has-text('All models')")
    await count(pg)
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


def put(path, body="solid x"):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(body if isinstance(body, bytes) else body.encode())


async def import_and_wait(pg):
    await pg.wait_for_function("() => { const b = document.querySelector('#import-go'); return b && !b.disabled; }")
    await pg.click("#import-go")
    await pg.wait_for_selector("#import-results", timeout=60000)
    return await pg.inner_text("#import-results h2")


async def phase2(pg):
    # 15. sorting a messy folder of downloads
    dl = home / "Downloads"
    put(dl / "Hive Guard (Jo Smith)/guard.stl")
    put(dl / "Armour Set/Helmet/helmet.stl")
    put(dl / "Armour Set/Arms/arm.stl")
    put(dl / "dragon.zip", "PK dragon")
    put(dl / "benchy.stl")
    put(dl / "benchy.png", PNG)
    put(dl / "Copy of Captain/captain.zip", "PK")
    put(dl / "Other Library Model/prime.stl")
    put(dl / "Other Library Model/model.json", json.dumps({"id": "mfromelsewhere", "name": "Tyrant Prime", "schema": "wargames", "category": {"game": "Warhammer 40k", "faction": "Tyranid"}, "kept": True}))
    put(dl / "readme.txt", "hello")
    await pg.click('.sidebar a[href="#/import"]')
    pick(dl)
    await pg.click("#import-sort")
    await pg.wait_for_function("() => document.querySelectorAll('#import-list .imp-row').length === 6")
    names = await pg.eval_on_selector_all("#import-list .imp-name", "els => els.map(e => e.value)")
    row = lambda folder: pg.locator(f'#import-list .imp-row[data-source$="{folder}"]')
    several = await row("Armour Set").locator("[data-warn=several]").count()
    dup = await row("Copy of Captain").locator("[data-warn=duplicate]").count()
    prime = await row("Other Library Model").locator(".cat-value").evaluate_all("els => els.map(e => e.value)")
    left = await pg.inner_text(".imp-left summary")
    check("a messy folder is proposed as models, with warnings and guesses", names == ["Armour Set", "Copy of Captain", "Hive Guard", "Tyrant Prime", "benchy", "dragon"]
          and several == 1 and dup == 1 and prime == ["Warhammer 40k", "Tyranid"] and "1 loose file" in left, (names, several, dup, prime, left))
    await row("Hive Guard (Jo Smith)").locator(".imp-pick").check()
    await row("Armour Set").locator(".imp-pick").check()
    await pg.select_option("#batch-schema", "wargames")
    await pg.fill("#batch-game", "Warhammer 40k")
    await pg.fill("#batch-faction", "Tyranid")
    await pg.click("#batch-apply")
    await row("Copy of Captain").locator(".imp-skip").click()
    await pg.wait_for_function("() => [...document.querySelectorAll('#import-list .imp-row:not(.skipped)')].every(r => r.querySelector('.imp-dest code'))")
    dests = await pg.eval_on_selector_all("#import-list .imp-row:not(.skipped) .imp-dest code", "els => els.map(e => e.textContent)")
    await pg.screenshot(path=str(out / "09-import.png"), full_page=True)
    await pg.evaluate("() => { window.__forceCopy = true; }")  # as across drives: copy, check, delete
    title = await import_and_wait(pg)
    t = "Wargames/Warhammer 40k/Tyranid"
    on_disk = [(library / t / "Hive Guard (Jo Smith)/guard.stl").is_file(), (library / t / "Armour Set/Helmet/helmet.stl").is_file(),
               (library / "Unsorted/benchy/benchy.png").is_file(), (library / "Unsorted/dragon/dragon.zip").is_file(), (library / t / "Tyrant Prime/prime.stl").is_file()]
    side = json.loads((library / t / "Hive Guard (Jo Smith)/model.json").read_text())
    prime_side = json.loads((library / t / "Tyrant Prime/model.json").read_text())
    remaining = sorted(x.name for x in dl.iterdir())
    check("importing moves models into their category folders", title.startswith("5 models moved") and all(on_disk) and side["schema"] == "wargames"
          and side["category"] == {"game": "Warhammer 40k", "faction": "Tyranid"} and side["authors"] == [{"name": "Jo Smith"}] and prime_side.get("kept") is True
          and remaining == ["Copy of Captain", "readme.txt"] and dests[0] == f"{t}/Armour Set", (title, on_disk, remaining, dests, side))
    await pg.screenshot(path=str(out / "10-imported.png"), full_page=True)

    # 16. adding one model folder by copy
    await pg.click("#import-clear")
    put(home / "Elsewhere/Gargoyle/gargoyle.stl")
    pick(home / "Elsewhere/Gargoyle")
    await pg.click("#import-add-folder")
    await pg.wait_for_selector("#import-list .imp-row")
    await pg.click("#mode-copy")
    title = await import_and_wait(pg)
    check("a model folder is copied in, the original kept", title.startswith("1 model copied") and (home / "Elsewhere/Gargoyle/gargoyle.stl").is_file()
          and (library / "Unsorted/Gargoyle/gargoyle.stl").is_file(), title)

    # 17. Move to category: one model, then several
    await pg.goto(B + "#/browse/unsorted")
    await pg.wait_for_selector(".card:has(.card-name:text-is('benchy'))")
    await pg.click(".card:has(.card-name:text-is('benchy'))")
    await pg.click("#details-move")
    await pg.select_option("#move-schema", "wargames")
    await pg.fill("#move-game", "Warhammer 40k")
    await pg.fill("#move-faction", "Necrons")
    await pg.click("#move-dialog button[type=submit]")
    await pg.wait_for_selector("#move-dialog", state="detached")
    one = (library / "Wargames/Warhammer 40k/Necrons/benchy/benchy.stl").is_file() and not (library / "Unsorted/benchy").exists()
    await pg.goto(B + "#/browse/unsorted")
    await pg.wait_for_selector(".card:has-text('Gargoyle')")
    await pg.click(".card:has-text('dragon')")
    await pg.click(".card:has-text('Gargoyle')", modifiers=["Control"])
    await pg.wait_for_selector("#picked-panel")
    await pg.click("#move-picked")
    await pg.select_option("#move-schema", "wargames")
    await pg.fill("#move-game", "Warhammer 40k")
    await pg.fill("#move-faction", "Necrons")
    await pg.click("#move-dialog button[type=submit]")
    await pg.wait_for_selector("#move-dialog", state="detached")
    nec = sorted(x.name for x in (library / "Wargames/Warhammer 40k/Necrons").iterdir())
    n = await count(pg)
    check("models move to a category, one or several at once", one and nec == ["Gargoyle", "benchy", "dragon"] and n == "1 model", (one, nec, n))

    # 18. a loose folder inside the library is offered for sorting
    put(library / "Old stuff/Lictor/lictor.stl")
    await pg.goto(B + "#/")
    await pg.click("#rescan")
    await pg.wait_for_selector("#home-loose .sort-loose")
    await pg.click("#home-loose .sort-loose")
    await pg.wait_for_function("() => document.querySelectorAll('#import-list .imp-row').length === 1")
    await pg.select_option("#imp-0-schema", "wargames")
    await pg.fill("#imp-0-game", "Warhammer 40k")
    await pg.fill("#imp-0-faction", "Tyranid")
    title = await import_and_wait(pg)
    await pg.goto(B + "#/")
    await pg.wait_for_selector("#home-counts")
    await pg.wait_for_timeout(300)
    loose_gone = await pg.locator("#home-loose").count() == 0
    counts = await pg.inner_text("#home-counts")
    check("a loose folder in the library is sorted from Home", (library / t / "Lictor/lictor.stl").is_file() and loose_gone and "11 models" in counts, (title, loose_gone, counts))
    await pg.screenshot(path=str(out / "11-home.png"))


def cube(size=10.0):
    """A closed ASCII STL cube."""
    v = [(x, y, z) for x in (0, size) for y in (0, size) for z in (0, size)]
    faces = [(0, 2, 3, 1), (4, 5, 7, 6), (0, 1, 5, 4), (2, 6, 7, 3), (0, 4, 6, 2), (1, 3, 7, 5)]
    out = ["solid cube"]
    for a_, b_, c_, d_ in faces:
        for t in ((a_, b_, c_), (a_, c_, d_)):
            out += ["facet normal 0 0 0", "outer loop", *(f"vertex {v[i][0]} {v[i][1]} {v[i][2]}" for i in t), "endloop", "endfacet"]
    return "\n".join(out + ["endsolid cube", ""])


async def phase3(pg):
    import zipfile
    # 19. a model with parts, variants, a ZIP, a readme and a picture: previewed on import
    src = home / "Elsewhere/Knight Armour"
    put(src / "Presupported/Helmet/helmet.stl", cube(20))
    put(src / "Presupported/Arms/arm.stl", cube(8))
    put(src / "Unsupported/Helmet/helmet.stl", cube(20))
    put(src / "README.md", "# Knight Armour\n\nPrint at **0.12 mm**.\n\n<script>window.__pwned = 1</script>\n\n[site](https://example.com) [bad](javascript:alert(1))\n")
    put(src / "photo.png", PNG)
    with zipfile.ZipFile(src / "extras.zip", "w") as z:
        z.writestr("Extras/shield.stl", cube(15))
        z.writestr("__MACOSX/Extras/._shield.stl", "x")
    await pg.goto(B + "#/import")
    await pg.click("#import-clear") if await pg.locator("#import-clear").count() else None
    pick(src)
    await pg.click("#import-add-folder")
    await pg.wait_for_selector("#import-list .imp-row")
    await import_and_wait(pg)
    dest = library / "Unsorted/Knight Armour"
    thumb = (dest / "_thumbs/model.png").is_file()
    await pg.goto(B + "#/browse/unsorted")
    card = pg.locator(".card:has(.card-name:text-is('Knight Armour'))")
    await card.wait_for()
    cover = await card.locator("img").get_attribute("src") or ""
    check("a preview is drawn on import (a picture of the model is still its cover)", thumb and cover.endswith("/photo.png"), (thumb, cover))

    # 20. the model's page: 3D view, part tree, variants
    await card.dblclick()
    await pg.wait_for_selector("#model-page")
    await pg.wait_for_selector("#viewer-size, .stage-3d .form-error", timeout=30000)
    size_text = await pg.inner_text("#viewer-size") if await pg.locator("#viewer-size").count() else await pg.inner_text(".stage-3d .form-error")
    shown = await pg.inner_text("#viewer-file")
    variants = await pg.eval_on_selector_all("#variants button", "els => els.map(e => e.textContent + (e.getAttribute('aria-pressed') === 'true' ? '*' : ''))")
    dirs = await pg.eval_on_selector_all("#part-tree [data-dir]", "els => els.map(e => e.dataset.dir)")
    await pg.screenshot(path=str(out / "12-model-page.png"))
    check("a model opens on its own page with a 3D view, parts and variants", "20.0 × 20.0 × 20.0 mm" in size_text and shown == "Presupported/Helmet/helmet.stl"
          and variants == ["Presupported*", "Unsupported", "All"] and dirs == ["Presupported", "Presupported/Arms", "Presupported/Helmet"], (size_text, shown, variants, dirs))
    await pg.click("#part-tree [data-file='Presupported/Arms/arm.stl']")
    await pg.wait_for_function("() => document.querySelector('#viewer-size')?.textContent.startsWith('8.0')")
    await pg.click("#variants button:text-is('Unsupported')")
    await pg.wait_for_function("() => document.querySelector('#viewer-file')?.textContent === 'Unsupported/Helmet/helmet.stl'")
    dirs = await pg.eval_on_selector_all("#part-tree [data-dir]", "els => els.map(e => e.dataset.dir)")
    check("the variant switch shows that variant's parts", dirs == ["Unsupported", "Unsupported/Helmet"], dirs)

    # 20b. variant names are set in Settings: Resin and FDM folders become a switch
    put(dest / "FDM/fdm.stl", cube(6))
    put(dest / "Resin 32mm/resin.stl", cube(7))
    await pg.goto(B + "#/settings")
    await pg.wait_for_selector("#variant-names li")
    defaults = await pg.eval_on_selector_all("#variant-names li span", "els => els.map(e => e.textContent)")
    await pg.click("#variant-names li:has(span:text-is('FDM')) .chip-x")
    await pg.wait_for_function("() => ![...document.querySelectorAll('#variant-names li span')].some(e => e.textContent === 'FDM')")
    await pg.fill("#variant-add", "fdm")
    await pg.click("#variant-add-btn")
    await pg.wait_for_function("() => [...document.querySelectorAll('#variant-names li span')].some(e => e.textContent === 'fdm')")
    stored = json.loads((library / "_library/library.json").read_text()).get("variant_folders")
    await pg.go_back()
    await pg.wait_for_selector("#variants")
    await pg.wait_for_function("() => document.querySelectorAll('#variants button').length === 5")
    variants = await pg.eval_on_selector_all("#variants button", "els => els.map(e => e.textContent)")
    check("variant folder names are set in Settings and kept in the library", defaults == ["Presupported", "Supported", "Unsupported", "No supports", "Sized", "Split", "FDM", "Resin"]
          and stored and stored[-1] == "fdm" and "FDM" not in stored and variants == ["Presupported", "FDM", "Resin 32mm", "Unsupported", "All"], (defaults, stored, variants))
    await pg.click("#variants button:text-is('Unsupported')")

    # 21. a ZIP's entries open from inside it
    await pg.click("#part-tree [data-file='extras.zip']")
    await pg.wait_for_selector("#part-tree [data-entry]")
    entries = await pg.eval_on_selector_all("#part-tree [data-entry]", "els => els.map(e => e.dataset.entry)")
    await pg.click("#part-tree [data-entry='Extras/shield.stl']")
    await pg.wait_for_function("() => document.querySelector('#viewer-file')?.textContent === 'extras.zip › Extras/shield.stl' && document.querySelector('#viewer-size')?.textContent.startsWith('15.0')")
    check("a ZIP's parts are listed and shown without unzipping", entries == ["Extras/shield.stl"] and (dest / "extras.zip").is_file(), entries)

    # 22. "Use as cover" from the 3D view, the readme, the picture
    await pg.click("#view-cover")
    await pg.wait_for_function("() => document.querySelector('.toast')?.textContent.includes('cover')")
    side = json.loads((dest / "model.json").read_text())
    snap = (dest / "_media/cover.png").read_bytes()[:4] == b"\x89PNG" if (dest / "_media/cover.png").exists() else False
    await pg.click("[data-tab=docs]")
    await pg.wait_for_selector("#doc-text h1")
    doc = await pg.inner_html("#doc-text")
    pwned = await pg.evaluate("() => window.__pwned || 0")
    await pg.screenshot(path=str(out / "13-readme.png"))
    await pg.click("[data-tab=pictures]")
    await pg.wait_for_selector("#picture-big")
    await pg.click(".strip-btn[data-file='photo.png']")
    await pg.click("#picture-cover")
    await pg.wait_for_function("() => document.querySelector('#picture-cover')?.disabled")
    side2 = json.loads((dest / "model.json").read_text())
    check("a view or a picture becomes the cover; readmes show without scripts", snap and side["cover"] == "_media/cover.png" and side2["cover"] == "photo.png"
          and "<strong>0.12 mm</strong>" in doc and "<script" not in doc and 'href="javascript' not in doc and not pwned, (side.get("cover"), side2.get("cover"), doc[:200], pwned))

    # 23. library files can be read in ranges (videos seek)
    req = urllib.request.Request(B + "library/Unsorted/Knight%20Armour/README.md", headers={"Range": "bytes=2-7"})
    with urllib.request.urlopen(req, timeout=5) as r:
        code, body, cr = r.status, r.read(), r.headers.get("content-range")
    check("library files are served in ranges", code == 206 and body == b"Knight" and cr and cr.startswith("bytes 2-7/"), (code, body, cr))

    # 24. Make previews, for models added by hand
    put(library / "Unsorted/Hand Made/hand.stl", cube(5))
    await pg.goto(B + "#/")
    await pg.click("#rescan")
    await pg.wait_for_timeout(500)
    await pg.click("#make-previews")
    await pg.wait_for_function("() => /preview/.test(document.querySelector('.toast')?.textContent || '')", timeout=60000)
    msg = await pg.inner_text(".toast")
    await pg.goto(B + "#/browse/unsorted")
    hand = pg.locator(".card:has(.card-name:text-is('Hand Made')) img")
    await hand.wait_for()
    src_ = await hand.get_attribute("src")
    check("Make previews draws the missing ones, shown on the card", (library / "Unsorted/Hand Made/_thumbs/model.png").is_file() and src_.endswith("_thumbs/model.png"), (msg, src_))
    await pg.screenshot(path=str(out / "14-previews.png"))


async def phase4(pg):
    t = library / "Wargames/Warhammer 40k"
    before = sorted(x.name for x in (t / "Tyranid").iterdir())
    # 25. renaming a faction moves its folders (with a preview first)
    await pg.goto(B + "#/browse/schema/wargames/Warhammer%2040k/Tyranid")
    await pg.click("#rename-node")
    await pg.fill("#rename-faction", "Tyranids")
    await pg.wait_for_selector("#change-preview[data-moving]")
    preview = await pg.inner_text("#change-preview")
    await pg.screenshot(path=str(out / "15-rename.png"))
    await pg.click("#rename-dialog button[type=submit]")
    await pg.wait_for_selector("#rename-dialog", state="detached", timeout=60000)
    await pg.wait_for_function("() => location.hash.includes('Tyranids')")
    n = await count(pg)
    after = sorted(x.name for x in (t / "Tyranids").iterdir()) if (t / "Tyranids").is_dir() else []
    side = json.loads((t / "Tyranids/Hive Guard (Jo Smith)/model.json").read_text())
    check("renaming a faction moves its folders, after a preview", after == before and not (t / "Tyranid").exists() and side["category"]["faction"] == "Tyranids"
          and f"{len(before)} model folders move" in preview and n == f"{len(before)} models", (preview, before, after, n))

    # 26. undo from Home
    await pg.goto(B + "#/")
    await pg.wait_for_selector("#undo-change")
    await pg.screenshot(path=str(out / "16-recent-changes.png"))
    await pg.click("#undo-change")
    await pg.wait_for_function("() => /Undone/.test(document.querySelector('.toast')?.textContent || '')", timeout=60000)
    side = json.loads((t / "Tyranid/Hive Guard (Jo Smith)/model.json").read_text())
    check("the rename is undone from Home", sorted(x.name for x in (t / "Tyranid").iterdir()) == before and not (t / "Tyranids").exists()
          and side["category"]["faction"] == "Tyranid", side.get("category"))

    # 27. merging: Necrons into Tyranid
    nec = sorted(x.name for x in (t / "Necrons").iterdir())
    await pg.goto(B + "#/browse/schema/wargames/Warhammer%2040k/Necrons")
    await pg.click("#rename-node")
    await pg.fill("#rename-faction", "Tyranid")
    await pg.wait_for_selector("#rename-merge")
    await pg.wait_for_selector("#change-preview[data-moving]")
    await pg.click("#rename-dialog button[type=submit]")
    await pg.wait_for_selector("#rename-dialog", state="detached", timeout=60000)
    merged = sorted(x.name for x in (t / "Tyranid").iterdir())
    check("a faction renamed to one that exists merges into it", merged == sorted(before + nec) and not (t / "Necrons").exists(), merged)

    # 28. editing the category: a level renamed (nothing moves), then a new top folder (everything moves)
    await pg.goto(B + "#/browse/schema/wargames")
    await pg.click("#edit-schema")
    await pg.fill(".es-level >> nth=1", "Army")
    await pg.wait_for_selector("#change-preview[data-moving='0']")
    await pg.click("#edit-schema-dialog button[type=submit]")
    await pg.wait_for_selector("#edit-schema-dialog", state="detached", timeout=60000)
    sch = json.loads((library / "_library/schemas/wargames.json").read_text())
    relabelled = [l["label"] for l in sch["levels"]] == ["Game", "Army"] and t.is_dir()
    await pg.click("#edit-schema")
    await pg.fill("#es-folder", "Tabletop")
    await pg.wait_for_function("() => +document.querySelector('#change-preview')?.dataset.moving > 0")
    await pg.screenshot(path=str(out / "17-edit-category.png"))
    await pg.click("#edit-schema-dialog button[type=submit]")
    await pg.wait_for_selector("#edit-schema-dialog", state="detached", timeout=60000)
    moved = (library / "Tabletop/Warhammer 40k/Tyranid/Hive Guard (Jo Smith)/guard.stl").is_file() and not (library / "Wargames").exists()
    check("editing a category relabels a level, and a new top folder moves its models", relabelled and moved, (sch["levels"], moved))

    # 29. several models' details at once
    await pg.goto(B + "#/browse/schema/wargames")
    await pg.wait_for_selector(".card:has(.card-name:text-is('Gargoyle'))")
    await pg.click(".card:has(.card-name:text-is('Gargoyle'))")
    await pg.click(".card:has(.card-name:text-is('dragon'))", modifiers=["Control"])
    await pg.click("#edit-picked")
    await pg.fill("#bulk-tags-add", "painted, display")
    await pg.fill("#bulk-license", "CC-BY")
    await pg.click("#bulk-dialog button[type=submit]")
    await pg.wait_for_selector("#bulk-dialog", state="detached")
    tags = [json.loads((library / f"Tabletop/Warhammer 40k/Tyranid/{m}/model.json").read_text()) for m in ("Gargoyle", "dragon")]
    check("several models' details are edited at once", all(x.get("tags") == ["painted", "display"] and x.get("license") == "CC-BY" for x in tags), tags)


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
            b = await p.chromium.launch(executable_path=a.chromium, args=["--enable-unsafe-swiftshader", "--use-angle=swiftshader"])
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
            check("the menu's places open", labels == ["Home", "Import", "All models", "Unsorted", "Favourites", "Settings"] and unsorted == "Unsorted" and active == "Unsorted", (labels, unsorted, active))

            await phase1(pg)
            await phase2(pg)
            await phase3(pg)
            await phase4(pg)

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
