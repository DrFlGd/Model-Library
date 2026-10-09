"""Agent F acceptance: card typography, lazy compositions, cover reset and Undo.

Called by desktop_page.py against the real backend/Chromium; screenshots land
next to the existing acceptance evidence. No mocks or model-render shortcuts.
"""
import json


async def card_checks(pg, library, base, out, api, check, png, cube):
    def add(rel, files):
        root = library / rel
        for name, data in files.items():
            dest = root / name
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_bytes(data if isinstance(data, bytes) else data.encode())
        return root

    add("Unsorted/F card one", {"one.stl": cube(5), "instructions.xyz": "custom"})
    add("Unsorted/F card two", {"body.stl": cube(7), "angle.png": png})
    add("Unsorted/F card five", {f"part{i}.stl": cube(5 + i) for i in range(5)})
    add("Unsorted/F card unknown", {"instructions.xyz": "custom", "archive.7z": "unknown"})
    chosen = add("Unsorted/F card chosen", {
        "chosen.png": png, "alternative.png": png, "body.stl": cube(8),
        "model.json": json.dumps({"id": "agent-f-chosen", "name": "F card chosen",
                                  "cover": "chosen.png", "authors": [{"name": "A maker"}]})
    })
    add("Unsorted/F card missing", {
        "alternative.png": png, "body.stl": cube(9),
        "model.json": json.dumps({"id": "agent-f-missing", "name": "F card missing",
                                  "cover": "deleted.png"})
    })
    add("Tabletop/Warhammer 40k/Tyranid/F card long", {
        "body.stl": cube(10),
        "model.json": json.dumps({"id": "agent-f-long", "name": "A very long named model for card testing",
                                  "authors": [{"name": "Extremely long credited author with full details and a suffix"}]})
    })
    for i in range(60):
        add(f"Unsorted/F scrolling {i:02d}", {"body.stl": cube(5)})

    await api(pg, "library_scan", {"full": True})
    models = (await api(pg, "models_query", {"scope": "all", "limit": 2000}))["items"]
    by_name = {m["name"]: m for m in models}
    shapes = {name: by_name[name]["files"] for name in
              ("F card one", "F card two", "F card five", "F card unknown", "F card chosen", "F card missing")}
    check("Agent F preview index is deterministic for one, two, five and unknown files",
          (shapes["F card one"]["previewable"] == 1
           and len(shapes["F card one"]["previews"]) == 1
           and shapes["F card two"]["previewable"] == 2
           and len(shapes["F card two"]["previews"]) == 2
           and shapes["F card five"]["previewable"] == 5
           and len(shapes["F card five"]["previews"]) == 4
           and shapes["F card unknown"]["previewable"] == 0
           and shapes["F card missing"]["cover_missing"]
           and shapes["F card chosen"]["explicit_cover"] == "chosen.png"), shapes)
    await pg.goto(base + "#/browse/all")
    await pg.reload()
    await pg.evaluate("""async () => {
      const {ui, setPref} = await import('/ui/state.js');
      ui.set({q: ''});
      setPref({layout: 'grid', sort: 'name'});
    }""")
    await pg.wait_for_selector('.card:has(.card-name:text-is("F card five"))')
    one = pg.locator('.card:has(.card-name:text-is("F card one"))')
    two = pg.locator('.card:has(.card-name:text-is("F card two"))')
    five = pg.locator('.card:has(.card-name:text-is("F card five"))')
    unknown = pg.locator('.card:has(.card-name:text-is("F card unknown"))')
    missing = pg.locator('.card:has(.card-name:text-is("F card missing"))')
    check("Agent F mixed and fallback cards show all represented file kinds",
          await one.locator(".cover-tiles-2 .cover-kind").count() == 1
          and await two.locator(".cover-tiles-2 .cover-file").count() == 2
          and await five.locator(".cover-tiles-4 .cover-file").count() == 4
          and await five.locator(".cover-count").inner_text() == "+1"
          and await unknown.locator(".cover-kind").count() >= 1
          and await missing.locator(".cover-warning").inner_text() == "Cover missing")

    long = pg.locator('.card:has(.card-name:text-is("A very long named model for card testing"))')
    await long.scroll_into_view_if_needed()
    typography = await long.evaluate("""el => {
      const name = el.querySelector('.card-name');
      const author = el.querySelector('.card-author');
      const place = el.querySelector('.card-location');
      return {font: getComputedStyle(author).fontStyle,
              weight: getComputedStyle(place).fontWeight,
              nameSize: parseFloat(getComputedStyle(name).fontSize),
              placeSize: parseFloat(getComputedStyle(place).fontSize),
              tooltip: author.title,
              crumbs: place.title};
    }""")
    check("Agent F author is italic and subcategory is a smaller upright breadcrumb",
          typography["font"] == "italic" and int(typography["weight"]) >= 500
          and typography["nameSize"] > typography["placeSize"]
          and typography["tooltip"].startswith("Extremely long")
          and typography["crumbs"].endswith("Tyranid"), typography)
    await pg.evaluate("document.documentElement.dataset.theme = 'light'")
    await pg.screenshot(path=str(out / "agent-f-card-grid-light.png"))
    await pg.evaluate("document.documentElement.dataset.theme = 'dark'")
    await pg.screenshot(path=str(out / "agent-f-card-grid-dark.png"))
    await pg.evaluate("document.documentElement.dataset.theme = 'light'")

    await pg.goto(base + "#/browse/unsorted")
    await pg.wait_for_selector('.card:has(.card-name:text-is("F card chosen"))')
    selected = pg.locator('.card:has(.card-name:text-is("F card chosen"))')
    await selected.click()
    await pg.wait_for_selector("#model-details")
    check("Agent F explicit cover overrides the automatic composition",
          await selected.locator(".thumb > img").count() == 1
          and await selected.locator(".cover-composed").count() == 0)
    # Adding another file does not change the explicit override.
    (chosen / "later.stl").write_bytes(cube(12))
    await api(pg, "library_scan", {"full": True})
    still = await api(pg, "model_get", {"id": "agent-f-chosen"})
    check("Agent F a file addition preserves the chosen cover",
          still["files"]["explicit_cover"] == "chosen.png"
          and still["files"]["previewable"] == 4)
    await pg.reload()
    selected = pg.locator('.card:has(.card-name:text-is("F card chosen"))')
    await selected.click()
    await pg.wait_for_selector("#details-more")
    await pg.click("#details-more")
    await pg.click('#context-menu [data-action="auto-cover"]')
    await pg.wait_for_function("() => document.querySelector('#toast')?.textContent.includes('automatic preview')")
    # The reset is journalled, and the chosen picture remains on disk.
    model = await api(pg, "model_get", {"id": "agent-f-chosen"})
    check("Agent F Use automatic preview removes only the metadata override",
          model["files"]["explicit_cover"] is None and (chosen / "chosen.png").is_file())
    await pg.locator('#toast button:text-is("Undo")').click()
    await pg.wait_for_function("() => document.querySelector('#toast')?.textContent.includes('Undone')", timeout=60000)
    restored = await api(pg, "model_get", {"id": "agent-f-chosen"})
    check("Agent F Undo restores the explicit cover", restored["files"]["explicit_cover"] == "chosen.png")
    await pg.screenshot(path=str(out / "agent-f-cover-restored.png"))

    # There are many cards but only the tiles actually visible should ask for
    # heavy mesh renders. Scrolling loads further previews on demand.
    await pg.goto(base + "#/browse/unsorted")
    await pg.wait_for_selector('.card:has(.card-name:text-is("F scrolling 59"))')
    total_cards = await pg.locator(".results .card").count()
    loaded_images = await pg.locator(".results .cover-file img").count()
    check("Agent F large grids defer off-screen mesh preview decoding",
          total_cards >= 60 and loaded_images < total_cards // 2,
          (total_cards, loaded_images))
