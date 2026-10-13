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
| `web/` | The front end (no build step). `app.js`: start-up and routing. `web/ui/`: Preact + htm islands (`shell.js` mounts them; `sidebar.js` with the menu and the category panel, `caticons.js` for category icons (the built-in set and the user's own pictures), `home.js` for Home (recently added, waiting to be sorted, categories, Recent changes, the Library menu) and the first-start screen, `browser.js` for the model grid/list and search, `details.js` for the details panel (one model, several, or what the place holds), `layout.js` for the page header, menu button, view switcher, sort menu, search note and the panel's edit boxes, `dialogs.js` for New category, Edit details and Move, `sort.js` for the Import page (the sorting workspace), `parts.js` for a model's files (the parts views, the 3D stage, pictures, documents, videos; shared by the model page and the workspace), `dupes.js` for the Duplicates page, `drop.js` for files dropped on the window, `filetypes.js` for the file type tags, `actions.js` for the actions every page shares (the action row, the right-click menu, the keys), `categories.js` for renaming, merging and editing categories and editing several models, `modelpage.js` for a model's own page (its head and workspace wiring; `filepanel.js`, `filesel.js`, `contents.js` and `stage.js` handle navigation, shared selection and viewing, and `extract.js` handles Make a new model; `workspace-details.js` is the Details panel on the right, `unsorted-preview.js` previews an Unsorted model's files beside the collection, `document-viewer.js` shows PDFs (named so WebKitGTK doesn't sniff it as a PDF), `fileops.js` has Send to another model, Add files and Delete model, `archive-actions.js` has Compress to ZIP and Extract archive, `modelcover.js` draws the automatic card covers), `categoryimport.js` for Import folders as categories (the staging tree and its review), `category.js` for the category picker and the subcategory tree editor, `settings.js`, `about.js` for the About page (versions, the changelog read from `CHANGELOG.md` beside the page, licences), `chrome.js` for the top bar and status bar, `library.js` for library and model actions, `state.js` for shared state and preferences). `viewer.js`: the three.js viewer (three.js in `web/vendor/three/`, loaded only on a model's page). `platform.js` / `platform-desktop.js`: the seam between the page and the backend (desktop now, a server later). |
| `desktop/core/` | Rust, no GUI: `api.rs` (the app's command table, `App::call`), `library.rs` (the library folder: `_library/library.json`, format number, read-only for newer formats), `schema.rs` (schema files, each category's subcategory tree, folder-name templates, converting older fixed-level schemas), `model.rs` (a model folder: its files, kinds, cover, `model.json`), `index.rs` (finding models, the in-memory index and its cache on this computer, search), `extract.rs` (checked file and ZIP-entry extraction, journal and exact Undo), `import.rs` (proposing models from folders, destinations, moving or copying with SHA-256 checks, moving between categories), `mesh.rs` (reading STL, OBJ and 3MF into triangles; binary STL for the viewer), `archive.rs` (listing and reading ZIP entries), `thumb.rs` (choosing a model's main 3D file and drawing its preview, a small software renderer, PNG out), `docs.rs` (readmes as safe HTML), `relayout.rs` (category changes that move folders: plan, journal in `_library/journal/`, apply, undo; the journal and its retention rules are shared by the modules below), `restructure.rs` (removing a subcategory and merging categories: plans checked against the disk, carried out by relayout), `fileops.rs` (SHA-256-checked transfers into an existing model: Send to another model, Add files), `delete.rs` (Delete model: whole folders moved into the journal, restorable, never erased), `archive_ops.rs` (Compress to ZIP and Extract archive in a model, read back and hashed before publishing; removing the originals is a separate step), `category_import.rs` (Import folders as categories: read-only scan, reviewed plan, crash-safe commit and Undo), `sort.rs` (the sorting workspace: a folder tree read as it is, items sent, grouped, joined and split, kept in the data folder), `dupes.rs` (duplicates by fingerprint and SHA-256, hashes in `model.json`, setting copies aside), `watch.rs` (the library folder watcher), `config.rs` (what stays on this computer). `src/bin/modlib-cli.rs`: `library-info`, `library-scan`, `make-test-library` and `serve` (the backend over HTTP, for tests now and the Docker build later). |
| `desktop/src-tauri/` | The Tauri app: a thin layer over `api.rs`, plus dialogs, the file manager and the `library://` protocol. Version in `tauri.conf.json`. |
| `tools/` | `build_desktop.py` (web/ to `build/desktop/ui`, fonts, and `CHANGELOG.md`, `THIRD_PARTY.md` and `build-info.json` for About), `set_version.py`, `vendor_preact.py`. |
| `tests/` | `desktop_page.py` (acceptance test: the page + the real backend through `modlib-cli serve` and `tauri_shim.js`, in Chromium; it also runs `model_workspace_panel.py`, `model_workspace_loose.py` and `agent_f_cards.py`), `desktop_ui.py` (WebDriver on the built app). |
| `.github/workflows/` | `desktop.yml` (the Windows app, tests and release; Linux and the portability check only on request), `vendor.yml` (vendored crates for offline builds). |
| `docs/` | `PLAN.md` (roadmap and every design), `AGENT_B_HANDOFF.md`, `AGENT_E_HANDOFF.md`, `agent-c-handoff.md` (contracts and limits of the 0.5.6 packages), `screenshots/`. |

### Conventions

- **Phases and releases.** The repository keeps `0.<phase>.0` in `desktop/src-tauri/tauri.conf.json` and `desktop/Cargo.toml` (`tools/set_version.py 0.1.0` stamps them). CI picks the next free `0.<phase>.<n>` from the release tags and publishes `v<version>` only when the Windows app, the Rust tests and both acceptance tests pass. The release notes are the **Unreleased** section below, so write it for the user.
- **Windows first.** Windows is the platform being tested (owner's call, 2026-10-12). CI builds and tests only the Windows app on every push; the Linux app and the Linux-to-Windows library check run only from *Run workflow* with "Linux too" ticked. Keep code portable all the same. Start a new phase by bumping the minor number.
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
- **GitHub from the session:** use the GitHub tools you have. CI reports land on the `desktop-ci-windows` branch (logs, acceptance screenshots in `page/`, `Cargo.lock`), and on `desktop-ci-linux` when Linux was run; fetch those to read results.

### Known limitations now

- ZIPs are browsed in place; selected entries can be copied into a new model without changing the archive; 7z and RAR are kept as archives and not opened. Changes that move folders (category changes, moves, imports, setting aside) are undone newest first; a model folder can't be renamed from the app except through a category's model-folder template.
- Duplicates are judged by 3D, slicer and archive files only (pictures and documents can differ). The Duplicates page shows the last search; it isn't run by itself.
- Changes made outside the app are noticed by a watcher on the library folder (2 s after they settle) and by a check when the window comes back to the front and every 5 minutes. A model folder moved by hand keeps its id only if it has a `model.json`; one without gets a new id from its new path.
- The index lives in memory and is cached per computer (`<data dir>/index/<library id>.json`); 10,000 models take about 1 s to read the first time and under a second after.
- Previews are drawn by the core's own renderer (`thumb.rs`, flat shaded, no textures or colours from 3MF). Models whose 3D file can't be read get no preview and are tried again by every *Make previews*. G-code and other slicer files aren't shown in 3D.
- `Cargo.lock` isn't committed yet: each CI run makes one (the Tauri crates can't be resolved in a cloud session) and leaves it on the `desktop-ci-windows` branch.
- The `library://` protocol and `serve` answer range requests (8 MB at a time when the end is open), so videos seek; a whole-file read still loads it into memory. 3D files over 500 MB aren't shown.
- The app icons are Grid Workshop's; the interface has its own mark (a stack of layers). New icons are to do.
- Installers aren't code-signed, and there's no auto-update.
- PDFs use the system's engine (WebView2 on Windows, checked by the owner in 0.5.6; WebKitGTK on Linux is unchecked).
- A review reads and hashes every file before it appears; it runs as a job with progress and Stop (`checking.rs`, `web/ui/checking.js`). When the change is then made, its files are checked once more before the job starts, without progress.

### Next (roadmap)

Jerred's second round of requirements (R1–R17, `docs/PLAN.md`, "Requirements round 2") is being built in the agreed order of groups; groups 1 and 2 are built and both UI reviews' items fixed; Jerred's notes of 13 October (`docs/PLAN.md`, "Notes of 13 October": selection actions out of their own bar, a tabbed or otherwise more consistent layout from mock-ups, the same top bar on every page) wait on his pick of a mock-up; group 3 (gray default render, 3MF colours and thumbnails) is next, then skins for R16 (the owner picked Drafting Sheet as the default look and wants Parts Bin and Layer Line as selectable skins too; the style study's source is in the project files, style-study/), and a UI review follows each group. Before that: the UI pass in `docs/PLAN.md` ("UI pass design") is done: actions (0.5.1), undo and asking (0.5.2) and one layout (0.5.3). The model file workspace followed (0.5.5), then six parallel packages A–F (0.5.6, PRs #3–#8 merged in #9). Phase 6 (Docker with sign-in) is on hold until the owner says the UI is ready; ask before starting it.

---

## Unreleased

- **Compress to ZIP puts the model's files at the root of the ZIP.** Before, the ZIP also held a `_model-library` folder with a list of its files; now it holds only your files, as they were laid out in the model. The ZIP goes in the model's own folder, beside its `model.json` and previews (which stay outside the ZIP, as before). ZIPs made by earlier versions still extract the same way.

## 0.5.12 (2026-10-12)

Fixes from the second UI review of 0.5.11 (Requirements round 2, R17; all 23 items, design in docs/PLAN.md, "UI review 2 (0.5.12)").

- **Category changes keep what belongs to them:** icons now follow Rename or move…, Remove subcategory and Merge categories (and come back with Undo). A rename stops and names the file when a subcategory's folder holds something that isn't a model, instead of leaving it behind as a new "model".
- **Right counts:** beside the subcategory tiles the summary counts every model below, and says how many are filed right there.
- **Choose icon…:** Enter in the search picks the first match without closing; your own picture stays a choice.
- **Side panels fold the same way:** « and » buttons named Hide and Show, a strip with the panel's name, [ for the left panel and ] for Details. The category panel's edge is easier to grab, and it stays beside a model's page when the model is filed in a category.
- **The same words and icons:** Remove subcategory… (no models are deleted) and Add subcategory everywhere; Models has its own icon; a category without an icon shows the layers icon everywhere its name does, also on Home; Remove subcategory and Merge categories are in plain words.
- **The Unsorted preview** has one action row in its head (greyed out in a read-only library); Move to category… from Unsorted asks where first instead of warning.
- **Layout:** long titles show the place's name large with the categories above it; medium windows start with the category panel folded and no empty summary; the folded menu keeps the categories as icons; subcategory tiles show one picture per model; an empty category with subcategories says how to fill it; Open another library… is beside Show in folder.

## 0.5.11 (2026-10-12)

Requirements round 2, group 2 (R5, R3, R4, review items 11–14; design in docs/PLAN.md, "Group 2 design").

- **A category panel beside the menu.** The menu lists your categories one row each; open one and its subcategories fill a panel beside it, opened down to where you are. Drag its edge to make it wider, or fold it to a thin strip. On a narrow window the tree stays in the menu as before.
- **Subcategories or models.** A category page shows its subcategories as tiles with a few covers from inside, then the models filed right at that level ("Here in …"). Switch to Models to see everything below it, as before; searching always looks through everything below.
- **Icons for categories and subcategories.** Right-click one (or use its menu) and choose Choose icon…: pick from about 45 built-in icons, search them by word, or use a picture of your own (kept in the library's `_library/icons` folder so it travels with the library). Undo is in the message, and icons follow renames and moves in Edit category.
- **The Unsorted preview** names the model at the top with Move to category… and Open beside it; the Files tab no longer repeats the details (they're on the Details tab), and its views are Folders, All files and By type.
- **An empty category** says how to fill it: drop files on the window or use Import.

## 0.5.10 (2026-10-12)

Fixes from the first UI review of 0.5.9 (Requirements round 2, R17; design in docs/PLAN.md, "UI review 1 (0.5.10)").

- **Details on a model's page save when you leave a box** (or press Enter), with Undo in the message, as in the library's details panel; Esc puts the box back. Save and Cancel changes are gone. A change that clashes with a newer one made elsewhere still isn't saved until you choose Use latest details or Keep my edits.
- **The same words for the same thing:** Select shown (n) and Select all (n) in the file panel and the viewing area; Make a model… for selected files and Make this folder a model / Make every folder here a model on Import; Category, Start over and Close without importing in Import folders as categories; Edit details… with its dots on Import.
- **A model's page:** the file panel's views are Folders, All files and By type (only the viewing area has List), and a model with more than three variants picks one from a menu instead of a row of buttons that wrapped.
- **Fixes:** Save and Cancel stay at the foot of a long dialog (Edit category hid them below the window); messages sit above the viewer's bar, Import's Move/Copy bar and the status bar; Home points to About for the folder layout; "All 2 entrys" now counts files and folders; About names the system properly (Windows).
- **One yellow button at a time:** Sort a folder… on Import only while the workspace is empty, and Import n models only while nothing is selected (Set category is the next step then); Look for duplicates only until it has looked.

## 0.5.9 (2026-10-12)

Group 1 of the second round of requirements (design in docs/PLAN.md, "Requirements round 2" and "Group 1 design").

- **The same file actions in every view of a model**: Make a new model… and Send to another model… are on the right-click menu of the viewing area's grid and list (Send was missing there), the file panel and the selection bar, and now also in the Unsorted preview beside the collection, which gets the selection bar too.
- **Add subcategory where you are**: the category or subcategory shown in the menu has an "+ Add subcategory" row under it; type a name and press Enter (Esc cancels). Undo is in the message.
- **An About page** (bottom of the menu, and linked from Settings): the app's version, the library format, when it was built, what it's built with, what's changed in each release (from this file), what's coming, how a library is laid out, and the licences. `tools/build_desktop.py` now copies `CHANGELOG.md` and `THIRD_PARTY.md` into the interface and writes `build-info.json`; `app_info` has `library_format`.

## 0.5.8 (2026-10-12)

- **Big reviews show progress and can be stopped.** Before Send to another model, Add files, Delete models, Compress to ZIP or Extract archive, Make a new model and Import folders as categories show their review, every file is read and checked. That now says how many files and how much has been read, with Stop (and Check again), in the dialog and the status bar; the rest of the app stays usable meanwhile.

## 0.5.7 (2026-10-12)

Testing moves to Windows (the owner's main platform).

- CI tests Windows only: the acceptance test now runs on Windows, and the Linux app and the Linux-to-Windows library check run only when asked for. Release notes now come from this section instead of the list of commit subjects.
- The acceptance test runs on Windows in UTF-8 mode, and its import check no longer uses a model name that only differs in case from one already in the library (the same folder on Windows).

## 0.5.6 (2026-10-10)

Six packages built in parallel by agents A–F (PRs #3–#8, merged together in #9; contracts and limits in `docs/AGENT_B_HANDOFF.md`, `docs/AGENT_E_HANDOFF.md`, `docs/agent-c-handoff.md` and docs/PLAN.md's "Agent D" section). Every change below writes a journal and can be undone from Recent changes; anything unfinished after a crash is kept and offered on Home.

- **A. The workspace fills the window.** Files docks beside the collapsible sidebar; Details is a resizable panel on the right with Save and Cancel, which keeps a draft safe when the model changes underneath it (from a dialog, the watcher or Undo) and asks before overwriting. Unsorted models' folders, ZIPs and files can be previewed beside the collection.
- **B. Working with a model's files.** Select visible or all files (extracting a whole model retires the empty source, with Undo); Send to another model (sorted or Unsorted) by moving or copying; Add files from anywhere on the computer; Delete one or several models, which moves them into the journal so they can be restored (they don't expire).
- **C. PDFs and ZIPs.** PDFs, ordinary and inside ZIPs, show in the app with page and zoom controls. Compress to ZIP and Extract archive show what will be written, check every byte after writing, and only remove the originals as a separate step you choose. Files with no viewer have Open in its own app. `.3ds` and `.max` count as 3D files.
- **D. Import folders as categories.** Import shows Sort a folder, Import folder, Import file and Import folders as categories in its action row. The last reads a folder tree into an editable staging tree (which folders are categories and which are models, exclusions, grouping loose files, renaming, mapping onto existing categories) and shows a review of every destination before anything is written.
- **E. Remove a subcategory and merge categories.** Removing a subcategory moves its models and subcategories up, or its models to Unsorted. Several categories or subcategories can be merged into an existing or new one, with a review of every path and collision.
- **F. Model cards.** Authors are italic, and the category path is smaller. Cards without a chosen cover get an automatic one made from up to four of the model's files, with a count of the rest. A chosen cover whose picture disappears says so and falls back, and **Use automatic preview** goes back to the automatic one. The index cache moves to format 2 and is rebuilt once.
- Fixed for Linux: WebKitGTK refused to start the app when a script's name or bytes looked like a PDF; the build now fails if one does.

## 0.5.5 (2026-10-08)

Model file workspace (design and implementation notes in docs/PLAN.md).

- Model files open directly from the shared selection: a 3D part, picture, readme or video has its own viewer; folders and ZIPs have sortable tiles or a list, a breadcrumb, shared selection and sibling navigation. Compact Import keeps its existing viewer.
- **A file panel on the left of each model**: resize it, collapse it with `[` or its button, and use it as a drawer on small windows. Folders, List and By type share file selection with the viewing area; variants and search keep large models manageable. Panel width, view and open state are remembered. Import keeps its compact layout.
- Make selected files or folders a new model from the file panel or viewing area: preview its name and category, move or copy, then Undo or Open from the message. Whole-model and read-only selections explain why extraction is unavailable.
- **Make a new model from selected files:** preview destinations, move or copy files and folders, unpack selected ZIP entries, and undo from Recent changes. Transfers stage verified copies before removing originals, preserve source details and previews for exact undo, reject unsafe paths, and protect files changed outside the app.
- Files copied directly into category folders now appear on Home, with Put in a folder, matching pictures and documents, and Undo in Recent changes.
- Updated the existing model-page acceptance checks for the shared file panel and folder viewers, added the panel checks to the acceptance run, and recorded the workspace implementation notes in `docs/PLAN.md`.

## 0.5.4 (2026-10-07)

Import follow-ups (owner's asks after the UI pass; design and notes in docs/PLAN.md, "Import follow-ups").

- **Choose which folder level is the model** on Import: Make this folder one model, Make every folder at this level a model (for maker › release › model collections where the app took the parts for models), Make each folder a model for several selected, and Split into models. A model's panel shows the folders above it; click one to make it the model instead. Each has Undo.
- **Dropping files or folders on the window asks what to do**: add them to the category shown (anywhere else, Unsorted) as a model, moved in with Undo or copied; or sort a folder on Import first. The window says where a drop goes while something is dragged over it.
- **File types are easy to tell apart**: a coloured tag per type (STL, 3MF, other 3D, slicer, ZIP and other archives, pictures, documents, videos) on Import's rows and in every file list; folders are amber, and ZIPs no longer share the folder icon.
- Core: `sort_join` takes several folders or a level, `sort_commit` can take what it imported off the list (`forget`), new `sort_paths`; file summaries have `exts`.

## 0.5.3 (2026-10-07)

UI pass, step 3 of 3: one layout (design in docs/PLAN.md, "UI pass design"; notes in "UI pass notes").

- **Every page starts the same way**: the title and its count, a line under it where needed, then the page's main button and one menu on the right (Library ▾ on Home, Category ▾ in the library, More ▾ on Import). Under it one toolbar: search, sort, view.
- **One view switcher** (icon and label) and **one sort menu** ("Sort: Name ▾") on the library, a model's files and Import. The library's list view has column headers that sort when clicked, and shows when each model was added and its size.
- **The details panel is edited in place**: change a model's name, authors or tags right in the panel; it's saved when you leave the box, with Undo in the message. Several selected shows what they share; nothing selected sums up the place (models, size, main authors and tags). Duplicates has the same panel, and Import's panel holds its actions.
- **"Searching for … · Clear"** above the results, saying how many were found in the place shown, with Search all models when the search carries into a category.
- **Home is the place to start**: recently added models, what's waiting to be sorted, the categories, Recent changes, and a Library ▾ menu (Show in folder, Read again, Make missing previews, Open another library…). How the library is laid out and What's coming moved to Settings › About.
- New in `web/ui/layout.js`: `PageHead`, `MenuButton`, `ViewSwitch`, `SortMenu`, `SearchNote`, `EditBox`. `models_query` also returns `bytes`.

## 0.5.2 (2026-10-07)

UI pass, step 2 of 3: undo and asking (design in docs/PLAN.md, "UI pass design"; notes in "UI pass notes").

- **Every change can be undone, from its message or from Recent changes on Home**: Move to category, Import (the files go back where they came from, or the copies are deleted), Edit details for one or several models, and Use as cover now write a journal in `_library/journal/`, as category changes do. Changes that move folders are undone newest first; an edit to details can be undone on its own unless a newer change touched the same model. Home shows Undo on every row and says why one has to wait.
- **Move to category shows what moves first**, with the same list of folders as category changes, and the button says how many models move.
- **One red confirm** for what can't be undone: Delete set-aside copies… asks in a dialog that names what it deletes.
- **Read again can be stopped** from the status bar; the library then stays as it was read before.
- Fixed: the Import page could be used for a moment before its workspace had loaded (it now says "Reading the workspace…"); a name typed on Import just as a model's details appeared could be put back (the cause of a rare page-test timeout).

## 0.5.1 (2026-10-07)

UI pass, step 1 of 3: consistent actions (owner's request; design in docs/PLAN.md, "UI pass design"; notes in "UI pass notes"). Phase 6 is on hold until the UI pass is done.

- **The same actions everywhere**: the details panel, the model page and the panel for several selected models have one row, Open, Edit details…, Move to category…, Star, Show in folder and More (Make a new preview, Copy folder path). Right-clicking a card, a list row, a Duplicates row, a file or a sidebar category shows the same actions as a menu (Shift+F10 or the menu key too). Import uses the same row and words (Set category…, Combine into one model, Split, Skip, Clear category).
- **Selecting works the same everywhere**: click, Ctrl-click, Shift-click for a run, tick boxes, Ctrl+A, Esc, arrows and Shift+arrows, in the library and on Import. Keys: Enter opens, E edits details, F2 edits the name, M moves, S stars, / searches, Ctrl+Z undoes.
- **Undo in the message**: starring, editing details, moving models, category changes, adding, renaming and deleting subcategories, setting duplicates aside, variant names, the library's name and every change on Import (a new core command, `sort_undo`, keeps the last 20 workspaces) show a message with an Undo button; Ctrl+Z undoes the last one. Start again on Import no longer asks.
- **A Category ▾ menu** on every category page and on right-click in the sidebar: Add subcategory…, Edit category…, Rename or move…, and a red Delete. An empty subcategory is deleted straight away (with Undo); one with models in it lists the folders that move up a level first.
- **The same words** on every page (Starred, Read again, Show more (N left), Largest first, Search, Open another library…) and tooltips on icon buttons.
- Fixed: the details panel showing a model from another place; Delete category drawn in yellow; Delete in Edit category losing unsaved edits; files with no viewer looking clickable (now greyed, with Open in its own app); filter chips that couldn't be taken off; the theme button never going back to following the system; Starred without a count; Read again on Home not saying when it fails; messages covering Import's footer; Esc missed when pressed just as a dialog opened.

## 0.5.0 (2026-10-06)

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
