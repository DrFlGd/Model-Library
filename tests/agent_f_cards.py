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
    add("Unsorted/F card mixed kinds", {"part.stl": cube(6), "guide.pdf": "%PDF", "intro.mp4": "sample"})
    add("Unsorted/F card all kinds", {"guide.pdf": "%PDF", "archive.7z": "sample", "custom.xyz": "sample"})
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
              ("F card one", "F card two", "F card five", "F card unknown", "F card mixed kinds", "F card all kinds", "F card chosen", "F card missing")}
    check("Agent F preview index is deterministic for one, two, five and unknown files",
          (shapes["F card one"]["previewable"] == 1
           and len(shapes["F card one"]["previews"]) == 1
           and shapes["F card two"]["previewable"] == 2
           and len(shapes["F card two"]["previews"]) == 2
           and shapes["F card five"]["previewable"] == 5
           and len(shapes["F card five"]["previews"]) == 4
           and shapes["F card unknown"]["previewable"] == 0
           and shapes["F card mixed kinds"]["previewable"] == 1
           and shapes["F card all kinds"]["previewable"] == 0
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
    # Also probe the pure grouping logic: more kinds than available slots must
    # never mislabel their counts, and the overflow badge counts hidden files.
    grouping = await pg.evaluate("""async () => {
      const {automaticTiles} = await import('/ui/modelcover.js');
      return {
        none: automaticTiles({files:{count:0,kinds:{},previews:[]}}),
        mixed: automaticTiles({files:{count:3,kinds:{model:1,doc:1,video:1},
          previews:[{file:'part.stl',kind:'model'}]}}),
        full: automaticTiles({files:{count:6,kinds:{model:1,doc:1,video:1,archive:1,slicer:1,other:1},
          previews:[{file:'part.stl',kind:'model'}]}})
      };
    }""")
    check("Agent F grouping preserves each kind's actual counts and accurately reports overflow",
          grouping["none"]["more"] == 0
          and len(grouping["none"]["tiles"]) == 1
          and [(t["kind"], t["count"]) for t in grouping["mixed"]["tiles"] if t["type"] == "kind"]
             == [("doc", 1), ("video", 1)]
          and len(grouping["full"]["tiles"]) == 4
          and grouping["full"]["more"] == 2
          and all(t["count"] == 1 for t in grouping["full"]["tiles"] if t["type"] == "kind"),
          grouping)

    # Scroll first, then require *actual loaded* mesh and image previews, not
    # transient placeholders while the lazy render queue is still working.
    await two.scroll_into_view_if_needed()
    await pg.wait_for_function("""id => {
      const card = document.querySelector('[data-model="' + id + '"]');
      const images = [...(card?.querySelectorAll('.cover-file img') || [])];
      return images.length === 2 && images.every(img => img.complete && img.naturalWidth > 0);
    }""", arg=by_name["F card two"]["id"], timeout=60000)
    await five.scroll_into_view_if_needed()
    await pg.wait_for_function("""id => {
      const card = document.querySelector('[data-model="' + id + '"]');
      const imgs = [...(card?.querySelectorAll('.cover-file img') || [])];
      return imgs.length === 4 && imgs.every(img => img.complete && img.naturalWidth > 0);
    }""", arg=by_name["F card five"]["id"], timeout=60000)

    mixed = pg.locator('.card:has(.card-name:text-is("F card mixed kinds"))')
    all_kinds = pg.locator('.card:has(.card-name:text-is("F card all kinds"))')
    mixed_tiles = await mixed.locator(".cover-kind").all_text_contents()
    all_kind_tiles = await all_kinds.locator(".cover-kind").all_text_contents()
    check("Agent F mixed file types have accurate labels and counts",
          await one.locator(".cover-tiles-2 .cover-kind").count() == 1
          and await two.locator(".cover-tiles-2 .cover-file").count() == 2
          and await five.locator(".cover-tiles-4 .cover-file").count() == 4
          and await five.locator(".cover-count").inner_text() == "+1"
          and sorted(t.strip() for t in mixed_tiles) == ["Document", "Video"]
          and sorted(t.strip() for t in all_kind_tiles) == ["Archive", "Document", "File"]
          and await unknown.locator(".cover-kind").count() == 2
          and await missing.locator(".cover-warning").inner_text() == "Cover missing",
          (mixed_tiles, all_kind_tiles))

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
    await pg.evaluate("document.documentElement.style.fontSize = '125%'")
    await pg.screenshot(path=str(out / "agent-f-card-grid-large-text.png"))
    await pg.evaluate("document.documentElement.style.fontSize = ''")

    await pg.goto(base + "#/browse/unsorted")
    await pg.wait_for_selector('.card:has(.card-name:text-is("F card chosen"))')
    selected = pg.locator('.card:has(.card-name:text-is("F card chosen"))')
    await selected.click()
    await pg.wait_for_selector("#model-details")
    check("Agent F explicit cover overrides the automatic composition",
          await selected.locator(".thumb > img").count() == 1
          and await selected.locator(".cover-composed").count() == 0)
    # Adding another file does not change the explicit override.
    (chosen / "later.stl").write_text(cube(12))
    await api(pg, "library_scan", {"full": True})
    still = await api(pg, "model_get", {"id": "agent-f-chosen"})
    check("Agent F a file addition preserves the chosen cover",
          still["files"]["explicit_cover"] == "chosen.png"
          and still["files"]["previewable"] == 4)
    # An external removal must show a deterministic warning without discarding
    # the explicit choice. Returning the file restores the chosen image.
    moved_cover = chosen / "chosen.png.missing"
    (chosen / "chosen.png").rename(moved_cover)
    await api(pg, "library_scan", {"full": True})
    await pg.reload()
    selected = pg.locator('.card:has(.card-name:text-is("F card chosen"))')
    missing_source = await api(pg, "model_get", {"id": "agent-f-chosen"})
    check("Agent F a missing explicitly chosen cover shows an automatic fallback",
          missing_source["files"]["cover_missing"]
          and await selected.locator(".cover-composed .cover-warning").count() == 1)
    moved_cover.rename(chosen / "chosen.png")
    await api(pg, "library_scan", {"full": True})
    await pg.reload()
    selected = pg.locator('.card:has(.card-name:text-is("F card chosen"))')
    restored_source = await api(pg, "model_get", {"id": "agent-f-chosen"})
    check("Agent F restoring a missing source restores the user's chosen cover",
          restored_source["files"]["explicit_cover"] == "chosen.png"
          and not restored_source["files"]["cover_missing"]
          and await selected.locator(".thumb > img").count() == 1)
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

    # Many cards, but the last one cannot have triggered a heavy render until
    # we scroll it into view; after scrolling it must receive a real image.
    await pg.goto(base + "#/browse/unsorted")
    await pg.wait_for_selector('.card:has(.card-name:text-is("F scrolling 59"))')
    first_scroll = pg.locator('.card:has(.card-name:text-is("F scrolling 00"))')
    last_scroll = pg.locator('.card:has(.card-name:text-is("F scrolling 59"))')
    await first_scroll.scroll_into_view_if_needed()
    await pg.wait_for_function("""() => {
      const c = [...document.querySelectorAll('.card')].find(e => e.querySelector('.card-name')?.textContent === 'F scrolling 00');
      return [...(c?.querySelectorAll('.cover-file img') || [])].some(i => i.complete && i.naturalWidth > 0);
    }""", timeout=60000)
    before = await last_scroll.locator(".cover-file img").count()
    total_cards = await pg.locator(".results .card").count()
    await last_scroll.scroll_into_view_if_needed()
    await pg.wait_for_function("""() => {
      const c = [...document.querySelectorAll('.card')].find(e => e.querySelector('.card-name')?.textContent === 'F scrolling 59');
      return [...(c?.querySelectorAll('.cover-file img') || [])].some(i => i.complete && i.naturalWidth > 0);
    }""", timeout=60000)
    check("Agent F large grids lazily load off-screen meshes after scrolling",
          total_cards >= 60 and before == 0, (total_cards, before))
