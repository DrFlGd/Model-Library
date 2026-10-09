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
sending several models to a category, move with a checked copy), adding a model
folder by copy, Move to category for one and several models, and sorting a loose
folder found inside the library.
Phase 3: a preview drawn on import, variant folder names set in Settings, a model's page with its 3D view, parts as a
tree and a Presupported/Unsupported switch, a ZIP's entries shown from inside
it, a readme rendered safely, pictures and "Use as cover", ranged reads of
library files (video seeking), and Make previews from Home.
Phase 4: renaming a faction with a preview, undoing it from Home, merging two
factions, editing a category (a level's label; a new top folder), and editing
several models' details at once.
Phase 5: the Import page as a sorting workspace (a messy folder read as it is,
models sent to categories, a folder made one model, a tidy share sent with its
folders kept as subcategories, loose files grouped, the list, grid and by-category
views), a model's files as folders, all files, by type and a grid, folders added
and moved outside the app showing up by themselves, and duplicates found, set
aside, undone and deleted.
Phase 1: making a schema, hand-made model folders listed under their categories,
search with typed filters, a model's details and files, editing details into
model.json, starring, and 10,000 generated models read and searched in time.
UI pass, step 1: the same action row and right-click menu, the keys (arrows,
Shift, Ctrl+A, Esc, E, M, S, Ctrl+Z), Undo in messages, the Category menu, a
selection that doesn't follow into another place, filter chips that come off,
the theme button's four choices, Starred's count, Import's menu, keys and undo,
messages clear of Import's footer, and files with no viewer greyed.
UI pass, step 2: Move to category's preview and Undo, Edit details and Import
undone, an older change undone from Home, Read again as a job.
UI pass, step 3: one page header on every page, one view switcher and sort menu,
the list's sorting headers, "Searching for …" in another place, the details
panel changed in place, Home's sections and Library menu, and the same panel on
Duplicates and Import.
Import follow-ups: file type tags on Import and in a model's files, making every
folder at a level a model (and Undo), choosing a folder above a model as the
model, and the menu for files dropped on the window (added to the category
shown, undone; sorted on Import).
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
from model_workspace_panel import panel_workspace_checks
from model_workspace_loose import loose_workspace_checks

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
    # 9. a new schema, from the menu, with a tree of subcategories of different depths
    await pg.click("#new-schema")
    await pg.fill("#schema-name", "Wargames")
    await pg.click("#subcat-tree-add")
    await pg.keyboard.type("Warhammer 40k")
    await pg.click('.st-row[data-path="Warhammer 40k"] .st-add')
    await pg.keyboard.type("Tyranid")
    await pg.keyboard.press("Enter")  # the next one beside it
    await pg.keyboard.type("Space Marines")
    await pg.click("#subcat-tree-add")
    await pg.keyboard.type("Terrain")
    await pg.click("#subcat-tree-add")  # left empty: dropped
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
    want = [{"name": "Terrain"}, {"name": "Warhammer 40k", "subcategories": [{"name": "Space Marines"}, {"name": "Tyranid"}]}]
    check("a category (schema) is made from the menu, with its subcategory tree", (library / "Wargames/Warhammer 40k/Space Marines").is_dir() and (library / "Wargames/Terrain").is_dir()
          and schema.get("subcategories") == want and "levels" not in schema
          and schema["fields"][0]["choices"] == ["28mm", "32mm"] and preview == "Wargames / Warhammer 40k / Tyranid / Model name (Author)"
          and head.lower() == "categories" and dtitle == "New category", (preview, head, dtitle, schema))

    # 10. model folders made by hand are found under their categories
    model_folder("Wargames/Warhammer 40k/Tyranid/Hive Tyrant (Jo Smith)", {"body.stl": "solid x", "Arms/left.stl": "solid x", "Arms/right.stl": "solid x", "_media/cover.png": PNG, "readme.pdf": "%PDF"})
    model_folder("Wargames/Warhammer 40k/Tyranid/Carnifex (Al Jones)", {"carnifex.3mf": "PK"})
    model_folder("Wargames/Warhammer 40k/Space Marines/Captain (Jo Smith)", {"captain.zip": "PK"})
    model_folder("Unsorted/Benchy", {"benchy.stl": "solid x"})
    await pg.goto(B + "#/")
    await library_menu(pg, "rescan")
    await pg.wait_for_function("() => document.querySelector('#home-counts')?.textContent.includes('4 models')")
    await pg.click('.sidebar a[href="#/browse/schema/wargames/Warhammer%2040k"]')
    tree = await pg.eval_on_selector_all("#schema-tree .nav-link", "els => els.map(e => e.textContent.trim())")
    await pg.goto(B + "#/browse/schema/wargames/Warhammer%2040k/Tyranid")
    n = await count(pg)
    names = await pg.eval_on_selector_all(".card .card-name", "els => els.map(e => e.textContent)")
    crumbs = await pg.inner_text(".crumbs")
    check("model folders are listed under their categories", tree == ["Wargames3", "Terrain0", "Warhammer 40k3"] and n == "2 models" and names == ["Carnifex", "Hive Tyrant"] and "Tyranid" in crumbs, (tree, n, names, crumbs))
    await pg.screenshot(path=str(out / "06-browse.png"))

    # 11. search, with typed filters
    await pg.goto(B + "#/browse/all")
    await pg.wait_for_selector(".page-head h1:has-text('All models')")
    await count(pg)
    found = [await search(pg, "tyrant"), await search(pg, "author:\"jo smith\""), await search(pg, "schema:wargames carni"), await search(pg, "in:\"space marines\"")]
    await search(pg, "nothing-like-this")
    none = await pg.is_visible("#no-results")
    check("search finds models by words and filters", found == ["1 model", "2 models", "1 model", "1 model"] and none, (found, none))
    await search(pg, "")

    # 12. a model's details and files
    await pg.click(".card:has-text('Hive Tyrant')")
    await pg.wait_for_selector("#details-files")
    files = await pg.eval_on_selector_all("#details-files .insp-file-name", "els => els.map(e => e.textContent)")
    by = await pg.input_value("#details-authors")
    cover = await pg.evaluate("() => { const i = document.querySelector('.card[aria-selected=true] img'); return !!i && i.complete && i.naturalWidth > 0; }")
    check("a model shows its details, parts and cover", files == ["Arms/left.stl", "Arms/right.stl", "_media/cover.png", "body.stl", "readme.pdf"] and by == "Jo Smith" and cover, (files, by, cover))

    # 13. editing details writes model.json (the folder keeps its name)
    await pg.click("#details-edit")
    await pg.fill("#edit-tags", "monster, big")
    await pg.fill("#edit-source", "https://example.com/tyrant")
    await pg.select_option("#field-scale", "32mm")
    await pg.screenshot(path=str(out / "07-edit.png"))
    await pg.click("#details-dialog button[type=submit]")
    await pg.wait_for_selector("#details-dialog", state="detached")
    await pg.wait_for_function("() => document.querySelector('#details-tags')?.value === 'monster, big'")
    d = library / "Wargames/Warhammer 40k/Tyranid/Hive Tyrant (Jo Smith)"
    side = json.loads((d / "model.json").read_text())
    scale = await pg.inner_text('#details-list dd[data-field="scale"]')
    check("details are saved to model.json", side.get("tags") == ["monster", "big"] and side.get("fields") == {"scale": "32mm"} and side.get("authors") == [{"name": "Jo Smith"}]
          and side.get("source") == {"url": "https://example.com/tyrant"} and side.get("id", "").startswith("m") and scale == "32mm", side)
    found = [await search(pg, "scale:32mm"), await search(pg, "tag:monster")]
    await search(pg, "")

    # 14. starring
    await pg.click(".card:has-text('Carnifex')")
    await pg.wait_for_function("() => document.querySelector('#details-name')?.value === 'Carnifex'")
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
    if await pg.locator("#import-results").count():  # the last import's results
        await pg.click("#import-results-close")
        await pg.wait_for_selector("#import-results", state="detached")
    await pg.wait_for_function("() => { const b = document.querySelector('#import-go'); return b && !b.disabled; }")
    await pg.click("#import-go")
    await pg.wait_for_selector("#import-results", timeout=60000)
    return await pg.inner_text("#import-results h2")


async def sort_rows(pg):
    """The workspace as a list: every model's name."""
    await pg.click('[data-view="list"]')
    await pg.click('[data-filter="all"]')
    await pg.wait_for_selector("#sort-list")
    return await pg.eval_on_selector_all("#sort-list tr[data-name]", "els => els.map(e => e.dataset.name)")


async def pick_rows(pg, *names):
    """Pick these models in the list view (Ctrl-click for the rest)."""
    for i, n in enumerate(names):
        await pg.click(f'#sort-list tr[data-name="{n}"] .sw-name', modifiers=["Control"] if i else [])
    await pg.wait_for_selector("#sort-actions")


async def send_to(pg, schema="", place=None, new=None):
    """Send what's picked: a category ("" is Unsorted), a subcategory, a new one inside."""
    await pg.select_option("#send-schema", schema)
    if place is not None:
        await pg.select_option("#send-place", place)
    if new:
        await pg.fill("#send-new", new)
    await clear_toast(pg)
    await pg.click("#send-go")
    await pg.wait_for_function("() => /to go to/.test(document.querySelector('.toast span')?.textContent || '')")


async def clear_toast(pg):
    await pg.evaluate("() => { const t = document.querySelector('.toast'); if (t) { t.textContent = ''; t.hidden = true; } }")


async def menu_item(pg, button, action):
    """Open a page's menu (More ▾, Library ▾…) and pick one of its items."""
    await pg.click(button)
    await pg.click(f'#context-menu [data-action="{action}"]')


async def library_menu(pg, action):
    await menu_item(pg, "#library-menu", action)


async def start_again(pg):
    """Empty Import's workspace (Start again, under More ▾) if anything is in it."""
    await pg.wait_for_selector("#sort-page[data-ready]")
    await pg.click("#import-more")
    item = pg.locator('#context-menu [data-action="import-clear"]')
    if await item.count():
        await item.click()
        await pg.wait_for_selector("#sort-page .sw-empty")
    else:
        await pg.keyboard.press("Escape")


async def toast_text(pg, pattern, timeout=30000):
    """Wait for a message matching `pattern` (a JS regex source); its text."""
    await pg.wait_for_function("p => new RegExp(p).test(document.querySelector('.toast span')?.textContent || '')", arg=pattern, timeout=timeout)
    return await pg.inner_text(".toast span")


async def category_menu(pg, action):
    """Choose from the Category menu of the category page shown."""
    await pg.click("#category-menu")
    await pg.click(f'#context-menu [data-action="{action}"]')


async def phase2(pg):
    # 15. sorting a messy folder of downloads in the workspace
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
    await pg.wait_for_selector('#sort-tree .sw-folder[data-name="Armour Set"]', timeout=30000)
    loose = await pg.locator('#sort-tree .sw-left[data-name="readme.txt"]').count()
    await pg.screenshot(path=str(out / "09-import.png"), full_page=True)
    # a folder of parts read as two models becomes one
    await pg.click('#sort-tree .sw-folder[data-name="Armour Set"] .sw-name')
    await pg.click("#sort-join")
    await pg.wait_for_selector('#sort-tree .sw-item[data-name="Armour Set"]')
    names = sorted(await sort_rows(pg))
    await pick_rows(pg, "Copy of Captain")
    dup = await pg.locator("#sort-details [data-warn=duplicate]").count()
    guess = await pg.inner_text('#sort-list tr[data-name="Tyrant Prime"]')
    check("a messy folder is read as it is, with models proposed, warnings, and a model.json's place used", names == ["Armour Set", "Copy of Captain", "Hive Guard", "Tyrant Prime", "benchy", "dragon"]
          and loose == 1 and dup == 1 and "→ Wargames › Warhammer 40k › Tyranid" in guess, (names, loose, dup, guess))
    await pg.click("#sort-skip")
    await pick_rows(pg, "Hive Guard", "Armour Set")
    await send_to(pg, "wargames", "Warhammer 40k\x1fTyranid")
    # (Tyrant Prime's model.json says where it goes: it's sorted already)
    await pick_rows(pg, "benchy", "dragon")
    await send_to(pg, "")
    await pg.wait_for_function("() => /Import 5 models/.test(document.querySelector('#import-go')?.textContent || '')")
    await pg.evaluate("() => { window.__forceCopy = true; }")  # as across drives: copy, check, delete
    title = await import_and_wait(pg)
    t = "Wargames/Warhammer 40k/Tyranid"
    on_disk = [(library / t / "Hive Guard (Jo Smith)/guard.stl").is_file(), (library / t / "Armour Set/Helmet/helmet.stl").is_file(),
               (library / "Unsorted/benchy/benchy.png").is_file(), (library / "Unsorted/dragon/dragon.zip").is_file(), (library / t / "Tyrant Prime/prime.stl").is_file()]
    side = json.loads((library / t / "Hive Guard (Jo Smith)/model.json").read_text())
    prime_side = json.loads((library / t / "Tyrant Prime/model.json").read_text())
    remaining = sorted(x.name for x in dl.iterdir())
    check("importing moves models into their category folders", title.startswith("5 models moved") and all(on_disk) and side["schema"] == "wargames"
          and side["path"] == ["Warhammer 40k", "Tyranid"] and "category" not in prime_side and side["authors"] == [{"name": "Jo Smith"}] and prime_side.get("kept") is True
          and remaining == ["Copy of Captain", "readme.txt"], (title, on_disk, remaining, side))
    await pg.screenshot(path=str(out / "10-imported.png"), full_page=True)

    # 16. adding one model folder by copy, after starting the workspace again
    await start_again(pg)  # can be undone, so it doesn't ask
    put(home / "Elsewhere/Gargoyle/gargoyle.stl")
    pick(home / "Elsewhere/Gargoyle")
    await menu_item(pg, "#import-more", "import-add-folder")
    await pg.wait_for_selector('#sort-list tr[data-name="Gargoyle"], #sort-tree .sw-item[data-name="Gargoyle"]')
    await sort_rows(pg)
    await pick_rows(pg, "Gargoyle")
    await send_to(pg, "")
    await pg.click("#mode-copy")
    title = await import_and_wait(pg)
    check("a model folder is copied in, the original kept", title.startswith("1 model copied") and (home / "Elsewhere/Gargoyle/gargoyle.stl").is_file()
          and (library / "Unsorted/Gargoyle/gargoyle.stl").is_file(), title)
    await pg.click("#mode-move")

    # 17. Move to category: one model, then several
    await pg.goto(B + "#/browse/unsorted")
    await pg.wait_for_selector(".card:has(.card-name:text-is('benchy'))")
    await pg.click(".card:has(.card-name:text-is('benchy'))")
    await pg.click("#details-move")
    await pg.select_option("#move-schema", "wargames")
    await pg.select_option("#move-place", "Warhammer 40k")
    await pg.fill("#move-new", "Necrons")  # a new subcategory, made as it moves
    await pg.click("#move-dialog button[type=submit]")
    await pg.wait_for_selector("#move-dialog", state="detached")
    one = (library / "Wargames/Warhammer 40k/Necrons/benchy/benchy.stl").is_file() and not (library / "Unsorted/benchy").exists()
    await pg.goto(B + "#/browse/unsorted")
    await pg.wait_for_selector(".card:has-text('Gargoyle')")
    await pg.click(".card:has-text('dragon')")
    await pg.click(".card:has-text('Gargoyle')", modifiers=["Control"])
    await pg.wait_for_selector("#picked-panel")
    await pg.click("#picked-move")
    await pg.select_option("#move-schema", "wargames")
    await pg.select_option("#move-place", "Warhammer 40k\x1fNecrons")
    await pg.click("#move-dialog button[type=submit]")
    await pg.wait_for_selector("#move-dialog", state="detached")
    nec = sorted(x.name for x in (library / "Wargames/Warhammer 40k/Necrons").iterdir())
    n = await count(pg)
    check("models move to a category, one or several at once", one and nec == ["Gargoyle", "benchy", "dragon"] and n == "1 model", (one, nec, n))

    # 18. a loose folder inside the library is offered for sorting
    put(library / "Old stuff/Lictor/lictor.stl")
    await pg.goto(B + "#/")
    await library_menu(pg, "rescan")
    await pg.wait_for_selector("#home-loose .sort-loose")
    await pg.click("#home-loose .sort-loose")
    await pg.wait_for_selector("#sort-page[data-ready]")
    await pg.wait_for_function("() => !document.querySelector('#import-progress')")
    await sort_rows(pg)
    await pick_rows(pg, "Lictor")
    await send_to(pg, "wargames", "Warhammer 40k\x1fTyranid")
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
    await pg.wait_for_selector("#sort-page[data-ready]")
    pick(src)
    await menu_item(pg, "#import-more", "import-add-folder")
    await pg.wait_for_function("() => !document.querySelector('#import-progress')")
    await sort_rows(pg)
    await pick_rows(pg, "Knight Armour")
    await send_to(pg, "")
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
    dirs = await pg.eval_on_selector_all("#part-tree [data-dir]", "els => els.map(e => e.dataset.dir).filter(Boolean)")
    await pg.screenshot(path=str(out / "12-model-page.png"))
    check("a model opens on its own page with a 3D view, parts and variants", "20.0 × 20.0 × 20.0 mm" in size_text and shown == "Presupported/Helmet/helmet.stl"
          and variants == ["Presupported*", "Unsupported", "All"] and dirs == ["Presupported", "Presupported/Arms", "Presupported/Helmet"], (size_text, shown, variants, dirs))
    await pg.click("#part-tree [data-file='Presupported/Arms/arm.stl']")
    await pg.wait_for_function("() => document.querySelector('#viewer-size')?.textContent.startsWith('8.0')")
    await pg.click("#variants button:text-is('Unsupported')")
    await pg.wait_for_function("() => document.querySelector('#viewer-file')?.textContent === 'Unsupported/Helmet/helmet.stl'")
    dirs = await pg.eval_on_selector_all("#part-tree [data-dir]", "els => els.map(e => e.dataset.dir).filter(Boolean)")
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
    await pg.click("#part-tree [aria-label='Unfold extras.zip']")
    await pg.wait_for_selector("#part-tree [data-entry]")
    entries = await pg.eval_on_selector_all("#part-tree [data-entry]", "els => els.map(e => e.dataset.entry).filter(e => !e.endsWith('/'))")
    await pg.click("#part-tree [data-entry='Extras/shield.stl']")
    await pg.wait_for_function("() => document.querySelector('#viewer-file')?.textContent === 'extras.zip › Extras/shield.stl' && document.querySelector('#viewer-size')?.textContent.startsWith('15.0')")
    check("a ZIP's parts are listed and shown without unzipping", entries == ["Extras/shield.stl"] and (dest / "extras.zip").is_file(), entries)

    # 22. "Use as cover" from the 3D view, the readme, the picture
    await pg.click("#view-cover")
    await pg.wait_for_function("() => document.querySelector('.toast')?.textContent.includes('cover')")
    side = json.loads((dest / "model.json").read_text())
    snap = (dest / "_media/cover.png").read_bytes()[:4] == b"\x89PNG" if (dest / "_media/cover.png").exists() else False
    await pg.click("#part-tree [data-file='README.md']")
    await pg.wait_for_selector("#doc-text h1")
    doc = await pg.inner_html("#doc-text")
    pwned = await pg.evaluate("() => window.__pwned || 0")
    await pg.screenshot(path=str(out / "13-readme.png"))
    await pg.click("#part-tree [data-file='photo.png']")
    await pg.wait_for_selector("#picture-big")
    await pg.click("#picture-cover")
    await pg.wait_for_function("() => document.querySelector('#picture-cover')?.disabled")
    side2 = json.loads((dest / "model.json").read_text())
    check("a view or a picture becomes the cover; readmes show without scripts", snap and side["cover"] == "_media/cover.png" and side2["cover"] == "photo.png"
          and "<strong>0.12 mm</strong>" in doc and "<script" not in doc and 'href="javascript' not in doc and not pwned, (side.get("cover"), side2.get("cover"), doc[:200], pwned))

    # 22b. the model's files in other views: all files, by type, a grid of previews
    await pg.click("#variants button:text-is('All')")  # the variant switch picks the files every view shows
    on_disk = sorted(x.relative_to(dest).as_posix() for x in dest.rglob("*") if x.is_file() and x.name != "model.json" and not x.relative_to(dest).as_posix().startswith("_thumbs/"))
    await pg.click('#parts-views [data-view="all"]')
    await pg.wait_for_selector("#all-files")
    every = await pg.eval_on_selector_all("#all-files [data-file]", "els => els.map(e => e.dataset.file)")
    await pg.fill("#file-panel .parts-filter", "helmet")
    await pg.wait_for_function("() => document.querySelectorAll('#all-files [data-file]').length === 2")
    await pg.fill("#file-panel .parts-filter", "")
    await pg.click('#parts-views [data-view="type"]')
    kinds = await pg.eval_on_selector_all("#by-type [data-kind]", "els => els.map(e => e.dataset.kind)")
    await pg.click('#parts-views [data-view="folders"]')
    await pg.click('#part-tree [data-key="d:Presupported/Helmet"]')
    await pg.wait_for_selector("#contents-view .file-tile")
    await pg.wait_for_function("() => [...document.querySelectorAll('#contents-view img')].some(i => i.complete && i.naturalWidth > 0)", timeout=30000)
    tiles = await pg.locator("#contents-view .file-tile").count()
    await pg.screenshot(path=str(out / "13b-files-grid.png"))
    await pg.click('#parts-views [data-view="type"]')
    await pg.wait_for_timeout(600)  # preferences are saved after a short pause
    kept = json.loads((home / "config/prefs.json").read_text()).get("ml-ui", {}).get("filePanelView") if (home / "config/prefs.json").exists() else None
    await pg.click('#parts-views [data-view="folders"]')
    await pg.wait_for_selector("#part-tree")
    check("a model's files show as folders, a searchable list, by type and folder previews", sorted(every) == on_disk and kinds == ["model", "image", "doc", "archive"]
          and tiles == 1 and kept == "type", (every, on_disk, kinds, tiles, kept))

    await panel_workspace_checks(pg, check)

    # 23. library files can be read in ranges (videos seek)
    req = urllib.request.Request(B + "library/Unsorted/Knight%20Armour/README.md", headers={"Range": "bytes=2-7"})
    with urllib.request.urlopen(req, timeout=5) as r:
        code, body, cr = r.status, r.read(), r.headers.get("content-range")
    check("library files are served in ranges", code == 206 and body == b"Knight" and cr and cr.startswith("bytes 2-7/"), (code, body, cr))

    # 24. Make previews, for models added by hand
    put(library / "Unsorted/Hand Made/hand.stl", cube(5))
    await pg.goto(B + "#/")
    await library_menu(pg, "rescan")
    await pg.wait_for_timeout(500)
    await library_menu(pg, "make-previews")
    await pg.wait_for_function("() => /preview/.test(document.querySelector('.toast')?.textContent || '')", timeout=60000)
    msg = await pg.inner_text(".toast")
    await pg.goto(B + "#/browse/unsorted")
    hand = pg.locator(".card:has(.card-name:text-is('Hand Made')) img")
    await hand.wait_for()
    src_ = await hand.get_attribute("src")
    check("Make previews draws the missing ones, shown on the card", (library / "Unsorted/Hand Made/_thumbs/model.png").is_file() and src_.endswith("_thumbs/model.png"), (msg, src_))
    await pg.screenshot(path=str(out / "14-previews.png"))


async def workspace_viewing(pg):
    """Package B: selection-driven viewers and direct folder/archive contents."""
    await pg.goto(B + "#/browse/unsorted")
    await pg.locator(".card:has(.card-name:text-is('Knight Armour'))").dblclick()
    await pg.wait_for_selector("#workspace-stage")
    # Drive the public selection contract; verify actual rendered viewers/tiles.
    async def show_file(key):
        await pg.evaluate("async key => (await import('./ui/filesel.js')).pick(key)", key)
    await show_file("f:photo.png")
    await pg.wait_for_selector("#picture-big")
    await show_file("f:README.md")
    await pg.wait_for_selector("#doc-text h1")
    check("file selection opens the picture and safe readme viewers", not await pg.evaluate("window.__pwned || 0"))
    await show_file("d:")
    await pg.wait_for_selector("#contents-view .file-tile")
    keys = await pg.eval_on_selector_all("#contents-view .file-tile", "els => els.map(e => e.dataset.key)")
    first_file = next((i for i, k in enumerate(keys) if k.startswith("f:")), len(keys))
    check("folder contents put subfolders before direct files", all(k.startswith("d:") for k in keys[:first_file]) and all(k.startswith("f:") for k in keys[first_file:]), keys)
    await pg.locator('#contents-view [data-key="f:photo.png"]').click()
    await pg.locator('#contents-view [data-key="f:README.md"]').click(modifiers=["Control"])
    selected = await pg.evaluate("async () => (await import('./ui/filesel.js')).fileSel.get().picked")
    check("tiles share a multiple selection without leaving the folder", sorted(selected) == ["f:README.md", "f:photo.png"] and await pg.locator("#contents-view").count() == 1, selected)
    await pg.locator('#contents-view [data-key="f:extras.zip"]').dblclick()
    await pg.locator('#contents-view [data-key="z:extras.zip!Extras/"]').dblclick()
    await pg.locator('#contents-view [data-key="z:extras.zip!Extras/shield.stl"]').dblclick()
    await pg.wait_for_function("() => document.querySelector('#viewer-file')?.textContent === 'extras.zip › Extras/shield.stl'")
    await pg.locator("#workspace-stage").click(position={"x": 8, "y": 8})
    await pg.keyboard.press("Backspace")
    await pg.wait_for_selector('#contents-view [data-key="z:extras.zip!Extras/shield.stl"]')
    check("ZIP folders open their mesh and Backspace returns to the folder", await pg.locator('[data-crumb="z:extras.zip!Extras/"]').count() == 1)
    await show_file("d:")
    await pg.click('#contents-views [data-view="list"]')
    await pg.click('.contents-columns [data-sort="size"]')
    check("contents list headers sort by largest first", await pg.locator('#contents-sort').get_attribute('data-sort') == 'size')
    projection = await pg.evaluate("""async () => {
      const {childrenOf, sortChildren} = await import('./ui/contents.js');
      const rows = childrenOf([{rel:'Z/a.stl',size:2},{rel:'a.stl',size:2,modified:10},{rel:'b.stl',size:9,modified:20}]);
      return {size:sortChildren(rows,'size').map(x=>x.key), newest:sortChildren(rows,'newest').map(x=>x.key)};
    }""")
    check("folder projection keeps folders first for size and newest sorts", projection == {"size": ["d:Z", "f:b.stl", "f:a.stl"], "newest": ["d:Z", "f:b.stl", "f:a.stl"]}, projection)
    await pg.click('#contents-views [data-view="grid"]')


async def phase4(pg):
    t = library / "Wargames/Warhammer 40k"
    before = sorted(x.name for x in (t / "Tyranid").iterdir())
    # 25. renaming a subcategory moves its folders (with a preview first)
    await pg.goto(B + "#/browse/schema/wargames/Warhammer%2040k/Tyranid")
    await category_menu(pg, "rename-subcategory")
    await pg.fill("#rename-name", "Tyranids")
    await pg.wait_for_selector("#change-preview[data-moving]")
    preview = await pg.inner_text("#change-preview")
    await pg.screenshot(path=str(out / "15-rename.png"))
    await pg.click("#rename-dialog button[type=submit]")
    await pg.wait_for_selector("#rename-dialog", state="detached", timeout=60000)
    await pg.wait_for_function("() => location.hash.includes('Tyranids')")
    n = await count(pg)
    after = sorted(x.name for x in (t / "Tyranids").iterdir()) if (t / "Tyranids").is_dir() else []
    side = json.loads((t / "Tyranids/Hive Guard (Jo Smith)/model.json").read_text())
    check("renaming a subcategory moves its folders, after a preview", after == before and not (t / "Tyranid").exists() and side["path"] == ["Warhammer 40k", "Tyranids"]
          and f"{len(before)} model folders move" in preview and n == f"{len(before)} models", (preview, before, after, n))

    # 26. undo from Home
    await pg.goto(B + "#/")
    await pg.wait_for_selector("#undo-change")
    await pg.screenshot(path=str(out / "16-recent-changes.png"))
    await pg.click("#undo-change")
    await pg.wait_for_function("() => /Undone/.test(document.querySelector('.toast')?.textContent || '')", timeout=60000)
    side = json.loads((t / "Tyranid/Hive Guard (Jo Smith)/model.json").read_text())
    check("the rename is undone from Home", sorted(x.name for x in (t / "Tyranid").iterdir()) == before and not (t / "Tyranids").exists()
          and side["path"] == ["Warhammer 40k", "Tyranid"], side.get("path"))

    # 27. merging: Necrons into Tyranid
    nec = sorted(x.name for x in (t / "Necrons").iterdir())
    await pg.goto(B + "#/browse/schema/wargames/Warhammer%2040k/Necrons")
    await category_menu(pg, "rename-subcategory")
    await pg.fill("#rename-name", "Tyranid")
    await pg.wait_for_selector("#rename-merge")
    await pg.wait_for_selector("#change-preview[data-moving]")
    await pg.click("#rename-dialog button[type=submit]")
    await pg.wait_for_selector("#rename-dialog", state="detached", timeout=60000)
    merged = sorted(x.name for x in (t / "Tyranid").iterdir())
    check("a subcategory renamed to one that exists merges into it", merged == sorted(before + nec) and not (t / "Necrons").exists(), merged)

    # 28. editing the category's tree: a new branch three deep and another at the
    # top (nothing moves), then Space Marines moved out a level and a new top folder
    await pg.goto(B + "#/browse/schema/wargames")
    await category_menu(pg, "edit-category")
    await pg.wait_for_selector("#es-tree")
    no_delete = await pg.locator("#edit-schema-dialog .danger-text").count() == 0
    await pg.click('#es-tree .st-row[data-path="Terrain"] .st-add')
    await pg.keyboard.type("Buildings")
    await pg.click('#es-tree .st-row[data-path="Terrain/Buildings"] .st-add')
    await pg.keyboard.type("Ruins")
    await pg.click("#es-tree-add")
    await pg.keyboard.type("Board games")
    await pg.wait_for_selector("#change-preview[data-moving='0']")
    await pg.screenshot(path=str(out / "17-edit-category.png"))
    await pg.click("#edit-schema-dialog button[type=submit]")
    await pg.wait_for_selector("#edit-schema-dialog", state="detached", timeout=60000)
    sch = json.loads((library / "_library/schemas/wargames.json").read_text())
    deep = (library / "Wargames/Terrain/Buildings/Ruins").is_dir() and (library / "Wargames/Board games").is_dir() and t.is_dir()
    tops = [x["name"] for x in sch["subcategories"]]
    await category_menu(pg, "edit-category")
    await pg.wait_for_selector("#es-tree")
    counted = await pg.inner_text('#es-tree .st-row[data-path="Warhammer 40k"] .st-count')
    await pg.click('#es-tree .st-row[data-path="Warhammer 40k/Space Marines"] .st-out')
    await pg.fill("#es-folder", "Tabletop")
    await pg.wait_for_function("() => +document.querySelector('#change-preview')?.dataset.moving > 0")
    await pg.screenshot(path=str(out / "17b-edit-category-move.png"))
    await pg.click("#edit-schema-dialog button[type=submit]")
    await pg.wait_for_selector("#edit-schema-dialog", state="detached", timeout=60000)
    moved = (library / "Tabletop/Warhammer 40k/Tyranid/Hive Guard (Jo Smith)/guard.stl").is_file() and not (library / "Wargames").exists() \
        and (library / "Tabletop/Space Marines/Captain (Jo Smith)").is_dir() and (library / "Tabletop/Terrain/Buildings/Ruins").is_dir()
    sch = json.loads((library / "_library/schemas/wargames.json").read_text())
    check("the category's tree is edited: new branches of any depth, and a moved subcategory and new top folder move its models",
          deep and tops == ["Board games", "Terrain", "Warhammer 40k"] and counted.strip().isdigit() and moved and no_delete
          and [x["name"] for x in sch["subcategories"]] == ["Board games", "Space Marines", "Terrain", "Warhammer 40k"], (deep, tops, counted, moved, sch.get("subcategories")))

    # 28b. subcategories added on a category's pages, at any depth, kept with no models
    t = library / "Tabletop/Warhammer 40k"
    await pg.goto(B + "#/browse/schema/wargames/Warhammer%2040k")
    for name in ("Orks", "Aeldari"):
        await category_menu(pg, "add-subcategory")
        await pg.fill("#subcat-name", name)
        await pg.click("#subcat-dialog button[type=submit]")
        await pg.wait_for_selector("#subcat-dialog", state="detached")
    await pg.goto(B + "#/browse/schema/wargames/Terrain/Buildings/Ruins")
    await category_menu(pg, "add-subcategory")
    await pg.fill("#subcat-name", "Gothic")
    await pg.click("#subcat-dialog button[type=submit]")
    await pg.wait_for_selector("#subcat-dialog", state="detached")
    await pg.goto(B + "#/browse/schema/wargames/Warhammer%2040k/Orks")
    await count(pg)
    ov = await pg.evaluate("async () => (await window.__modlib.platform.api('library_overview')).schemas[0].tree")
    w40k = [c["value"] for c in next(x for x in ov if x["value"] == "Warhammer 40k")["children"]]
    made = (t / "Orks").is_dir() and (t / "Aeldari").is_dir() and (library / "Tabletop/Terrain/Buildings/Ruins/Gothic").is_dir()
    await pg.click("#category-menu")
    await pg.wait_for_selector("#context-menu")
    items = await pg.eval_on_selector_all("#context-menu .menu-label", "els => els.map(e => e.textContent)")
    await pg.screenshot(path=str(out / "18-subcategories.png"))
    await pg.click('#context-menu [data-action="delete-subcategory"]')  # empty: at once, with Undo
    await pg.wait_for_function("() => !location.hash.includes('Orks')")
    gone = not (t / "Orks").exists()
    await toast_text(pg, "^Deleted the subcategory Orks")
    await pg.click(".toast .toast-action")
    await toast_text(pg, "^Undone")
    back = (t / "Orks").is_dir()
    check("subcategories are added at any depth, kept with no models, and deleted with Undo", made and gone and back and "Orks" in w40k and "Aeldari" in w40k
          and items == ["Add subcategory…", "Edit category…", "Rename or move…", "Delete subcategory"], (made, gone, back, w40k, items))
    # one with models in it: they move up a level, after a list of what moves
    model_folder("Tabletop/Terrain/Buildings/Ruins/Tower", {"tower.stl": cube(6)})
    await api(pg, "library_scan", {"full": False})
    await pg.goto(B + "#/browse/schema/wargames/Terrain/Buildings/Ruins")
    await pg.reload()
    await pg.wait_for_selector(".card:has(.card-name:text-is('Tower'))", timeout=30000)
    await pg.click("#category-menu")
    await pg.click('#context-menu [data-action="delete-subcategory"]')
    await pg.wait_for_selector("#delete-subcategory-dialog #change-preview[data-moving='1']", timeout=30000)
    red = "danger" in (await pg.get_attribute("#delete-subcategory-dialog button[type=submit]", "class"))
    await pg.click("#delete-subcategory-dialog button[type=submit]")
    await pg.wait_for_selector("#delete-subcategory-dialog", state="detached", timeout=60000)
    up = (library / "Tabletop/Terrain/Buildings/Tower/tower.stl").is_file() and not (library / "Tabletop/Terrain/Buildings/Ruins").exists()
    await toast_text(pg, "^Deleted the subcategory Ruins")
    await pg.keyboard.press("Control+z")
    await toast_text(pg, "^Undone", 60000)
    down = (library / "Tabletop/Terrain/Buildings/Ruins/Tower/tower.stl").is_file() and (library / "Tabletop/Terrain/Buildings/Ruins/Gothic").is_dir()
    check("a subcategory with models is deleted after a list of what moves up, and Ctrl+Z puts it back", red and up and down, (red, up, down))

    # 29. several models' details at once
    await pg.goto(B + "#/browse/schema/wargames")
    await pg.wait_for_selector(".card:has(.card-name:text-is('Gargoyle'))")
    await pg.click(".card:has(.card-name:text-is('Gargoyle'))")
    await pg.click(".card:has(.card-name:text-is('dragon'))", modifiers=["Control"])
    await pg.click("#picked-edit")
    await pg.fill("#bulk-tags-add", "painted, display")
    await pg.fill("#bulk-license", "CC-BY")
    await pg.click("#bulk-dialog button[type=submit]")
    await pg.wait_for_selector("#bulk-dialog", state="detached")
    tags = [json.loads((library / f"Tabletop/Warhammer 40k/Tyranid/{m}/model.json").read_text()) for m in ("Gargoyle", "dragon")]
    check("several models' details are edited at once", all(x.get("tags") == ["painted", "display"] and x.get("license") == "CC-BY" for x in tags), tags)


async def api(pg, cmd, args=None):
    return await pg.evaluate("([c, a]) => window.__modlib.platform.api(c, a)", [cmd, args or {}])


async def phase5(pg):
    # 30. a tidy share sorted as it is: a folder sent with its folders kept, loose files grouped
    await api(pg, "schema_create", {"schema": {"name": "Household"}})
    share = home / "NAS share"
    put(share / "Home items/Kitchen/Spoon rest (Jo)/spoon.stl", cube(11))
    put(share / "Home items/Kitchen/Spoon rest (Jo)/photo.png", PNG)
    put(share / "Home items/Kitchen/Gadgets/Bag clip/Presupported/clip.stl", cube(12))
    put(share / "Home items/Kitchen/Gadgets/Bag clip/Unsupported/clip.stl", cube(12.5))
    put(share / "Home items/Office/Lamp/lamp.stl", cube(13))
    put(share / "hinge.stl", cube(14))
    put(share / "latch.stl", cube(15))
    await pg.goto(B + "#/import")
    await pg.reload()  # the new category in the pickers
    await pg.wait_for_selector("#sort-page[data-ready]")
    await start_again(pg)
    pick(share)
    await pg.click("#import-sort")
    await pg.click('[data-view="folders"]', timeout=30000)
    await pg.wait_for_selector('#sort-tree .sw-folder[data-name="Home items"]', timeout=30000)
    await pg.click('#sort-tree .sw-folder[data-name="Home items"] .sw-name')
    await pg.select_option("#send-schema", "household")
    await pg.check("#send-keep")
    await pg.uncheck("#send-keep-self")
    await pg.screenshot(path=str(out / "19-sort-folders.png"))
    await pg.click("#send-go")
    await toast_text(pg, "^Set 3 models to go to")
    names = sorted(await sort_rows(pg))
    await pick_rows(pg, "hinge", "latch")
    await pg.click("#sort-group")
    await toast_text(pg, "^Combined into one model")
    # the new model's details (named after its folder) before renaming it
    await pg.wait_for_function("() => document.querySelector('#details-name')?.value === 'NAS share'")
    await pg.fill("#details-name", "Hinge and latch")
    await pg.press("#details-name", "Tab")
    await pg.wait_for_selector('#sort-list tr[data-name="Hinge and latch"]')
    grouped = sorted(await sort_rows(pg))
    await pick_rows(pg, "Hinge and latch")
    await send_to(pg, "household", None, "Bits and bobs")
    await pg.click('[data-view="grid"]')
    await pg.wait_for_selector("#sort-grid .card")
    await pg.wait_for_timeout(1500)  # previews are drawn
    cards = await pg.locator("#sort-grid .card").count()
    await pg.screenshot(path=str(out / "20-sort-grid.png"))
    await pg.click('[data-view="category"]')
    await pg.wait_for_selector("#sort-groups")
    groups = await pg.eval_on_selector_all("#sort-groups [data-group]", "els => els.map(e => e.dataset.group)")
    await pg.screenshot(path=str(out / "21-sort-by-category.png"))
    title = await import_and_wait(pg)
    h = library / "Household"
    on_disk = [(h / "Kitchen/Spoon rest (Jo)/spoon.stl").is_file(), (h / "Kitchen/Gadgets/Bag clip/Presupported/clip.stl").is_file(),
               (h / "Office/Lamp/lamp.stl").is_file(), (h / "Bits and bobs/Hinge and latch/hinge.stl").is_file(), (h / "Bits and bobs/Hinge and latch/latch.stl").is_file()]
    spoon = json.loads((h / "Kitchen/Spoon rest (Jo)/model.json").read_text())
    check("a tidy share comes in as it is: folders kept as subcategories, loose files grouped into one model",
          names == ["Bag clip", "Lamp", "Spoon rest", "hinge", "latch"] and grouped == ["Bag clip", "Hinge and latch", "Lamp", "Spoon rest"] and cards == 4
          and groups == ["Household › Bits and bobs", "Household › Kitchen", "Household › Kitchen › Gadgets", "Household › Office"]
          and title.startswith("4 models moved") and all(on_disk) and spoon["path"] == ["Kitchen"] and spoon["authors"] == [{"name": "Jo"}],
          (names, grouped, cards, groups, title, on_disk, spoon))

    # 31. changes made outside the app show up by themselves
    await pg.goto(B + "#/browse/unsorted")
    await count(pg)
    put(library / "Unsorted/Dropped In/dropped.stl", cube(4))
    await pg.wait_for_selector(".card:has(.card-name:text-is('Dropped In'))", timeout=30000)
    put(library / "Unsorted/Hand Moved/moved.stl", cube(3))
    put(library / "Unsorted/Hand Moved/model.json", json.dumps({"id": "mhandmoved", "name": "Hand Moved"}))
    await pg.wait_for_selector(".card:has(.card-name:text-is('Hand Moved'))", timeout=30000)
    (h / "Office/Hand Moved").parent.mkdir(parents=True, exist_ok=True)
    (library / "Unsorted/Hand Moved").rename(h / "Office/Hand Moved")
    await pg.wait_for_selector(".card:has(.card-name:text-is('Hand Moved'))", state="detached", timeout=30000)
    m = await api(pg, "model_get", {"id": "mhandmoved"})
    for _ in range(50):  # its model.json is told its new place
        side = json.loads((h / "Office/Hand Moved/model.json").read_text())
        if side.get("path") == ["Office"]:
            break
        await pg.wait_for_timeout(200)
    check("folders added or moved outside the app show up by themselves; a moved model keeps its id", m["rel"] == "Household/Office/Hand Moved"
          and side.get("schema") == "household" and side.get("path") == ["Office"], (m.get("rel"), side))

    # 32. duplicates: found by content, an extra copy set aside, undone, set aside again and deleted
    put(library / "Unsorted/Bracket/bracket.stl", cube(21))
    put(library / "Unsorted/Bracket copy/bracket.stl", cube(21))
    put(library / "Unsorted/Bracket copy/notes.txt", "notes")
    for _ in range(100):
        if (await api(pg, "models_query", {"scope": "unsorted", "q": "bracket"}))["total"] == 2:
            break
        await pg.wait_for_timeout(200)
    await pg.click('.sidebar a[href="#/duplicates"]')
    await pg.wait_for_selector("#dupes-page")
    await pg.click("#dupes-find")
    group = pg.locator(".dupe-group:has(.dupe-name:text-is('Bracket copy'))")
    await group.wait_for(timeout=60000)
    keep = await group.locator("li.keep .dupe-name").inner_text()
    summary = await pg.inner_text("#dupes-summary")
    await pg.screenshot(path=str(out / "22-duplicates.png"), full_page=True)
    hashed = json.loads((h / "Office/Lamp/model.json").read_text()).get("hashes", {})
    await clear_toast(pg)
    await group.locator(".dupe-set-aside").click()
    await toast_text(pg, "^Set aside 1 copy")
    aside = library / "_library/set-aside/Unsorted/Bracket"
    set_aside = aside.is_dir() and not (library / "Unsorted/Bracket").exists()
    listed = (await api(pg, "models_query", {"scope": "unsorted", "q": "bracket"}))["total"]
    check("duplicates are found by their contents and an extra copy is set aside", keep == "Bracket copy" and "copies" in summary and set_aside and listed == 1
          and "sha256" in hashed.get("lamp.stl", {}), (keep, summary, set_aside, listed, hashed))
    await pg.click(".toast .toast-action")
    await pg.wait_for_function("() => /back where they were/.test(document.querySelector('.toast')?.textContent || '')", timeout=30000)
    back = (library / "Unsorted/Bracket/bracket.stl").is_file() and not aside.exists()
    await group.wait_for(timeout=30000)
    await group.locator(".dupe-set-aside").click()
    await pg.wait_for_selector("#dupes-aside")
    await pg.click("#dupes-empty")
    await pg.wait_for_selector("#confirm-dialog")
    red = "danger" in (await pg.get_attribute("#confirm-dialog button[type=submit]", "class"))
    await pg.screenshot(path=str(out / "23-duplicates-delete.png"))
    await pg.click("#confirm-dialog button[type=submit]")
    await pg.wait_for_selector("#dupes-aside", state="detached")
    gone = not (library / "_library/set-aside").exists() and (library / "Unsorted/Bracket copy/bracket.stl").is_file()
    await pg.goto(B + "#/")
    await pg.wait_for_selector("#recent-changes")
    recent = await pg.inner_text("#recent-changes")
    check("setting aside is undone, or the copies deleted after asking (red)", back and gone and red and "Set aside 1 duplicate copy" in recent and "copies deleted" in recent, (back, gone, red, recent))


async def labels_of(pg, sel):
    return await pg.eval_on_selector_all(sel, "els => els.map(e => e.textContent.trim())")


async def ui_pass(pg):
    # 33. one action row, and the same actions on right-click
    await pg.goto(B + "#/browse/unsorted")
    await count(pg)
    first = pg.locator(".results .card").first
    await first.click()
    await pg.wait_for_selector("#model-details .action-row")
    row = await labels_of(pg, "#model-details .action-row button")
    await first.click(button="right")
    await pg.wait_for_selector("#context-menu")
    menu = await labels_of(pg, "#context-menu .menu-label")
    await pg.screenshot(path=str(out / "24-right-click.png"))
    await pg.keyboard.press("Escape")
    await pg.wait_for_selector("#context-menu", state="detached")
    await pg.click("#details-more")
    more = await labels_of(pg, "#context-menu .menu-label")
    await pg.keyboard.press("Escape")
    check("the details panel and the right-click menu have the same actions, in the same order",
          row == ["Open", "Edit details…", "Move to category…", "Star", "Show in folder", "More ▾"]
          and menu == ["Open", "Edit details…", "Move to category…", "Star", "Show in folder", "Compress to ZIP…", "Extract archive…", "Make a new preview", "Copy folder path"]
          and more == ["Compress to ZIP…", "Extract archive…", "Make a new preview", "Copy folder path"], (row, menu, more))

    # 34. keys: arrows and Shift, Ctrl+A, Esc, E, M, S, Ctrl+Z
    n = int((await count(pg)).split()[0])
    await first.click()
    a = await first.get_attribute("data-model")
    await pg.keyboard.press("ArrowRight")
    b = await pg.evaluate("() => [...document.querySelectorAll('.results [aria-selected=true]')].map(e => e.dataset.model)")
    await pg.keyboard.press("Shift+ArrowLeft")
    two = await pg.inner_text("#picked-panel h2")
    await pg.keyboard.press("Control+a")
    every = await pg.inner_text("#picked-panel h2")
    await pg.keyboard.press("Escape")
    cleared = await pg.locator(".results [aria-selected=true]").count()
    await first.click()
    await pg.keyboard.press("e")
    await pg.wait_for_selector("#details-dialog")
    typing_in = await pg.evaluate("() => document.activeElement?.id")
    await pg.keyboard.press("Escape")
    await pg.wait_for_selector("#details-dialog", state="detached")
    await pg.keyboard.press("m")
    await pg.wait_for_selector("#move-dialog")
    await pg.keyboard.press("Escape")
    await pg.wait_for_selector("#move-dialog", state="detached")
    stars = await pg.inner_text('.sidebar a[href="#/browse/favs"] .nav-count')
    await pg.keyboard.press("s")
    said = await toast_text(pg, "^Starred")
    await pg.wait_for_function("() => document.querySelector('.sidebar a[href=\"#/browse/favs\"] .nav-count')?.textContent === '2'")
    await pg.keyboard.press("Control+z")
    await toast_text(pg, "^Undone")
    await pg.wait_for_function("() => document.querySelector('.sidebar a[href=\"#/browse/favs\"] .nav-count')?.textContent === '1'")
    favs = json.loads((library / "_library/library.json").read_text()).get("favourites", [])
    check("keys select, edit, move, star and undo, and Starred counts its models",
          b and b[0] != a and len(b) == 1 and two == "2 models selected" and every == f"{n} models selected" and cleared == 0
          and typing_in == "edit-name" and said.startswith("Starred") and stars == "1" and len(favs) == 1, (a, b, two, every, cleared, typing_in, said, stars, favs))

    # 35. a model selected in one place isn't shown in another (bug: it was)
    await first.click()
    await pg.goto(B + "#/browse/schema/household/Office")
    await count(pg)
    leaked = await pg.locator("#model-details").count()
    check("the details panel doesn't show a model from another place", leaked == 0, leaked)

    # 36. filter chips come off again, the theme button has the four choices
    await pg.goto(B + "#/browse/all")
    await count(pg)
    chip = pg.locator(".filters .chip-btn").first
    f = await chip.get_attribute("data-filter")
    await chip.click()
    await pg.wait_for_function("f => document.querySelector('#search').value.includes(f)", arg=f)
    await pg.click(f".filters .chip-btn[data-filter={json.dumps(f)}]")  # a JSON string is a CSS string
    await pg.wait_for_function("() => document.querySelector('#search').value === ''")
    themes = []
    for _ in range(4):
        await pg.click("#theme-toggle")
        themes.append(await pg.get_attribute("#theme-toggle", "data-theme"))
    check("filter chips come off again, and the theme button goes back to following the system", themes == ["light", "dark", "night", "system"], (f, themes))

    # 37. the sidebar's Category menu; deleting a category is red and asks
    await pg.click('.sidebar a[href="#/browse/schema/household"]', button="right")
    items = await labels_of(pg, "#context-menu .menu-label")
    await pg.click('#context-menu [data-action="delete-category"]')
    await pg.wait_for_selector("#delete-schema-dialog")
    red = "danger" in (await pg.get_attribute("#delete-schema-dialog button[type=submit]", "class"))
    await pg.screenshot(path=str(out / "25-delete-category.png"))
    await pg.click("#delete-schema-dialog .dialog-actions .ghost")
    await pg.wait_for_selector("#delete-schema-dialog", state="detached")
    check("a category's menu is on the sidebar too, and Delete category is red", items == ["Add subcategory…", "Edit category…", "Delete category…"] and red, (items, red))

    # 38. files without a viewer show their details and an external-app action
    put(library / "Unsorted/Knight Armour/settings.ini", "x")
    await api(pg, "library_scan", {"full": False})
    await pg.goto(B + "#/browse/unsorted")
    await pg.dblclick(".card:has(.card-name:text-is('Knight Armour'))")
    await pg.wait_for_selector("#part-tree [data-file='settings.ini']", timeout=30000)
    await pg.click("#part-tree [data-file='settings.ini']")
    await pg.wait_for_selector(".workspace-no-view")
    fallback = await pg.inner_text(".workspace-no-view")
    await pg.click("#part-tree [data-file='settings.ini']", button="right")
    file_menu = await labels_of(pg, "#context-menu .menu-label")
    await pg.keyboard.press("Escape")
    mp_row = await labels_of(pg, "#model-page .action-row button")
    check("files with no viewer show details and open in their own app; the model page has the same row", "settings.ini" in fallback and "Open externally" in fallback
          and "Open externally" in file_menu and "Show in folder" in file_menu and "Make a new model…" in file_menu
          and mp_row == ["Edit details…", "Move to category…", "Star", "Show in folder", "More ▾"], (fallback, file_menu, mp_row))

    # 39. Import: right-click, Ctrl+A and Esc, Ctrl+Z, and messages clear of the footer
    more = home / "More"
    put(more / "Widget/widget.stl", cube(5))
    put(more / "Gizmo/gizmo.stl", cube(6))
    await pg.goto(B + "#/import")
    await pg.wait_for_selector("#sort-page[data-ready]")
    await start_again(pg)
    pick(more)
    await pg.click("#import-sort")
    await pg.wait_for_function("() => !document.querySelector('#import-progress')")
    await sort_rows(pg)
    row = '#sort-list tr[data-name="Widget"]'
    await pg.click(row, button="right")
    imenu = await labels_of(pg, "#context-menu .menu-label")
    await pg.keyboard.press("Escape")
    await pg.click(row + " .sw-name")
    await pg.keyboard.press("Control+a")
    both = await pg.inner_text("#sort-picked")
    await pg.keyboard.press("Escape")
    none = await pg.locator("#sort-actions").count()
    await pick_rows(pg, "Widget")
    await send_to(pg, "household")
    placed = await pg.inner_text(row)
    await pg.keyboard.press("Control+z")
    await toast_text(pg, "^Undone")
    await pg.wait_for_function("r => document.querySelector(r)?.textContent.includes('No category')", arg=row)
    apart = await pg.evaluate("() => { const t = document.querySelector('.toast').getBoundingClientRect(); const f = document.querySelector('.sw-go').getBoundingClientRect(); return t.bottom <= f.top + 1 || t.top >= f.bottom - 1; }")
    await pg.screenshot(path=str(out / "26-import-undo.png"))
    check("Import has the same right-click menu, keys and Undo, and messages don't cover its footer",
          imenu[:3] == ["Edit details", "Set category…", "Show in folder"] and "Skip" in imenu and both == "2 models selected" and none == 0
          and "Household" in placed and apart, (imenu, both, none, placed, apart))
    await start_again(pg)


async def ui_pass2(pg):
    """UI pass step 2: every change is recorded and can be undone; one red confirm."""
    # 40. Move to category lists the folders that move first, and is undone from its message
    await pg.goto(B + "#/browse/unsorted")
    await pg.click(".card:has(.card-name:text-is('Dropped In'))")
    await pg.keyboard.press("m")
    await pg.wait_for_selector("#move-dialog")
    await pg.select_option("#move-schema", "household")
    await pg.fill("#move-new", "Shelf")
    await pg.wait_for_function("() => document.querySelector('#change-preview')?.textContent.includes('Shelf/Dropped In')")
    preview = await pg.inner_text("#change-preview")
    button = await pg.inner_text("#move-dialog button[type=submit]")
    await clear_toast(pg)
    await pg.screenshot(path=str(out / "27-move-preview.png"))
    await clear_toast(pg)
    await pg.click("#move-dialog button[type=submit]")
    await pg.wait_for_selector("#move-dialog", state="detached", timeout=60000)
    said = await toast_text(pg, "^Moved Dropped In to")
    moved = (library / "Household/Shelf/Dropped In").is_dir()
    await pg.click(".toast .toast-action")
    await toast_text(pg, "^Undone", 60000)
    back = (library / "Unsorted/Dropped In").is_dir() and not (library / "Household/Shelf").exists()
    check("Move to category shows the folders that move, and Undo in its message puts them back", "1 model folder moves" in preview and button == "Move 1 model"
          and said.startswith("Moved Dropped In to Household › Shelf") and moved and back, (preview, button, said, moved, back))

    # 41. Edit details is undone with Ctrl+Z
    await pg.goto(B + "#/browse/unsorted")
    await pg.click(".card:has(.card-name:text-is('Hand Made'))")
    await pg.keyboard.press("e")
    await pg.wait_for_selector("#details-dialog")
    await pg.fill("#edit-name", "Hand Made Two")
    await pg.click("#details-dialog button[type=submit]")
    await pg.wait_for_selector("#details-dialog", state="detached")
    await toast_text(pg, "^Saved Hand Made Two")
    side = library / "Unsorted/Hand Made/model.json"
    named = json.loads(side.read_text())["name"]
    await pg.keyboard.press("Control+z")
    await toast_text(pg, "^Undone", 60000)
    await pg.wait_for_selector(".card:has(.card-name:text-is('Hand Made'))")
    check("Edit details is undone with Ctrl+Z", named == "Hand Made Two" and json.loads(side.read_text())["name"] == "Hand Made", named)

    # 42. an import is undone from its results: the files go back, and the workspace has them again
    src = home / "Undo me"
    put(src / "Gadget/gadget.stl", cube(7))
    await pg.goto(B + "#/import")
    await pg.wait_for_selector("#sort-page[data-ready]")
    await start_again(pg)
    pick(src)
    await pg.click("#import-sort")
    await pg.wait_for_selector('#sort-tree .sw-item[data-name="Gadget"], #sort-list tr[data-name="Gadget"]', timeout=30000)
    await sort_rows(pg)
    await pick_rows(pg, "Gadget")
    await send_to(pg, "household", "Bits and bobs")
    await import_and_wait(pg)
    inside = (library / "Household/Bits and bobs/Gadget/gadget.stl").is_file() and not (src / "Gadget").exists()
    await pg.screenshot(path=str(out / "28-import-results.png"))
    await clear_toast(pg)
    await pg.click("#import-undo")
    await toast_text(pg, "^Undone", 60000)
    await pg.wait_for_selector("#import-results", state="detached")
    back = (src / "Gadget/gadget.stl").is_file() and not (src / "Gadget/model.json").exists() and not (library / "Household/Bits and bobs/Gadget").exists()
    await sort_rows(pg)
    row = await pg.inner_text('#sort-list tr[data-name="Gadget"]')
    check("an import is undone from its results: the files go back and the workspace has them again", inside and back and "Bits and bobs" in row, (inside, back, row))
    await start_again(pg)

    # 43. Home: an older change to details can be undone while newer ones leave its model alone
    q = await api(pg, "models_query", {"scope": "unsorted"})
    by = {m["name"]: m["id"] for m in q["items"]}
    await api(pg, "model_update", {"id": by["Benchy"], "patch": {"tags": "boat"}, "journal": True})
    await api(pg, "model_update", {"id": by["Knight Armour"], "patch": {"tags": "armour"}, "journal": True})
    await pg.goto(B + "#/")
    await pg.wait_for_selector("#recent-changes .undo-change")
    rows = await pg.eval_on_selector_all("#recent-changes li", "els => els.map(e => [e.querySelector('span').textContent, !!e.querySelector('.undo-change:not([disabled])')])")
    older = next((ok for label, ok in rows if label.startswith("Edited Benchy's details")), None)
    await clear_toast(pg)
    await pg.screenshot(path=str(out / "29-recent-changes.png"))
    await pg.click("#recent-changes li:has-text(\"Edited Benchy's details\") .undo-change")
    await toast_text(pg, "^Undone", 60000)
    tags = json.loads((library / "Unsorted/Benchy/model.json").read_text()).get("tags", [])
    check("Home undoes an older change to details while newer changes leave that model alone", older is True and "boat" not in tags, (rows[:4], tags))

    # 44. Read again is a job with Stop in the status bar
    await library_menu(pg, "rescan")
    said = await toast_text(pg, "models, read in|^Stopped")
    check("Read again runs as a job and says what it read", "read in" in said, said)



async def ui_pass3(pg):
    """UI pass step 3: one page layout (header, toolbar, details panel, Home)."""
    # 45. every page starts with the same header: the title and its count, then the page's main button and menu
    q = await api(pg, "models_query", {"scope": "unsorted", "sort": "name"})
    knight = next(m for m in q["items"] if m["name"] == "Knight Armour")
    heads = []
    for route, ready in [("#/", "#library-menu"), ("#/browse/all", "#browse-count"), ("#/browse/schema/household", "#category-menu"), (f"#/model/{knight['id']}", "#mp-head .action-row"),
                         ("#/import", "#import-more"), ("#/duplicates", "#dupes-find"), ("#/settings", "#settings-about")]:
        await pg.goto(B + route)
        await pg.wait_for_selector(f".page-head {ready}, {ready}")
        heads.append(await pg.evaluate("() => { const h = document.querySelectorAll('.page-head'); return h.length === 1 ? h[0].querySelector('.page-title h1')?.textContent.trim() || null : h.length; }"))
    check("every page starts with the same header", heads == ["My Models", "All models", "Household", "Knight Armour", "Import", "Duplicates", "Settings"], heads)

    # 46. one view switcher (icon, label, tooltip) and one sort menu; the list's headers sort too
    await pg.goto(B + "#/browse/all")
    await count(pg)
    switch = await pg.eval_on_selector_all(".view-switch button", "els => els.map(e => [e.dataset.view, !!e.querySelector('svg'), e.querySelector('.view-label')?.textContent, e.title])")
    await pg.click("#sort")
    sorts = await labels_of(pg, "#context-menu .menu-label")
    await pg.click('#context-menu [data-action="sort-size"]')
    await pg.wait_for_selector('#sort[data-sort="size"]')
    await pg.click('.view-switch [data-view="list"]')
    await pg.wait_for_selector("#list-head")
    sizes = await pg.eval_on_selector_all(".results .row", "els => els.map(e => e.querySelector('.row-size').textContent)")
    await pg.click('#list-head [data-sort="name"]')
    await pg.wait_for_selector('#sort[data-sort="name"]')
    try:  # the rows follow once the new order comes back
        await pg.wait_for_function("() => { const n = [...document.querySelectorAll('.results .row .row-name')].map(e => e.textContent.trim().toLowerCase()); return n.length > 2 && n.every((x, i) => !i || n[i - 1] <= x); }", timeout=5000)
    except Exception:
        pass
    names = await pg.eval_on_selector_all(".results .row .row-name", "els => els.map(e => e.textContent.trim().toLowerCase())")
    await pg.click('.view-switch [data-view="grid"]')
    check("one view switcher and one sort menu, and the list's headers sort",
          switch == [["grid", True, "Grid", "Grid"], ["list", True, "List", "List"]] and sorts == ["Name", "Recently added", "Largest first"]
          and len(sizes) > 2 and names == sorted(names), (switch, sorts, sizes[:4], names[:4]))

    # 47. a search carried into another place says so, and can be cleared there
    await pg.goto(B + "#/browse/all")
    await search(pg, "knight")
    await pg.click('.sidebar a[href="#/browse/schema/household"]')
    await pg.wait_for_function("() => document.querySelector('#search-note')?.textContent.includes('found in Household')")
    note = await pg.inner_text("#search-note")
    nothing = await pg.inner_text("#no-results")
    await pg.click("#search-clear")
    await pg.wait_for_function("() => !document.querySelector('#search-note') && document.querySelector('#search').value === ''")
    check("a search carried into another place says so and clears there", note.startswith("Searching for knight") and "0 found in Household" in note and "Search all models" in note
          and "Nothing in Household" in nothing, (note, nothing))

    # 48. the details panel is changed in place, with Undo; nothing selected sums up the place; several show what they share
    await pg.goto(B + "#/browse/unsorted")
    await count(pg)
    await pg.keyboard.press("Escape")  # nothing selected (a model may still be from earlier)
    await pg.wait_for_selector("#place-summary #place-counts")
    summary = await pg.inner_text("#place-summary #place-counts")
    await pg.click(".card:has(.card-name:text-is('Knight Armour'))")
    await pg.wait_for_function("() => document.querySelector('#details-name')?.value === 'Knight Armour'")
    side = library / "Unsorted/Knight Armour/model.json"
    before = json.loads(side.read_text()).get("authors") or []
    await clear_toast(pg)
    await pg.fill("#details-authors", "Ann Smith, Bo Lee")
    await pg.press("#details-authors", "Enter")
    said = await toast_text(pg, "^Saved the authors of Knight Armour")
    saved = json.loads(side.read_text()).get("authors")
    await pg.click(".toast .toast-action")
    await toast_text(pg, "^Undone")
    undone = json.loads(side.read_text()).get("authors") or []
    await pg.wait_for_function("v => document.querySelector('#details-authors')?.value === v", arg=", ".join(x["name"] for x in before))
    await pg.click(".card:has(.card-name:text-is('Benchy'))", modifiers=["Control"])
    await pg.wait_for_selector("#picked-shared")
    shared = await pg.inner_text("#picked-shared")
    await pg.keyboard.press("Escape")
    check("the details panel is changed in place with Undo; it sums up the place, and what several share",
          "models" in summary and said and saved == [{"name": "Ann Smith"}, {"name": "Bo Lee"}] and undone == before and "Category" in shared and "Unsorted" in shared,
          (summary, said, saved, undone, shared))

    # 49. Home: recently added, waiting to be sorted, and one Library menu; the layout is in Settings
    await pg.goto(B + "#/")
    await pg.wait_for_selector("#home-added .home-model")
    added = await pg.locator("#home-added .home-model").count()
    await pg.click("#library-menu")
    lib_menu = await labels_of(pg, "#context-menu .menu-label")
    await pg.keyboard.press("Escape")
    waiting = await pg.inner_text("#home-waiting")
    await pg.evaluate("() => document.activeElement?.blur()")
    await clear_toast(pg)
    await pg.screenshot(path=str(out / "30-home.png"))
    await pg.goto(B + "#/settings")
    tree = await pg.inner_text("#settings-about .folder-tree")
    check("Home has recently added models, what's waiting and the Library menu; the layout is in Settings",
          added == 6 and lib_menu == ["Show in folder", "Read again", "Make missing previews", "Open another library…"] and "Unsorted" in waiting and "model.json" in tree,
          (added, lib_menu, waiting))

    # 50. Duplicates and Import have the same details panel
    await pg.goto(B + "#/duplicates")
    await pg.click("#dupes-find")
    first = pg.locator(".dupe-models li").first
    await first.wait_for(timeout=60000)
    name = await first.locator(".dupe-name").inner_text()
    await first.click()
    await pg.wait_for_function("n => document.querySelector('#model-details #details-name')?.value === n", arg=name)
    dupes_row = await labels_of(pg, "#model-details .action-row button")
    await clear_toast(pg)
    await pg.screenshot(path=str(out / "31-duplicates-panel.png"))
    src = home / "Panel test"
    put(src / "Widget two/widget.stl", cube(6))
    await pg.goto(B + "#/import")
    await start_again(pg)
    pick(src)
    await pg.click("#import-sort")
    await pg.wait_for_selector("#sort-summary")
    empty_panel = await pg.inner_text("#sort-summary")
    await sort_rows(pg)
    await pick_rows(pg, "Widget two")
    inside = await pg.evaluate("() => !!document.querySelector('#sort-details #details-name') && !!document.querySelector('#sort-details #sort-actions')")
    import_switch = await pg.eval_on_selector_all("#sort-page .toolbar .view-switch button", "els => els.map(e => !!e.querySelector('svg') && !!e.querySelector('.view-label'))")
    await clear_toast(pg)
    await pg.screenshot(path=str(out / "32-import-panel.png"))
    await start_again(pg)
    check("Duplicates and Import show the same details panel, and Import the same view switcher", dupes_row[:2] == ["Open", "Edit details…"] and "with no category" in empty_panel and inside
          and import_switch == [True] * 4, (name, dupes_row, empty_panel, inside, import_switch))


async def drop(pg, *paths, at=(500, 300)):
    """Drag files over the window and drop them, as a file manager would."""
    await pg.evaluate("() => window.__shimEmit('tauri://drag-enter', {})")
    hint = await pg.inner_text("#drop-hint")
    await pg.evaluate("a => window.__shimEmit('tauri://drag-drop', { paths: a.paths, position: { x: a.x, y: a.y } })", {"paths": [str(x) for x in paths], "x": at[0], "y": at[1]})
    await pg.wait_for_selector("#context-menu .menu-head")
    return hint


async def import_follow_ups(pg):
    """The owner's asks after the UI pass: the model's folder level, the drop menu, file types."""
    # 51. file types are easy to tell apart on Import: a tag per type, folders amber
    src = home / "Makers"
    for rel in ["Ann/2024-01/Knight/Body/body.stl", "Ann/2024-01/Knight/Arms/arm.stl", "Ann/2024-02/Dragon/Wings/wing.stl",
                "Ann/2024-02/Dragon/Head/head.stl", "Bo/2024-03/Robot/Legs/leg.stl"]:
        put(src / rel, cube(5))
    put(src / "Bo/2024-03/Robot/Torso/torso.3mf", "PK 3mf")
    put(src / "Bo/2024-03/Robot/Torso/torso.zip", "PK zip")
    put(src / "Bo/notes.txt", "hello")
    await pg.goto(B + "#/import")
    await start_again(pg)
    pick(src)
    await pg.click("#import-sort")
    await pg.wait_for_selector("#sort-page .sw-main", timeout=30000)
    await pg.click('[data-filter="all"]')
    await tree_items(pg)
    torso = await pg.eval_on_selector_all('#sort-tree .sw-item[data-name="Torso"] .ft', "els => els.map(e => [e.dataset.type, e.className])")
    body = await pg.eval_on_selector_all('#sort-tree .sw-item[data-name="Body"] .ft', "els => els.map(e => e.dataset.type)")
    loose = await pg.eval_on_selector_all('#sort-tree .sw-left[data-name="notes.txt"] .ft', "els => els.map(e => e.dataset.type)")
    folder = await pg.locator('#sort-tree .sw-folder[data-name="Ann"] .sw-mark-folder').count()
    colours = await pg.evaluate("() => ['stl', '3mf', 'zip'].map(t => getComputedStyle(document.querySelector(`.ft[data-type='${t}']`)).backgroundColor)")
    await pick_rows_tree(pg, "Torso")
    await pg.wait_for_selector("#sort-details .tree-file .ft")
    files = await pg.eval_on_selector_all("#sort-details .tree-file .ft", "els => els.map(e => e.dataset.type)")
    check("file types are easy to tell apart on Import: a coloured tag per type, folders marked",
          sorted(t for t, _ in torso) == ["3mf", "zip"] and "ft-archive" in dict(torso)["zip"] and body == ["stl"] and loose == ["txt"] and folder == 1
          and len(set(colours)) == 3 and sorted(files) == ["3mf", "zip"], (torso, body, loose, folder, colours, files))

    # 52. the parts were read as models: every folder at the model's level is made one, and that's undone
    before = sorted(await tree_items(pg))
    await pg.click('#sort-tree .sw-folder[data-name="Knight"] .sw-name')
    await pg.wait_for_selector("#sort-join-level")
    level_label = await pg.inner_text("#sort-join-level")
    await clear_toast(pg)
    await pg.screenshot(path=str(out / "33-import-level.png"))
    await pg.click("#sort-join-level")
    said = await toast_text(pg, "^Made 3 folders into models")
    after = sorted(await tree_items(pg))
    await pg.click(".toast .toast-action")
    await toast_text(pg, "^Undone")
    undone = sorted(await tree_items(pg))
    check("every folder at a level can be made a model, with Undo", before == ["Arms", "Body", "Head", "Legs", "Torso", "Wings"]
          and "(3)" in level_label and after == ["Dragon", "Knight", "Robot"] and undone == before, (before, level_label, said, after, undone))

    # 53. a model's panel shows the folders above it; clicking one makes it the model
    await pick_rows_tree(pg, "Wings")
    crumbs = await pg.eval_on_selector_all("#sort-level .sw-crumb", "els => els.map(e => e.textContent)")
    await clear_toast(pg)
    await pg.screenshot(path=str(out / "35-model-folder.png"))
    await pg.click('#sort-level .sw-crumb[data-folder="Dragon"]')
    await pg.wait_for_function("() => document.querySelector('#sort-details #details-name')?.value === 'Dragon'")
    kids = sorted(await tree_items(pg))
    check("a folder above a model can be chosen as the model", crumbs == ["Makers", "Ann", "2024-02", "Dragon"] and "Dragon" in kids and "Wings" not in kids and "Head" not in kids, (crumbs, kids))
    await start_again(pg)

    # 54. files dropped on a category page: a menu asks; added there (moved in), undone back where they were
    lamp = home / "Desk/Lamp"
    put(lamp / "lamp.stl", cube(7))
    put(lamp / "lamp.png", PNG)
    await pg.goto(B + "#/browse/schema/household/Kitchen")
    await count(pg)
    hint = await drop(pg, lamp)
    head = await pg.inner_text("#context-menu .menu-head")
    menu = await labels_of(pg, "#context-menu .menu-item .menu-label")
    await clear_toast(pg)
    await pg.screenshot(path=str(out / "34-drop-menu.png"))
    await pg.click('#context-menu [data-action="drop-add"]')
    said = await toast_text(pg, "^Moved Lamp into")
    moved = (library / "Household/Kitchen/Lamp/lamp.stl").is_file() and not lamp.exists()
    await pg.wait_for_selector(".card:has(.card-name:text-is('Lamp'))")
    await pg.click(".toast .toast-action")
    await toast_text(pg, "^Undone")
    back = (lamp / "lamp.stl").is_file() and not (library / "Household/Kitchen/Lamp").exists()
    check("files dropped on a category page are added there as a model after asking, and Undo puts them back",
          "Household › Kitchen" in hint and head == "Lamp (folder)" and menu[0].startswith("Add to Household › Kitchen as a model") and any(x.startswith("Sort it on Import") for x in menu)
          and said.startswith("Moved Lamp into Household › Kitchen") and moved and back, (hint, head, menu, said, moved, back))

    # 55. or sorted on Import, to choose the models first
    await drop(pg, src)
    await pg.click('#context-menu [data-action="drop-sort"]')
    await pg.wait_for_selector("#sort-page .sw-main", timeout=30000)
    await pg.wait_for_function("() => (window.__modlib && document.querySelector('#sort-page')?.textContent || '').includes('Makers')", timeout=30000)
    on_import = await pg.evaluate("() => location.hash")
    await drop(pg, lamp)
    import_menu = await labels_of(pg, "#context-menu .menu-item .menu-label")
    await pg.keyboard.press("Escape")
    await start_again(pg)
    check("a dropped folder can be sorted on Import, and Import's own drop menu adds or sorts", on_import == "#/import"
          and import_menu[0].startswith("Add as a model") and import_menu[1].startswith("Sort what's in it"), (on_import, import_menu))


async def model_workspace_extract(pg):
    """Package E: real dialog, core preview/job, and both message actions."""
    import zipfile
    source = library / "Unsorted/Workspace extraction"
    put(source / "Presupported/Helmet/helmet.stl", cube(7))
    put(source / "Unsupported/Helmet/helmet.stl", cube(8))
    put(source / "body.stl", cube(12))
    put(source / "photo.png", PNG)
    put(source / "model.json", json.dumps({"id": "workspace-extract", "name": "Workspace extraction",
                                          "authors": [{"name": "Test maker"}], "tags": ["fixture"], "cover": "photo.png"}))
    with zipfile.ZipFile(source / "extras.zip", "w") as z:
        z.writestr("Parts/clip.stl", cube(3))
    await api(pg, "library_scan", {"full": True})
    await pg.goto(B + "#/model/workspace-extract")
    await pg.wait_for_selector('#model-page[data-model="workspace-extract"]')
    before = {str(p.relative_to(source)): p.read_bytes() for p in source.rglob("*") if p.is_file() and "_thumbs" not in p.parts}

    async def select(keys):
        await pg.evaluate("""async keys => {
            const {fileSel} = await import('/ui/filesel.js');
            fileSel.set({picked: keys, anchor: keys[0] || null});
        }""", keys)
        await pg.wait_for_selector("#extract-model")

    async def make(name):
        await pg.fill("#extract-name", name)
        await pg.wait_for_function("() => document.querySelector('#extract-destination')?.textContent.includes(document.querySelector('#extract-name').value)")
        destination = (await pg.inner_text("#extract-destination")).rstrip("/")
        await pg.click('#extract-dialog button[type=submit]')
        await pg.wait_for_selector("#extract-dialog", state="detached")
        await pg.wait_for_function("() => document.querySelector('#toast')?.textContent.startsWith('Made ')")
        return library / destination

    async def undo():
        await pg.locator('#toast button:text-is("Undo")').click()
        await pg.wait_for_function("() => document.querySelector('#toast')?.textContent.includes('Undone')")

    await select(["d:Presupported/Helmet", "d:Unsupported/Helmet"])
    await pg.click("#extract-model")
    dest = await make("Helmet")
    moved = (dest / "Presupported/Helmet/helmet.stl").exists() and (dest / "Unsupported/Helmet/helmet.stl").exists()
    buttons = await pg.locator("#toast button").all_text_contents()
    on_source = await pg.get_attribute("#model-page", "data-model") == "workspace-extract"
    check("selected variant folders become a new model beside the source, with Undo and Open", moved and not (source / "Presupported/Helmet/helmet.stl").exists()
          and buttons == ["Undo", "Open"] and on_source, (moved, buttons, on_source))
    await undo()
    after = {str(p.relative_to(source)): p.read_bytes() for p in source.rglob("*") if p.is_file() and "_thumbs" not in p.parts}
    check("extraction Undo restores source files and sidecar exactly", before == after and not dest.exists())

    await select(["f:body.stl"])
    await pg.click("#extract-model")
    await pg.locator('input[name="extract-where"]').nth(1).check()
    overview = await api(pg, "library_overview")
    category = next(s for s in overview["schemas"] if s["name"] == "Household")
    await pg.select_option("#extract-schema", category["id"])
    await pg.select_option("#extract-mode", "copy")
    copy_dest = await make("Copied body")
    destination = copy_dest / "body.stl"
    copied = destination.exists() and (source / "body.stl").exists()
    await pg.locator('#toast button:text-is("Open")').click()
    await pg.wait_for_function("() => document.querySelector('#mp-name')?.textContent === 'Copied body'")
    check("extraction category picker and Copy keep the source; Open opens the new model", copied)
    await pg.keyboard.press("Control+z")
    await pg.wait_for_function("() => document.querySelector('#toast')?.textContent.includes('Undone')")
    await pg.goto(B + "#/model/workspace-extract")
    await pg.wait_for_selector('#model-page[data-model="workspace-extract"]')

    archive = (source / "extras.zip").read_bytes()
    await select(["z:extras.zip!Parts/clip.stl"])
    await pg.click("#extract-model")
    unpack_dest = await make("Unpacked clip")
    check("selected ZIP entries are unpacked and the ZIP stays unchanged", (unpack_dest / "clip.stl").exists()
          and (source / "extras.zip").read_bytes() == archive)
    await undo()
    model = await api(pg, "model_get", {"id": "workspace-extract"})
    await select(["f:" + f["rel"] for f in model["files_list"]])
    disabled = await pg.is_disabled("#extract-model")
    reason = await pg.get_attribute("#extract-model", "title")
    await pg.evaluate("""async () => {const {ui} = await import('/ui/state.js'); ui.set({library: {...ui.get().library, read_only: true}});} """)
    readonly = await pg.get_attribute("#extract-model", "title")
    check("whole-model and read-only extraction explain why they are unavailable", disabled and reason == "That's the whole model: use Move to category instead" and "read-only" in readonly, (reason, readonly))
    await pg.evaluate("""async () => {const {ui} = await import('/ui/state.js'); ui.set({library: {...ui.get().library, read_only: false}});} """)


async def tree_items(pg):
    """The models in Import's Folders view (everything unfolded)."""
    await pg.click('[data-view="folders"]')
    await pg.click("#sort-unfold")
    return await pg.eval_on_selector_all("#sort-tree .sw-item", "els => els.map(e => e.dataset.name)")


async def pick_rows_tree(pg, name):
    """Select one model in Import's Folders view."""
    await pg.click('[data-view="folders"]')
    await pg.click("#sort-unfold")
    await pg.click(f'#sort-tree .sw-item[data-name="{name}"] .sw-name')
    await pg.wait_for_function("n => document.querySelector('#sort-details #details-name')?.value === n", arg=name)


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
            pg.on("dialog", lambda d: asyncio.ensure_future(d.accept()))
            pg.on("console", lambda m: errors.append(f"{m.text} ({m.location.get('url', '')})") if m.type == "error" and "net::ERR_" not in m.text and "status of 400" not in m.text else None)  # a file that can't be drawn says so on the page
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
            unsorted = await pg.inner_text(".page-head h1")
            active = await pg.inner_text(".sidebar .nav-link.active .nav-label")
            check("the menu's places open", labels == ["Home", "Import", "All models", "Unsorted", "Starred", "Duplicates", "Settings"] and unsorted == "Unsorted" and active == "Unsorted", (labels, unsorted, active))

            await phase1(pg)
            await phase2(pg)
            await phase3(pg)
            await workspace_viewing(pg)
            await phase4(pg)
            await phase5(pg)
            await ui_pass(pg)
            await ui_pass2(pg)
            await ui_pass3(pg)
            await import_follow_ups(pg)
            await loose_workspace_checks(pg, check, library, B, api, cube, PNG, out)
            await model_workspace_extract(pg)

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
