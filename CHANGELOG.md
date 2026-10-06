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
| `web/` | The front end (no build step). `app.js`: start-up and routing. `web/ui/`: Preact + htm islands (`shell.js` mounts them; `sidebar.js` with the schema trees, `home.js` with the first-start screen, `browser.js` for the model grid/list and search, `details.js` for the selected model, `dialogs.js` for New schema and Edit details, `settings.js`, `chrome.js` for the top bar and status bar, `library.js` for library and model actions, `state.js` for shared state and preferences). `platform.js` / `platform-desktop.js`: the seam between the page and the backend (desktop now, a server later). |
| `desktop/core/` | Rust, no GUI: `api.rs` (the app's command table, `App::call`), `library.rs` (the library folder: `_library/library.json`, format number, read-only for newer formats), `schema.rs` (schema files, folder-name templates), `model.rs` (a model folder: its files, kinds, cover, `model.json`), `index.rs` (finding models, the in-memory index and its cache on this computer, search), `config.rs` (what stays on this computer). `src/bin/modlib-cli.rs`: `library-info`, `library-scan`, `make-test-library` and `serve` (the backend over HTTP, for tests now and the Docker build later). |
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

- No importing yet (Phase 2): models are folders put into the library by hand, then *Read the folders again* on Home. Schemas can be made but not changed (Phase 4).
- The index lives in memory and is cached per computer (`<data dir>/index/<library id>.json`); 10,000 models take about 1 s to read the first time and under a second after. Thumbnails aren't generated yet: covers are pictures already in the folder.
- `Cargo.lock` isn't committed yet: the first CI run makes it (the Tauri crates can't be resolved in a cloud session). Commit `ci-out/Cargo.lock` from the `desktop-ci-linux` branch once it exists.
- The `library://` protocol reads whole files into memory and has no range requests; videos and large STLs need that (Phase 3).
- The app icons are Grid Workshop's; the interface has its own mark (a stack of layers). New icons are to do.
- Installers aren't code-signed, and there's no auto-update.

### Next (roadmap)

Phase 2 in `docs/PLAN.md`: the import wizard (files, folders and ZIPs; moved by default, or copied), placing models by schema, and thumbnails.

---

## Unreleased

Phase 1 (0.1): schemas, model details and search (design in docs/PLAN.md, "Phase 1 design"; notes in "Phase 1 notes").

- **The first start asks where the library goes** (owner's note, 2026-10-06): use the suggested `~/Model Library` or choose any folder. The library stays an ordinary folder you can move and open again.
- **New schema…** in the menu makes a schema: its name, top folder, levels (Game, Faction…), the model folder name (`{name} ({author})`) and its own fields (text, number, choice, yes or no, date), with a preview of where models will go.
- **Models are found in the folders**: under a schema's folder, the folders at the bottom level are models and the path gives their category; each folder in `Unsorted/` is a model. Folders without a `model.json` are listed with their name and author read from the folder name.
- **Browsing**: each schema's categories are a tree in the menu with counts; All models, Unsorted and Favourites list theirs; a grid or a list, sorted by name, newest or size, with author and tag chips to narrow down.
- **Search** in each place: words match the start of names, authors, tags, categories and fields, accents ignored; `author:`, `tag:`, `schema:`, a level (`faction:tyranid`) or a field (`scale:32mm`) filter, with quotes for spaces.
- **Model details** beside the list: cover, authors, category, source, the schema's fields, tags, notes, and the files (parts keep their sub-folders) by kind. **Edit details…** writes `model.json` (keeping keys it doesn't know); a model's first edit or star gives it an id. **Star** keeps favourites in the library.
- The search test waits for the results of its own search (CI was slower than the debounce).
- `modlib-cli library-scan` and `make-test-library` time a generated 10,000-model library; the acceptance test checks it opens in seconds and searches in under 100 ms.

## 0.0.0 (2026-10-06)

Phase 0 (0.0): the new repository, seeded from Claude Grid Workshop (owner's decision, 2026-10-06).

- **The app opens a library folder**: `~/Model Library` on first start, or any folder chosen with *Open another library…*. It adds only `_library/` (library.json, schemas/, a README) and `Unsorted/`; a folder that already holds models is otherwise left as it is. A library made by a newer version opens read-only.
- **Home** shows the library, how its folders will be laid out, and what's coming; **All models**, **Unsorted** and **Favourites** are in the menu, empty until importing arrives.
- **Settings**: rename the library, show it in the file manager, open another or one opened before, and choose the theme (light, dark, night, or follow the system).
- Kept from Grid Workshop: the Tauri shell and `library://` protocol, the command table and `serve`, the library format number and read-only rule, the preferences store, the themes, the sidebar and status bar, the tests' harness and CI. Left behind: OpenSCAD, rendering, components, the catalog and the website build.
