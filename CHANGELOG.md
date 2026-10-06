# Changelog

What changed in each release of the Model Library desktop app, and why.

It's written for whoever picks the work up next, people or agents: start with **For agents** below, then read the newest release. Newest first.

**Keeping it up to date:** add a line under **Unreleased** with each change you push (what changed for the user, and anything the next person needs to know). When CI publishes a release, move those lines under a heading for that version. CI writes the GitHub release notes from the commit subjects; this file is where the reasons and lessons go. Design detail belongs in docs/PLAN.md; link to it rather than repeating it.

---

## For agents

### What this is

A desktop app (Windows, Linux; Tauri 2) for organising thousands of 3D model files into a folder library laid out by user-defined schemas, with each model's details in a `model.json` beside its files. A Docker build will follow, serving the same backend. The owner ("DrFlGd", Jerred) steers by phases; the plan, the decisions taken and the phases are in `docs/PLAN.md`.

The code started as a trimmed copy of [Claude Grid Workshop](https://github.com/DrFlGd/Claude-Grid-Workshop) (same owner): its Tauri shell, its one-command-table backend, its library-folder code and its Preact interface. Much more of it is planned to come across (layered metadata `meta.rs`, documents `docs.rs`, library merge, the browser views, the metadata editor, the side viewer, the three.js viewer): look there before writing something from scratch. `docs/PLAN.md` section 2 lists what maps to what.

### Where things are

| Path | What |
| --- | --- |
| `web/` | The front end (no build step). `app.js`: start-up and routing. `web/ui/`: Preact + htm islands (`shell.js` mounts them; `sidebar.js` with the schema trees, `home.js` with the first-start screen, `browser.js` for the model grid/list and search, `details.js` for the selected model, `dialogs.js` for New category, Edit details and Move, `sort.js` for the Import page (the sorting workspace), `parts.js` for a model's files (the parts views, the 3D stage, pictures, documents, videos; shared by the model page and the workspace), `dupes.js` for the Duplicates page, `categories.js` for renaming, merging and editing categories and editing several models, `modelpage.js` for a model's own page (its head; the rest is `parts.js`), `category.js` for the category picker and the subcategory tree editor, `settings.js`, `chrome.js` for the top bar and status bar, `library.js` for library and model actions, `state.js` for shared state and preferences). `viewer.js`: the three.js viewer (three.js in `web/vendor/three/`, loaded only on a model's page). `platform.js` / `platform-desktop.js`: the seam between the page and the backend (desktop now, a server later). |
| `desktop/core/` | Rust, no GUI: `api.rs` (the app's command table, `App::call`), `library.rs` (the library folder: `_library/library.json`, format number, read-only for newer formats), `schema.rs` (schema files, each category's subcategory tree, folder-name templates, converting older fixed-level schemas), `model.rs` (a model folder: its files, kinds, cover, `model.json`), `index.rs` (finding models, the in-memory index and its cache on this computer, search), `import.rs` (proposing models from folders, destinations, moving or copying with SHA-256 checks, moving between categories), `mesh.rs` (reading STL, OBJ and 3MF into triangles; binary STL for the viewer), `archive.rs` (listing and reading ZIP entries), `thumb.rs` (choosing a model's main 3D file and drawing its preview, a small software renderer, PNG out), `docs.rs` (readmes as safe HTML), `relayout.rs` (category changes that move folders: plan, journal in `_library/journal/`, apply, undo), `sort.rs` (the sorting workspace: a folder tree read as it is, items sent, grouped, joined and split, kept in the data folder), `dupes.rs` (duplicates by fingerprint and SHA-256, hashes in `model.json`, setting copies aside), `watch.rs` (the library folder watcher), `config.rs` (what stays on this computer). `src/bin/modlib-cli.rs`: `library-info`, `library-scan`, `make-test-library` and `serve` (the backend over HTTP, for tests now and the Docker build later). |
| `desktop/src-tauri/` | The Tauri app: a thin layer over `api.rs`, plus dialogs, the file manager and the `library://` protocol. Version in `tauri.conf.json`. |
| `tools/` | `build_desktop.py` (web/ to `build/desktop/ui`, fonts), `set_version.py`, `vendor_preact.py`. |
| `tests/` | `desktop_page.py` (acceptance test: the page + the real backend through `modlib-cli serve` and `tauri_shim.js`, in Chromium), `desktop_ui.py` (WebDriver on the built app). |
| `.github/workflows/` | `desktop.yml` (Linux and Windows apps, tests, portability, release), `vendor.yml` (vendored crates for offline builds). |

### Conventions

- **Phases and releases.** The repository keeps `0.<phase>.0` in `desktop/src-tauri/tauri.conf.json` and `desktop/Cargo.toml` (`tools/set_version.py 0.1.0` stamps them). CI picks the next free `0.<phase>.<n>` from the release tags and publishes `v<version>` only when the Linux and Windows apps, both tests and the portability check pass. Start a new phase by bumping the minor number.
- **Plan first.** For a new phase or a big feature, the owner wants the design written into `docs/PLAN.md` before code, and a notes section ("Phase N notes") afterwards recording what was built and where it differs.
- **Commits:** a subject saying what changed for the user, a body with the details.
- **Text:** plain words, no jargon in the interface; British spellings in the UI (colour, favourites, organise), as in Grid Workshop.
- **Tests grow with each phase.** Add checks to `desktop_page.py` for every user-visible feature, and to `desktop_ui.py` when it needs the real app.
- **The library folder is the source of truth** and must make sense without the app. Never write a database into it; keep unknown keys when rewriting its JSON; write JSON with `config::write_json` (sorted keys, so synced or versioned libraries diff cleanly).

### Working in a cloud session

The sandbox has a network allowlist and is reset between sessions.

- **Rust:** crates.io is blocked. `vendor.yml` pushes `desktop/core`'s dependencies to the `crates-vendor` branch whenever `desktop/core/Cargo.toml` changes. Clone that branch, then build offline: a workspace with `core` linked to `desktop/core`, `Cargo.lock` from the branch, and `cargo <cmd> --offline --config <vendor-config.toml with directory = the branch's vendor/>`. Until this repository's branch exists, Grid Workshop's `crates-vendor` branch has a superset of today's dependencies and works the same way. New dependencies: push the `Cargo.toml` change and wait for `vendor.yml`.
- **The Tauri app can't be built locally** (no Tauri crates; npm's Tauri CLI is blocked). Test the backend with `modlib-cli serve` + `tests/tauri_shim.js` (what `desktop_page.py` does); the real app only builds in CI.
- **Acceptance test locally:** `python3 tools/build_desktop.py --out build/desktop`, then `python3 tests/desktop_page.py --cli <modlib-cli> --ui build/desktop/ui --home <tmp> --out <shots> --chromium /opt/pw-browsers/chromium` (Playwright's own browser download is blocked; the pre-installed Chromium works).
- **GitHub from the session:** `gh` works through the REST API. CI reports land on the `desktop-ci-linux` and `desktop-ci-windows` branches (logs, JSON reports, screenshots); clone those to read results.

### Known limitations now

- ZIPs are read in place (listed, and their 3D files and pictures shown) but never unpacked; 7z and RAR are kept as archives and not opened. Only the newest category change can be undone; a model folder can't be renamed from the app except through a category's model-folder template.
- Duplicates are judged by 3D, slicer and archive files only (pictures and documents can differ). The Duplicates page shows the last search; it isn't run by itself. Only the newest set-aside can be undone (like category changes).
- Changes made outside the app are noticed by a watcher on the library folder (2 s after they settle) and by a check when the window comes back to the front and every 5 minutes. A model folder moved by hand keeps its id only if it has a `model.json`; one without gets a new id from its new path.
- The index lives in memory and is cached per computer (`<data dir>/index/<library id>.json`); 10,000 models take about 1 s to read the first time and under a second after.
- Previews are drawn by the core's own renderer (`thumb.rs`, flat shaded, no textures or colours from 3MF). Models whose 3D file can't be read get no preview and are tried again by every *Make previews*. G-code and other slicer files aren't shown in 3D.
- `Cargo.lock` isn't committed yet: the first CI run makes it (the Tauri crates can't be resolved in a cloud session). Commit `ci-out/Cargo.lock` from the `desktop-ci-linux` branch once it exists.
- The `library://` protocol and `serve` answer range requests (8 MB at a time when the end is open), so videos seek; a whole-file read still loads it into memory. 3D files over 500 MB aren't shown.
- The app icons are Grid Workshop's; the interface has its own mark (a stack of layers). New icons are to do.
- Installers aren't code-signed, and there's no auto-update.

### Next (roadmap)

Phase 6 in `docs/PLAN.md`: the Docker build with sign-in. The owner is trying Phase 5 (the sorting workspace) on a real NAS share first.

---

## Unreleased

Phase 5: large collections (design in docs/PLAN.md, "Phase 5 design"; notes in "Phase 5 notes").

- **The Import page is a sorting workspace** (owner's request): *Sort a folder…* reads a whole folder tree as it is on disk and proposes which folders and files are models, all the way down. Four views: *Folders* (the tree as on disk), *List* (sortable), *Grid* (a preview of each model) and *By category* (grouped by where each will go). Pick one or many (click, Ctrl, Shift, ticks; a picked folder takes everything in it) and *Send to* a category and subcategory, or Unsorted; for a folder, *Keep its folders as subcategories* brings a tidy tree in as it is. *Group into one model* makes picked models, folders and loose files one model; *Make one model* turns a folder read as several into one; *Split* and *Ungroup* undo that. *Import N sorted models* moves (or copies) what has a place; the rest waits. The workspace is kept on this computer, one per library, and comes back next time; *Read again* picks up changes and keeps what was decided.
- **A model's files in several views** (model page and the workspace's details pane): *Folders* (the part tree with the variant switch), *All files* (one list with a filter, sorted by folder, name, size or kind), *By type* and *Grid* (previews of every 3D file and picture, drawn by the core and cached in the data folder). The choice is remembered.
- **Duplicates** (new page in the menu): *Look for duplicates* compares files of the same size by a quick fingerprint, then SHA-256, and lists models whose 3D, slicer and archive files are all the same as another's, and files in more than one model. Hashes are kept in each model's `model.json` (`hashes`), so the next look only reads what changed. *Set aside* moves extra copies to `_library/set-aside/` (journalled: undo on the page or on Home); *Delete set-aside copies…* deletes them after asking. Import's duplicate warning now compares fingerprints too, not just sizes.
- **Changes made outside the app show up by themselves**: the library folder is watched, and checked again when the window comes back to the front and every 5 minutes. A model folder moved by hand keeps its id, star and details, and its `model.json` is told its new place.
- Tests: the workspace test built a path with a forward slash, so it failed on Windows (the app itself takes paths from the folder walk, which uses the system's separator).
- Tests: the page test now waits for a grouped model's details before renaming it (it raced on a slow runner).

## 0.4.2 (2026-10-06)

- **Subcategories are a tree you build in the category dialogs** (owner's request, replacing 0.4.1's fixed levels): **New category** and **Edit category…** have a subcategory editor. Add as many as you like, at the top or inside any other, each branch as deep as it needs (Home items › Office › Desk items; Home items › Kitchen). Enter adds the next one beside it; ← and → move one out a level or into the one above. In **Edit category…**, renaming, moving or removing one moves its folders, with the preview and undo from Phase 4; a removed one's models move up to the one above it. Models can sit at any level, including the top of a category.
- **Choosing a place** (Import, Move to category): the category, then any of its subcategories from a list, and optionally a new one to make inside it ("/" makes several levels).
- **Rename or move…** takes a new name and a new place anywhere in the tree (a name that's there already merges the two). **Add subcategory…** works at any depth.
- Categories from earlier versions (with named levels) become trees when their library is opened; nothing moves. `model.json` records `path` (the list of subcategory names) instead of `category`; older files are still read when importing. Search: `in:office` finds models with Office anywhere in their path (the level filters, such as `faction:`, went with the levels).
- A subcategory can't be made inside a model's folder, and a folder made by hand under a category is read as a subcategory when it holds model folders, as a model when it holds files, and skipped when it's empty (design in docs/PLAN.md, "Subcategory tree design").

## 0.4.0 and 0.4.1 (2026-10-06)

- 0.4.1: **Subcategories made in the app** (owner's request): **Add <level>…** on a category's or subcategory's page added one below it, kept in the category file (`subcategories`) with its folder, so it showed with no models in it; **Remove** took away an empty one. The levels themselves stayed fixed, which wasn't what was asked: replaced by the subcategory tree (Unreleased).
- 0.4.1: **General wording** (owner's request): examples in the interface no longer refer to wargaming (the New category dialog, Home's folder example, placeholders).

Phase 4 (0.4.0): editing categories (design in docs/PLAN.md, "Phase 4 design"; notes in "Phase 4 notes").

- **Rename or move…** on a category's page (Warhammer 40k › Tyranid): rename it, merge it into one that's already there by giving that name, or move it under another value. A preview shows every folder that moves, and any that would clash and get a number.
- **Edit category…** on a category's own page: its name, top folder, levels (rename a label, add one with a value for the models already there, remove or reorder), the model folder name (optionally renaming existing folders to match) and its fields. **Delete this category…** moves its models to Unsorted.
- Every change that moves folders is written to a journal in `_library/journal/` first. **Recent changes** on Home lists them; the newest can be **undone** (folders, model.json categories and the category file all go back). A change that stopped partway can be finished or put back from Home.
- **Edit details…** for several picked models: add or remove tags, and set authors, licence or the category's fields for all of them.
- Previews made by *Make previews* and details edited for several models are read back into the index straight away. Windows doesn't always change a folder's time when a file inside it changes, so the cache missed them (found by the Windows CI run).

## 0.3.0 and 0.3.1 (2026-10-06)

- 0.3.1: **Variant folder names are set in Settings** (owner's request), kept in the library (`library.json` `variant_folders`). The defaults add Sized, Split, FDM and Resin to Presupported, Supported, Unsupported and No supports. A folder is a variant when its name is one of them or holds one as whole words ("Resin 32mm").

0.3.0, Phase 3 (0.3): viewing models (design in docs/PLAN.md, "Phase 3 design"; notes in "Phase 3 notes").

- **A model's own page**: double-click a model, press Enter, or use **Open** in its details. A large 3D view (STL, OBJ, 3MF; turn, zoom, pan; 3/4, top and front views; edges; size in mm and triangle count), with tabs for its **Pictures**, **Documents** and **Videos**.
- **Parts as a tree**: the model's files keep their folders (Helmet, Arms…); click a part to show it. Folders named Presupported, Supported, Unsupported and the like are **variants**: a switch shows one variant's parts (supported first) or all.
- **ZIPs open in place**: a ZIP in the tree unfolds into its entries, and its 3D files and pictures are shown straight from the archive, never unpacked.
- **Previews**: importing draws `_thumbs/model.png` from the model's main 3D file (supported variant first, then the shallowest, then the largest), used as the card picture when the model has no picture of its own. **Make previews** on Home draws the missing ones for models added by hand. **Use as cover** saves the 3D view or a picture as the model's cover.
- **Readmes** (Markdown, text) are shown formatted; raw HTML shows as text and only web links are kept, opening in the browser. PDFs show in the page; other documents open in their own app.
- **Videos** (MP4, WebM, M4V, MOV) play in the page and seek: library files are served in ranges.
- `modlib-cli thumb --model <folder>` draws one model's preview.

## 0.2.0 (2026-10-06)

Phase 2 (0.2): importing, and moving models between categories (design in docs/PLAN.md, "Phase 2 design"; notes in "Phase 2 notes").

- **Import** in the menu: *Sort a folder…* proposes a model for each sub-folder and each loose model file or archive (pictures and documents named like it go with it); *Add a model folder…*, *Add a file…*, or dropping folders and files on the window adds each as one model.
- Each proposal shows its files and warnings (*maybe several models*, with **Split**; *looks like a model already in the library*; *already in the library*), with its name and author read from the folder name or its own `model.json`, and a category guessed from folder names the library already uses.
- Give each a category and its levels (values already in the library are suggested), author and tags; tick several and use **Set for picked**. The destination folder is shown as you type.
- **Move** (default) or **Copy**. Moves on the same drive are renames; otherwise every file is copied, checked by SHA-256, and only then is the original deleted. Each imported model gets a `model.json` (keeping the one it brought). Progress shows with a Stop button; a stopped or failed model stays where it was.
- **Move to category…** on a model (the folder button in its details), or on several picked with Ctrl or Shift click. Folders keep their names; emptied category folders are removed; stars follow.
- **Home** lists folders in the library that aren't sorted yet, each with **Sort…**.
- The menu says **Categories** and **New category…** (owner's request); code and files keep "schema".

## 0.1.0 and 0.1.1 (2026-10-06)

0.1.1 renamed schemas to **categories** in the interface (owner's request).

Phase 1 (0.1): schemas, model details and search (design in docs/PLAN.md, "Phase 1 design"; notes in "Phase 1 notes").

- **The first start asks where the library goes** (owner's note, 2026-10-06): use the suggested `~/Model Library` or choose any folder. The library stays an ordinary folder you can move and open again.
- **New schema…** in the menu makes a schema: its name, top folder, levels (Game, Faction…), the model folder name (`{name} ({author})`) and its own fields (text, number, choice, yes or no, date), with a preview of where models will go.
- **Models are found in the folders**: under a schema's folder, the folders at the bottom level are models and the path gives their category; each folder in `Unsorted/` is a model. Folders without a `model.json` are listed with their name and author read from the folder name.
- **Browsing**: each schema's categories are a tree in the menu with counts; All models, Unsorted and Favourites list theirs; a grid or a list, sorted by name, newest or size, with author and tag chips to narrow down.
- **Search** in each place: words match the start of names, authors, tags, categories and fields, accents ignored; `author:`, `tag:`, `schema:`, a level (`faction:tyranid`) or a field (`scale:32mm`) filter, with quotes for spaces.
- **Model details** beside the list: cover, authors, category, source, the schema's fields, tags, notes, and the files (parts keep their sub-folders) by kind. **Edit details…** writes `model.json` (keeping keys it doesn't know); a model's first edit or star gives it an id. **Star** keeps favourites in the library.
- `modlib-cli library-scan` and `make-test-library` time a generated 10,000-model library; the acceptance test checks it opens in seconds and searches in under 100 ms.

## 0.0.0 (2026-10-06)

Phase 0 (0.0): the new repository, seeded from Claude Grid Workshop (owner's decision, 2026-10-06).

- **The app opens a library folder**: `~/Model Library` on first start, or any folder chosen with *Open another library…*. It adds only `_library/` (library.json, schemas/, a README) and `Unsorted/`; a folder that already holds models is otherwise left as it is. A library made by a newer version opens read-only.
- **Home** shows the library, how its folders will be laid out, and what's coming; **All models**, **Unsorted** and **Favourites** are in the menu, empty until importing arrives.
- **Settings**: rename the library, show it in the file manager, open another or one opened before, and choose the theme (light, dark, night, or follow the system).
- Kept from Grid Workshop: the Tauri shell and `library://` protocol, the command table and `serve`, the library format number and read-only rule, the preferences store, the themes, the sidebar and status bar, the tests' harness and CI. Left behind: OpenSCAD, rendering, components, the catalog and the website build.
