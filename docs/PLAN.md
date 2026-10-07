# Model Library: architecture and build plan

Draft 2, 2026-10-06 (kept here as the plan of record; notes for each finished phase are at the end). Written after reading `DrFlGd/Claude-Grid-Workshop` at `main` (0.4.1). Draft 2 adds Jerred's answers (section 12) and the model structure (section 3a). The code lives in the new repository `DrFlGd/Model-Library`.

## 1. The idea in one paragraph

The library is a plain folder tree that makes sense without the app. A **schema** (for example "Wargames: Game > Faction > Model") decides where each imported model goes, so a Hive Tyrant lands in `Wargames/Warhammer 40k/Tyranid/Hive Tyrant (Author Name)/`. Each model folder holds its 3D files, its supplemental files (images, PDFs, videos, readmes) and one small metadata file. The app's database is only a fast index over those folders and can be rebuilt from them at any time, so a new version of the app (or a copy of the library on another machine, or the Docker build) just opens the folder and reads it back.

## 2. What to take from Claude Grid Workshop

Grid Workshop is Tauri 2 + a Rust core (`desktop/core`, about 10.6k lines, no GUI) + a no-build Preact/htm front end (`web/`, about 7k lines). Its architecture already fits this app well, and a fair amount carries over almost directly.

| Grid Workshop piece | Use in Model Library |
| --- | --- |
| Tauri shell (`desktop/src-tauri/src/main.rs`): dialogs, opener, `library://` protocol | Keep. The protocol needs **range requests** (it reads whole files into memory today), which videos and 200 MB STLs need. |
| `api.rs`: one command table behind `App::call`, plus background jobs the page polls | Keep the pattern exactly. It is what makes Docker cheap later (below). |
| `workshop-cli serve`: the same commands over HTTP (used for tests today) | Becomes the **Docker server**. |
| `web/platform.js` / `platform-desktop.js`: the browser vs desktop seam | Keep; add a "server" platform for Docker. |
| `library.rs`: portable library folder, `FORMAT` number (newer library opens read-only), safe relative paths, trash | Keep the ideas and most helpers; the layout changes (section 3). |
| `meta.rs`: layered metadata (library > project > folder > item) with "where this value came from" | Keep. Maps naturally to library > category level > model > file. |
| `merge.rs`: merge another library, report conflicting edits | Reuse for "re-import / merge a library copy". |
| `docs.rs`: Markdown README to HTML, PDFs, text | Keep for readmes and PDFs. |
| `sources.rs`: ZIP and folder intake | Reuse the archive and folder walking for import. |
| `web/ui/`: browser (grid, list, table, grouped views), sidebar, inspector, palette, quick look, side viewer (`sideview.js`), metadata editor (`metaedit.js`, single and bulk edits with undo) | Keep. This is the "solid UI framework": it already does most of the browsing and editing interaction. |
| `web/ui/index-local.js`: search with facets, typed filters, synonyms | Keep the query language and facet UI; move the index itself into SQLite in Rust (section 6). |
| `web/viewer.js` (three.js, STL) and `thumb_put` (page renders a thumbnail, backend stores it) | Keep. Add 3MF and OBJ loaders; drop the 42 mm Gridfinity grid. |
| Tests: `desktop_page.py` (page + real backend via `tauri_shim.js`), `desktop_ui.py`, CI with `crates-vendor` branch | Keep the harness and CI recipe. |
| OpenSCAD engine, `render.rs`, `components.rs`, `catalog/`, `vendor/`, `sitebuild.rs`, parameter forms | Drop. Not needed for a file library. |

Recommendation: start a **new repository** seeded with the shell, core helpers, `web/ui` and test harness, rather than growing Grid Workshop. The products differ (a parametric generator vs a file library) and Grid Workshop has its own release phases.

## 3. On-disk layout

```text
<Library root>/
  _library/                         app files, kept small
    library.json                    format version, library id, name, settings
    schemas/wargames.json           one file per schema
    trash/                          removed models, until emptied
  Wargames/                         a schema's top folder
    _category.json                  optional: metadata every model below inherits
    Warhammer 40k/
      Tyranid/
        _category.json              e.g. faction notes, default tags
        Hive Tyrant (Author Name)/  one model
          model.json                the sidecar (below)
          Hive Tyrant body.stl
          Hive Tyrant arms.3mf
          Presupported/…            sub-folders inside a model are kept as they came
          _media/cover.jpg          supplemental files (images, PDFs, videos, docs)
          _media/assembly.pdf
          _media/README.md
          _thumbs/                  generated previews (rebuildable)
  Unsorted/                         imports with no schema yet
```

Rules:
- **A model is a folder**, not a single file (section 3a). Real releases are many files (parts, variants, presupported versions, a Lychee or slicer project), and they move together.
- Folder names come from **name templates** in the schema (`{name} ({author})`), cleaned for Windows (no `<>:"/\|?*`, no trailing dot or space, reserved names like `CON`), with a length budget so the full path stays under the Windows 260-character limit unless long paths are enabled. Clashes get ` (2)`.
- Files starting with `_` and `model.json` are the app's; everything else is the user's and is never renamed silently.

## 3a. What a model looks like inside

Models arrive in three shapes, and all three become the same thing: **one model folder**.

| Arrives as | Becomes |
| --- | --- |
| A single file in a folder | A model folder holding that file |
| A ZIP in a folder | A model folder holding the ZIP, **kept zipped** by default (below) |
| Several files, maybe in sub-folders | A model folder with the sub-folders kept as they are |

Inside a model, the app shows a **part tree** built from the sub-folders, so a large armour set is one model ("Knight Armour Set") with parts Helmet, Arms, Legs and so on:

- Each sub-folder is a **part group**; its files are the parts. The model page lists the tree, and the viewer can show one part, one group, or all of it.
- Sub-folders whose names mean a variant rather than a part (`Presupported`, `Supported`, `Unsupported`, `Lychee`, `Chitubox`, `32mm`, `75mm`, …) are labelled as **variants**, and the tree gets a variant switch instead of showing them as extra parts. The list of variant words is editable in the library settings.
- Labels, the role of each sub-folder (group or variant) and the order are stored in `model.json` and editable, without renaming anything on disk.
- **ZIPs are read without unpacking.** The app lists a ZIP's contents in the part tree (sub-folders inside it become groups the same way), shows and thumbnails its STLs and images by reading just that entry, and searches its file names. An **Unpack** action extracts it into the model folder and removes the ZIP after a check, and the import wizard has a "unpack ZIPs" switch for whole batches. RAR and 7z are unpacked at import (they can't be read entry by entry cheaply).

## 4. Metadata: the sidecar

`model.json` (plain JSON, pretty-printed, so it is readable and diff-able):

```json
{
  "format": 1,
  "id": "01J9Z6K4V3…",              // ULID: stable even if the folder is moved or renamed
  "schema": "wargames",
  "category": { "game": "Warhammer 40k", "faction": "Tyranid", "model": "Hive Tyrant" },
  "name": "Hive Tyrant",
  "authors": [{ "name": "Author Name", "url": "…" }],
  "released": "2025-03-14",
  "source": { "site": "MyMiniFactory", "url": "…", "order": "…" },
  "license": "personal",
  "tags": ["monster", "presupported"],
  "fields": { "scale": "32mm", "base": "60mm" },   // the schema's own fields
  "parts": {                          // labels and roles for sub-folders (and folders inside ZIPs)
    "Helmet": { "label": "Helmet", "role": "group", "order": 1 },
    "Presupported": { "role": "variant", "label": "Presupported" }
  },
  "files": {
    "Hive Tyrant body.stl": { "role": "part", "sha256": "…", "notes": "" },
    "Hive Tyrant.zip": { "role": "archive", "sha256": "…" },
    "_media/cover.jpg": { "role": "cover" }
  },
  "added": "2026-10-06T16:00:00Z",
  "updated": "2026-10-06T16:00:00Z"
}
```

- Inheritance reuses `meta.rs`: library defaults, then each `_category.json` up the tree, then the model, then a file entry. The editor shows where each value comes from, as it does today.
- The sidecar is the source of truth. The index stores a copy plus the sidecar's modified time, so a changed sidecar (edited by hand, or synced from another machine) is re-read.
- Unknown keys are kept on write, so an older app never deletes a newer app's data.

## 5. Schemas and categories

A schema is a JSON file the app edits through a form:

```json
{
  "id": "wargames", "name": "Wargames", "folder": "Wargames",
  "levels": [
    { "key": "game", "label": "Game", "values": "open" },
    { "key": "faction", "label": "Faction", "values": "open" }
  ],
  "model_folder": "{name} ({author})",
  "fields": [
    { "key": "scale", "label": "Scale", "type": "choice", "choices": ["28mm", "32mm", "75mm"] },
    { "key": "base", "label": "Base size", "type": "text" },
    { "key": "presupported", "label": "Presupported", "type": "yes/no" }
  ]
}
```

- Level values are **learned from the library** (every faction already present becomes a suggestion) and can be pinned to a fixed list.
- Editing a category ("Tyranid" to "Tyranids", or merging two factions) and editing a schema (adding a level, changing the name template) both **re-lay the library**. The app always shows a preview first ("212 folders will move"), then runs it as a journaled job (a move log in `_library/`) that can resume after a crash and be undone. Moves on one drive are renames, so this is fast even for terabytes.
- A model has one place on disk. Anything else ("also Necromunda", "painted", "favourites") is tags and saved searches (virtual collections), not copies.

## 6. Index and search

- **SQLite** (with FTS5 full-text search) in the app's data folder on each computer, never in the library. Tables: models, files, categories, tags, and the sidecar modified time for each model.
- Opening a library: walk folders, read only `model.json` and `_category.json` files whose modified time changed. Thousands of models open in seconds; 2-3 TB of model files are never read just to open the library.
- A **file watcher** (`notify` crate) plus a reconcile scan picks up changes made outside the app (a folder moved in Explorer is recognised by its `id`).
- The existing query language and facets in `index-local.js` move onto SQL: `game:"Warhammer 40k" faction:Tyranid tag:presupported author:…`.

## 7. Import flow

1. **Drop** files, folders or archives (ZIP, 7z, RAR; reuse `sources.rs` for ZIP) onto the app, or pick "Import".
2. **Stage**: archives are unpacked to a staging folder on the same drive as the library.
3. **Classify** every file: models (STL, 3MF, OBJ, STEP), slicer and support projects (Bambu/Orca/Prusa 3MF projects, `.lys`, `.chitubox`, `.gcode`, netfabb `.fabbproject`), images, PDFs, videos, docs. Unknown files are kept as "other".
4. **Group** into model candidates (one archive or top folder = one model by default), with drag-to-regroup.
5. **Fill in**: pick a schema, then the category levels and fields. Guesses come from folder and file names, embedded 3MF metadata and readmes; values autocomplete from the library. Batch imports use a table where a column can be filled for many rows at once (the existing bulk editor).
6. **Preview** the destination path for each model, then **commit**: **move** by default (no duplicate files), with **copy** as an option per import. A move on the same drive is a rename; a move from another drive or the NAS is copy, checksum check, then delete the original. Then write `model.json` and queue thumbnails. Duplicates (same file hashes as an existing model) are flagged before commit.
7. Messy existing folders go through this same wizard: point it at a folder on the NAS or a local drive and it proposes model candidates for review. **Adopt in place** is a later option for folders that are already well organised: point the app at a folder tree, map its folder levels to a schema's levels ("level 1 = Game, level 2 = Faction"), and write sidecars without moving anything; re-laying out is a separate, previewed step.

## 8. Viewing models and supplemental files

- **3D**: the three.js viewer, with STL, 3MF and OBJ loaders, loading in a worker so a large file doesn't freeze the window, with a size cap above which the user is asked first.
- **Thumbnails**: use an embedded 3MF thumbnail or a `cover` image when there is one; otherwise the page renders one with the same viewer and stores it with `thumb_put` (the mechanism Grid Workshop already has), as a small WebP in `_thumbs/`. Generated in the background, a few at a time.
- **Supplemental**: image gallery, PDFs (the built-in viewer the side viewer uses), video (HTML5 player, needs the range-request fix in section 2), Markdown and text via `docs.rs`. All shown in the side viewer tabs and the model page.
- "Open in slicer" and "Show in folder" buttons from the start; deeper slicer features later.

## 9. Scale (thousands of models, 2-3 TB)

- Opening and searching never reads model files, only sidecars and the index.
- Hashing is background work with a quick fingerprint first (size and the first and last 64 KB), full SHA-256 later and only once per file (stored in the sidecar).
- Thumbnails, hashing and imports are jobs with progress and cancel (the existing job system), with a concurrency limit so a NAS is not flooded.
- Every multi-folder change (import, re-layout, rename) is journaled so it can resume or roll back.
- Test against a generated library of 10,000 model folders before the first real import.

## 10. Docker later

Because every action already goes through one command table, the Docker build is the `serve` binary plus the same `web/` front end:
- Rust server in a small image, the library mounted as a volume (`-v /mnt/nas/models:/library`), the index in a second volume.
- A "server" platform in `platform.js`: uploads instead of native file dialogs, downloads instead of "Show in folder", HTTP range requests for files.
- Simple login (one password, or leave auth to a reverse proxy).
- The desktop app and the server can open the same library, since all real data lives in the folders.

## 11. Phases

| Phase | What | Done when |
| --- | --- | --- |
| 0 | New repository seeded from Grid Workshop (shell, core helpers, `web/ui`, tests, CI); OpenSCAD parts removed | The empty app builds on Windows and Linux in CI |
| 1 | Library core: library format, sidecar read/write, schema files, scan into SQLite, browse by category tree, search | A hand-made test library of a few hundred models opens, browses and searches; 10,000 generated models open in seconds |
| 2 | Import: staging, classify, group, schema form, destination preview, copy/move with checksum, batch table, duplicate check | Import a folder of Printables/MMF downloads into the right folders with sidecars |
| 3 | Viewing: part tree with groups and variants, files inside ZIPs, 3D viewer (STL, 3MF, OBJ), thumbnails, supplemental tabs, range requests | An armour set shows as one model with its parts, and a zipped model previews without unpacking |
| 4 | Editing: schema editor, category rename and merge, bulk metadata, journaled re-layout with preview and undo | Renaming a faction moves its folders and can be undone |
| 5 | Large collections: adopt-in-place for tidy trees, duplicate report across the library, file watcher | A whole NAS share of models is in the library |
| UI pass (0.5.x) | Consistent actions, undo for every change, one page layout (see "UI pass design") | The same action has the same name, place, keys and undo on every page |
| 6 | Docker: server platform, auth, image (on hold until the UI pass is done) | The same library browses from a browser on another machine |
| Later | Metadata fetch from Printables, MMF and Thingiverse pages; collections; print status and notes; slicer integration; plugin ideas from Grid Workshop's "Future" list | |

Following Grid Workshop's convention, each phase gets written into a plan doc in the repo before code, with a notes section afterwards.

## 12. Decisions taken (Jerred, 2026-10-06)

1. **New repository**: `DrFlGd/Model-Library`.
2. **Move or copy** is an option at import; **move is the default**, so files aren't duplicated.
3. Existing models are in imperfectly organised folders on a NAS and local drives, and the tool is meant for general use, so the **import wizard comes first** (Phase 2). Adopt-in-place for tidy trees comes later (Phase 5).
4. **The folder is the model.** Single files, ZIPs and multi-file folders with sub-folders are all supported (section 3a). Sub-parts stay viewable as a part tree.
5. **Thumbnails are stored in the library**, in each model's `_thumbs/` folder.

## Phase 0 notes

Built (2026-10-06): the repository seeded from Grid Workshop with the OpenSCAD parts left out.

- **Library folder.** `_library/library.json` (format 1, id, name, created, favourites), `_library/schemas/` (empty until Phase 1), `_library/README.txt` and `Unsorted/`. Opening a folder that already holds models adds only those; a library with a higher format opens read-only and nothing is created in it. Default location `~/Model Library`.
- **Backend.** `desktop/core` keeps Grid Workshop's single command table (`App::call`) with `app_info`, `prefs_get` / `prefs_set` (favourites stored in the library), `library_open` and `library_rename`; `modlib-cli serve` exposes it over HTTP. The jobs system, `meta.rs`, `docs.rs` and `merge.rs` were not copied yet: they come with the phases that use them.
- **Interface.** Home (the library, its layout, what's coming), placeholders for All models, Unsorted and Favourites, Settings (rename, show in folder, open another or a recent library, theme), the status bar with the open library. Same look and themes as Grid Workshop, with its own mark.
- **Tests.** Rust unit tests for the library and commands; `tests/desktop_page.py` (9 checks, including a moved library opening the same); `tests/desktop_ui.py` on the built app; CI checks a library made on Linux opens the same on Windows.


## Phase 1 design

Written before the code (2026-10-06). Goal: a library of model folders (made by hand, or by the import of Phase 2) opens, browses by schema and category, searches, and shows and edits each model's details.

**Which folders are models.** Under a schema's top folder, the folders at depth *levels + 1* are models (`Wargames/<Game>/<Faction>/<Model>/` for a two-level schema), and so is any folder holding a `model.json`, at any depth. The folder names above a model are its category values: **the path is the truth**, so a folder moved by hand in a file manager shows in its new place. Each folder directly in `Unsorted/` is a model with no schema. Nothing below a model folder is a model (its sub-folders are parts, Phase 3). Folders starting with `_` are the app's and aren't categories.

**Models without a model.json** are listed too, with what the folder says: the name and author parsed from the schema's model-folder template (`Hive Tyrant (Author Name)` → name *Hive Tyrant*, author *Author Name*). The first edit, or starring the model, writes its `model.json` with a new id. Until then its id is derived from its path.

**Schemas** are `_library/schemas/<id>.json` (format, id, name, folder, levels, model_folder template, fields). Phase 1 adds **New schema…** (name, top folder, levels, model folder template, fields), with *Wargames: Game > Faction* as the example it starts from. Changing an existing schema re-lays the library and waits for Phase 4.

**Model details** are the sidecar of section 4: name, authors, released, source (site, URL), license, tags, notes and the schema's own fields. Editing them rewrites `model.json` (unknown keys kept). Changing the name doesn't rename the folder yet (Phase 4). Inheritance from `_category.json` files (section 4) also moves to Phase 4, with the category editor.

**The index: in memory, cached on each computer.** A change from section 6: instead of SQLite, the core builds an in-memory index from the folders and caches it in the app's data folder (`index/<library id>.json`, with each model's `model.json` and folder modified times, so opening again rereads only what changed). Ten thousand models are a few megabytes and search in milliseconds, it needs no native dependency on Windows, and SQLite stays an option if collections outgrow it. A **Rescan** button rereads everything (a file added deep inside a model's sub-folder doesn't change the model folder's modified time).

**Each model in the index** has its id, path, schema, category values, details, and a summary of its files by kind (models: STL, 3MF, OBJ, STEP…; slicer and support projects: gcode, Lychee, Chitubox, netfabb…; images; documents; videos; archives; other), total size, and a cover picture (the sidecar's `cover`, else the first image in `_media/`, else in the folder).

**Search** runs in the core: words match the name, authors, tags, category values, field values and file names (prefixes count, accents ignored), and typed filters narrow it: `author:`, `tag:`, `schema:`, `kind:` (has a file of that kind), and a schema's level keys (`faction:tyranid`). Results come sorted (name, recently added, size) and in pages, with counts of authors and tags for filter chips.

**Interface.** The sidebar lists the schemas with their category trees and counts; *All models*, *Unsorted* and *Favourites* list models. A place shows a search box, sort, grid or list view, and model cards (cover or a kind icon, name, author, category). Selecting a model opens a details panel (category path, details, files by kind, Show in folder, star); *Edit details…* opens a form with the schema's fields.

**Done when:** a hand-made library of a few hundred models opens, browses by category and searches; a generated library of 10,000 models opens in seconds and searches in under 100 ms (`modlib-cli make-test-library` and a timed check in CI).

## Phase 1 notes

Built 2026-10-06, as designed above, with these differences and details:

- **The first start asks where the library goes** (Jerred's note during the phase: the library folder must be configurable and portable, as in Grid Workshop). The core no longer makes `~/Model Library` on its own; `app_info` reports `first_run` and the suggested `default_library`, and Home shows *Use ~/Model Library* or *Choose a folder…* until one is opened. Settings still opens or switches libraries at any time.
- **New schema…** starts empty with *Wargames*, *Game* and *Faction* as placeholder hints rather than filled-in values, so nobody makes a "Wargames" schema by accident. The preview line shows where a model would go.
- **Rescan** is *Read the folders again* on Home (`library_scan` with `full`); the index also refreshes itself when a model is edited or starred, and opening the app again rereads only folders whose modified time changed.
- **Ids.** A model without `model.json` has an id derived from its path (`p` + hash), so a star or an edit first writes its `model.json` (with a time-ordered `m…` id, the name and author it was showing, its schema and category) and favourites follow the new id.
- **Search** also matches file names and a schema's field keys (`scale:32mm`); level filters match the start of a value (`faction:space`). Field values that aren't text (numbers, yes or no) are filterable but not searched as words.
- **Timings** (cloud sandbox, release build, 10,000 generated models): first read about 1 s, reopen from the cache under 1 s, a search with a filter about 5–7 ms; the cache is about 6 MB. CI checks the same with `make-test-library` and `library-scan` in `desktop_page.py` (`--big`).
- Not done here, as planned: thumbnails (Phase 2), viewing files and parts (Phase 3), changing schemas and `_category.json` inheritance (Phase 4).

## Phase 2 design

Written before the code (2026-10-06). Goal: get models into the library and into their categories. Point the app at a messy folder (on the NAS, a local drive, or one already sitting inside the library), review what it proposes, pick categories, and the files are moved (or copied) into the right folders with a `model.json`. Jerred also asked for **Move to category…** on models already in the library (for example, ones in Unsorted).

Changes from section 7, by decision: ZIPs stay zipped (2026-10-06), so there is no unpacking or staging step; an archive becomes a model folder holding the archive. Thumbnails stay in Phase 3 with the viewer that renders them.

**Sources.** Two ways in, both on the Import page:
- *Sort a folder…*: the folder's contents are the candidates. Each sub-folder is one model; each loose model file or archive is one model; loose pictures and documents whose name starts the same as a loose model go with it, and other loose files are listed as left behind.
- *Add a model folder…* / *Add a file…*, or dropping folders and files on the window: each dropped item is one model.
Folders that are the library itself, or hold it, are refused; folders inside the library are allowed (that's how a messy folder already in the library gets sorted).

**Candidates** show: name and author (from a `model.json` if the folder has one, as when re-importing from another library; else read from the folder or file name by the `{name} ({author})` pattern), a summary of the files by kind and size, and warnings:
- *Maybe several models*: no model files at its top but two or more sub-folders with model files. It may be one model with parts (an armour set) or a collection; **Split** replaces it with its sub-folders.
- *Already in the library*: same number of files and the same file sizes as a model already there (cheap: no hashing of the library).
- *Nothing to import*: no files.

**Category guesses.** If a folder name on the candidate's path (or its own name) equals a known category value, that schema and those levels are suggested (for example `Downloads/Tyranid/Carnifex` suggests Wargames > ? > Tyranid only if Tyranid is already a Faction). A `model.json` with a schema and category wins. Anything unguessed goes to Unsorted unless set.

**The review table.** One row per candidate: include it or not, name, author, category (a schema, or Unsorted) with a box per level (suggesting values already in the library), tags, and the destination folder, worked out by the core as you type (`import_plan`). Tick rows and use **Set for selected** to give many the same category, author or tags. **Move** (default) or **Copy** for the whole import.

**Committing** runs as a background job with progress and a Stop button:
1. Destination: `<schema folder>/<level values…>/<model folder name>` (`{name} ({author})`, or just the name with no author), each part cleaned for Windows; Unsorted models go in `Unsorted/<name (author)>`. An existing folder gets ` (2)`, ` (3)`…
2. Move: a rename when on the same drive. Otherwise (and for Copy) every file is copied, each copy checked against the original by SHA-256, and only for a move are the originals deleted once the whole model is copied and checked. A failure leaves the original in place and reports it.
3. A loose file (or archive) gets a new folder of its own.
4. `model.json` is written: the details from the table and the folder's own `model.json` if it had one (keys kept), with `added` and the import source recorded (`imported_from`).
5. The index rereads the library when the job ends.

**Move to category…** on one or several selected models (Ctrl or Shift click picks several) opens the same category picker. The model's folder is renamed into place (same drive), its `model.json` gets the new schema and category (and an id, if it had none; favourites follow), and category folders left empty are removed. The folder keeps its name.

**Unsorted folders on Home.** Folders at the top of the library that aren't a category's folder, `Unsorted` or the app's are listed on Home as *not sorted yet*, each with **Sort…** that opens the Import page on it.

**Done when:** a folder of Printables/MMF-style downloads (folders, loose STLs, ZIPs, an armour set with part folders, a model from another library with its `model.json`, and a duplicate) imports into the right category folders with `model.json`s, by move and by copy, with the copy checked; and models in Unsorted can be moved into categories.

## Phase 2 notes

Built 2026-10-06, as designed above, with these details:

- **Core:** `import.rs` (`scan`, `plan`, `transfer`, `commit_one`, `move_model`, `loose_folders`). Commands: `import_scan {paths, contents}`, `import_plan {items}`, `import_commit {items, mode, force_copy}` (a job), `job {id}`, `jobs`, `job_cancel {id}`, `models_move {ids, schema, values}`; `library_overview` gains `loose`. Jobs follow Grid Workshop's pattern: one import at a time, the page polls `job` every 300 ms, and the index rereads the library when a job ends.
- **Checked copies:** each file is hashed (SHA-256) while it's copied, the copy is hashed again, and the original of a move is deleted only after the whole model is copied and checked. `force_copy` (tests only) takes that path even on one drive. A stop or failure removes the half-made copy and leaves the original. Loose files are gathered in a hidden `.importing-…` folder next to the destination and renamed into place, so a half-import never shows as a model.
- **model.json on import:** the details from the table, `imported_from` (the source path), and `schema` and `category` (also written by Move to category, through `model::set_place`). A `model.json` brought from another library keeps its keys; its id is replaced only if this library already uses it.
- **Move and Copy** is chosen per import, and goes back to Move after each one.
- **Folder names** keep `{name} ({author})` from the schema; a name already taken gets ` (2)`. Level values and names are cleaned for Windows (`clean_folder_name`), and can't start with `_`.
- **Guesses:** a category is suggested when a folder name on the source path (or the model's name) equals a value the library already has (its whole path above comes from that model), or names a category's folder. A `model.json` with a schema and category wins.
- **Drag and drop** uses Tauri's `tauri://drag-drop` event (paths of the dropped items); it can't be exercised by the page test, which uses the Add buttons.
- **Loose folders** on Home are non-empty top-level folders that aren't a category's, `Unsorted` or the app's; sorting one moves its contents out and leaves the empty folder (no longer listed).
- Not done: a regroup by dragging rows (Split covers the common case of a collection folder), unpacking archives (decision: ZIPs stay zipped), checking for duplicates by content hash (Phase 5).

## Phase 3 design

Written before the code (2026-10-06). Goal: see what's in each model: its parts, variants, files inside ZIPs, the 3D models themselves, its pictures, PDFs, videos and readmes, with a thumbnail on every card. Done when an armour set shows as one model with its parts, and a zipped model previews without unpacking.

**The model page.** Double-clicking a card, or **Open** in the details panel, opens `#/model/<id>`: a large viewer on the left and, on the right, the model's details and its files as a tree. Tabs above the viewer switch between **3D**, **Pictures**, **Documents** and **Videos** (only the ones the model has). Back returns to where you were.

**Parts and variants.** Sub-folders show as a part tree (Helmet, Arms, Legs…), each part clickable to view. Folders named like a variant (Presupported, Pre-supported, Supported, Unsupported, No supports…), at any depth, become a **variant switch** above the tree: picking one shows that variant's files plus the files outside any variant folder. The choice is per model and not saved.

**Inside ZIPs.** A ZIP in the tree unfolds into its entries (read from the archive's directory, nothing unpacked). 3D files and pictures inside a ZIP open in the viewer straight from the archive; nested archives are listed, not opened. 7z and RAR stay closed (no library to read them here).

**The 3D viewer** is Grid Workshop's three.js viewer (orbit, iso/top/front views, edges), without its Gridfinity grid: a plain floor grid in 10 mm squares, with the model's size shown. The core reads STL (binary and text), OBJ and 3MF (including Bambu/Orca projects whose meshes live in separate files, with component and build transforms) and sends the page triangles as binary STL, so the page has one loader. Very large meshes (over 2 million triangles) are shown without edges.

**Thumbnails** are made by the core, not the page, so the Docker server can make them too: a small software renderer draws the model's main 3D file (the largest at the shallowest depth, preferring files outside an "unsupported" folder; inside a ZIP if that's all there is) from the same three-quarter view as the viewer, flat-shaded in the app's amber, on a transparent background, 400×300, into `_thumbs/model.png` in the model folder. A card's cover is the model.json `cover`, else a picture in `_media/`, else any picture, else that thumbnail. Making them runs as a job: after each import for the new models, and from **Make previews** on Home for any model without one. Files over 500 MB are skipped.

**Pictures** show as a gallery (click for full size, arrows to step). **Documents:** PDFs open inside the app where the system's web view can show them, with **Open in default app** always there; Markdown and text readmes are shown as formatted text (raw HTML in them is shown as text, never run). **Videos** play in the page; the `library://` protocol (and `serve`) answer range requests so seeking works without reading whole files.

**Set as cover** on a picture (or on the 3D view, which saves a snapshot to `_media/cover.png`) writes the model.json `cover`.

## Phase 3 notes

Built 2026-10-06, as designed above, with these details:

- **Core:** `mesh.rs` (STL binary and text, OBJ, 3MF with components in other files via `p:path`, component and build transforms; `to_stl` for the page), `archive.rs` (ZIP listing without `__MACOSX` and `._` files; reading one entry with a size limit), `thumb.rs` (`pick_main`, `make`, the renderer: 800×600 drawn, averaged down to 400×300, PNG written with flate2), `docs.rs` (pulldown-cmark; HTML events become text; links other than http(s) lose their target). Commands: `model_zip {id, file}`, `model_mesh {id, file, entry?}` (binary STL), `model_entry {id, file, entry}` (bytes), `model_doc {id, file}` (`{html}`), `model_cover {id, file | snapshot}`, `thumbs_make {ids?, force?}` (a job). `model_get` gains `main` and `has_thumb`.
- **Cover order:** a picture in the folder still wins over the drawn preview (`index.rs` uses `_thumbs/model.png` only when the model has no cover otherwise). So a model imported with its own photo keeps it on its card; the preview is there for the many models without one.
- **Variants** are matched on the folder name with spaces, dashes and case ignored (`presupported`, `supported`, `withsupports`, `supports`; `unsupported`, `nosupports`, `nosupport`, `withoutsupports`). The supported variant is chosen first; **All** shows everything. The part tree opens two levels (a variant folder, then its parts).
- **Readme links** are caught by the page and opened in the browser (the window's navigation guard from Phase 0 is the backstop).
- **Range requests:** `App::library_range` serves both the Tauri protocol and `serve`; an open-ended range returns at most 8 MB, which is what video elements ask for.
- **Tests:** `api.rs` checks meshes, ZIP entries, readmes, previews and ranges; `desktop_page.py` checks 19–24 (preview on import, the model page with variants, a ZIP's part, covers and a readme, ranges, Make previews). Headless Chromium needs `--enable-unsafe-swiftshader` for WebGL.
- Not done: textures and 3MF colours in the viewer and previews; slicer files (G-code) in 3D; nested archives; 7z and RAR.

## Phase 4 design

Written before the code (2026-10-06). Goal: change categories after the fact, with the folders on disk following, safely. Done when renaming a faction moves its folders and can be undone.

**One engine: re-layout.** Every change that moves model folders is planned the same way: work out, for each model affected, its new category values and its new folder; show a preview; then run it as a job. The kinds of change:

- **Rename, merge or move a category value.** From a category's page (the browse header), **Rename or move…** edits the values down to it (Warhammer 40k › Tyranid). Renaming Tyranid to Tyranids renames it; renaming it to a value that's already there (Tyranids) merges the two; changing a value higher up moves it (Tyranid under another game). Every model below it gets the new values.
- **Edit a category** (the schema itself): its name, top folder, levels (rename a label, add a level with a value for the models already there, remove a level, reorder), the model-folder template (with an option to rename existing model folders to match) and its fields. Renaming a level's label or editing fields moves nothing; changing the top folder, levels or (with the option) the template re-lays out its models.
- **Delete a category**: its models go to Unsorted (their folders keep their names), then its schema file is removed.

**Preview.** Before anything moves, the dialog shows how many models move, a sample of from → to paths, and any name clashes (a model folder that would land on an existing one gets " (2)", as in importing).

**Safety: a journal.** A re-layout writes its plan to `_library/journal/<id>.json` before the first move: every model's old and new folder, schema and category values, and the schema file before and after. Each move is a rename on the same drive (a checked copy otherwise, as in importing); each moved model's `model.json` gets its new category, and a model without one gets one, so its id (and its star) survives the move. Progress is read back from the disk (a move is done when its new folder exists and its old one doesn't), so the journal isn't rewritten for every model. Category folders left empty are removed.

- **Undo**: **Recent changes** on Home lists the last changes; the newest can be undone, which moves every folder back, restores the category values in `model.json` and the schema file as it was.
- **Interrupted** (the app closed or a move failed partway): Home says so, with **Finish it** and **Put things back**.

**Several models' details at once.** With several models picked (Ctrl or Shift click), **Edit details…** adds or removes tags, and sets authors, licence or a category field, for all of them.

Not in this phase: editing a single model's folder name from the app, merging two schemas, undo for anything older than the newest change.

## Phase 4 notes

Built 2026-10-06, as designed above, with these details:

- **Core:** `relayout.rs` (`plan`, `summary`, `start`, `apply`, `undo`, `list`); `schema::edited` turns the edit form into the new schema file and says where each new level's values come from (an old level, or one value for every model); `schema::save` and `schema::remove`. Commands: `relayout_plan {change}` (the preview), `relayout_apply {change}` (a job), `journals`, `journal_undo {id}`, `journal_finish {id}`, `models_update {ids, patch}`.
- **The journal** holds the whole plan (each model's id, old and new folder, schema and category values, and the schema file before and after) plus `state` (running, done, stopped, undoing, undone) and `direction`. Journal ids are model-style time-ordered ids, so the newest sorts first; the 20 newest are kept.
- **Which models are in a change:** a model whose folder and category don't change (a level's label or a field edited) is left out, so editing labels or fields writes only the schema file.
- **Moves** use import's `destination` (clean folder names, " (2)" on a clash) and `transfer` (a rename, or a checked copy across drives). Every moved model gets a `model.json` if it had none, so its id and star survive; `model::set_place` records its new category. Emptied category folders are removed; an old top folder goes when it's empty.
- **Undo** is only for the newest change that isn't undone. "Finish it" after an interruption runs the same plan again, skipping models already in place; for an interrupted undo, Home offers to finish undoing or make the change again.
- **Bulk details** (`models_update`): tags added and removed case-insensitively; authors and licence replace; the category's fields are offered only when every picked model is in the same category.
- **Tests:** `relayout.rs` (merge with a clash, undo, schema edit with a new top level and renamed folders, only-newest undo, delete), `api.rs` (rename, undo and bulk edit through the commands), and page checks 25–29.
- Not done: undo of older changes, merging two categories (schemas) into one, renaming one model's folder by itself.
- **Subcategories made in the app** (owner's request after Phase 4, 2026-10-06): a schema file can hold `subcategories`, a nested list of `{name, subcategories}`, for category values made in the app before any model is in them. Their folders are made at once and kept when the last model leaves (`schema::kept_folder` stops the tidy-up); the overview lists them with a count of 0. Commands `subcategory_add {schema, path, name}` and `subcategory_remove {schema, path}` (only when nothing is in it). A re-layout maps them like models: a rename or merge moves their paths, a schema edit maps them through the new levels, and the folders follow (`relayout::sync_folders`).
- Interface wording is general: no wargaming examples (owner's request).

## Subcategory tree design

Written before the code (2026-10-06), after the owner tried v0.4.1: a category's subcategories should be a free tree, built in the New and Edit category dialogs, where each branch can go as deep as it needs ("Home items" › Office › Desk items, Computer models; "Home items" › Kitchen; "Home items" › Garage). Named levels (Game, Faction) go: they forced every branch to the same depth.

- **The schema file holds the tree**: `subcategories`, a nested list of `{name, subcategories}`, is every subcategory of the category, at any depth. A model can sit at any node, including the category's top folder.
- **Where models are**, under a category's folder: a folder with a `model.json` is a model; otherwise a folder in the tree is a subcategory; otherwise a folder with model folders inside it is a subcategory (moved there by hand); otherwise a folder with files is a model (dropped there by hand); an empty folder is skipped.
- **Older categories** (with `levels`) are converted when a library that can be written is opened: every folder at those levels becomes a node in the tree and `levels` is dropped. Nothing moves. A library that can't be written is still read by its levels.
- **`model.json`** records `"path": ["Office", "Desk items"]` for its place (it was `"category": {level: value}`, still read when importing older files).
- **New category**: name, top folder, a subcategory tree editor (add a subcategory at the top or inside any node; Enter adds the next one beside it; remove a node with what's inside it), model folder names and fields.
- **Edit category**: the same editor, filled from the library (with each node's model count). Renaming a node, moving it to another parent (indent and outdent) or removing it moves the folders, through the Phase 4 re-layout (preview, journal, undo). A removed node's models move up to the nearest node that's kept. Each node from the library carries its original path, so the core can map every model's old path to its new one.
- **Choosing a place** (Import, Move to category): the category, then a subcategory from a list of the whole tree (or the top of the category), and an optional new subcategory to make inside it (a "/" makes several levels at once).
- **Rename or move…** on a subcategory's page: a new name and a new parent anywhere in the tree (not inside itself). A name that's already there merges the two.
- **Add subcategory…** works on every node, at any depth.
- **Search**: `in:office` finds models with Office anywhere in their path.

## Subcategory tree notes

Built 2026-10-06, as designed above, with these details:

- **Core:** `schema.rs` has the tree (`Schema::tree`, `subcategories`/`set_subcategories`), the form's tree (`tree_spec`: nodes with `orig` map their old path to the new one), `Remap` and `remap` (the longest matching old path decides; a removed node's paths collapse onto the nearest kept node above it), `as_tree`/`upgrade`/`upgrade_all` (older schemas), `define_path` (an import or move adds its path to the tree) and `check_path` (a path can't run into a model's folder). `index::find_models` follows the rules above, with `probe` looking up to four folders down for model folders. `model::set_place` writes `path`; `model::path_of` reads `path` or an older `category`.
- **Commands** keep their shapes: `schema_create {schema: {name, folder, subcategories, model_folder, fields}}`; `relayout_plan`/`relayout_apply` with `{kind: "category", from, to}` (paths of any length) or `{kind: "schema", spec: {…, subcategories: [{name, orig?, subcategories}], removed: [[path]]}}`. A spec without `subcategories` keeps the tree as it is. Journal moves record `path_before`/`path_after`; older journals' `category_*` are still read for undo.
- **Older libraries:** opening a library that can be written converts its schemas (`App::open_library` calls `schema::upgrade_all`). Undoing a change made to an older schema writes the older schema back, levels and all.
- **Interface:** `category.js` has `CategoryPicker` (category, subcategory list with full paths, new subcategory box) and `SubcategoryTree` (the editor; a new node gets focus as soon as it's drawn so typing straight on lands in it). Unnamed new nodes with nothing in them are dropped when saving.
- **Tests:** `relayout.rs` (merge across depths with a clash, undo, a tree edit with renames, a move, a removal, a new branch and a new top folder, delete; an older schema moved across depths and undone), `schema.rs` (remapping), `import.rs` (any depth, the top of a category, a model's folder refused), `api.rs` (converting an older library on open, `in:` search, subcategories at any depth), and page checks 9, 15–18 and 25–28b reworked for trees.

## Phase 5 design

Written before the code (2026-10-06). Goal: a whole NAS share of models gets into the library. The owner's direction for the first part: "import a folder as is and provide a UI to go through and quickly apply categories and subcategories to each item shown in its existing folder structure", with several views, single or multiple selection, sending items to a subcategory, grouping files into one model, and several views of a model's parts. The duplicate report and noticing outside changes are as planned.

**1. The sorting workspace (the Import page).**

- **Sort a folder…** reads the whole folder tree (a job with progress, as a share can be large) and shows it as it is on disk. Inside it the app proposes which folders and files are models, all the way down:
  - a folder with a `model.json`, or with 3D, slicer or archive files of its own, is a model, and its sub-folders are its parts;
  - a folder whose sub-folders are all variants (Presupported, Resin…) is a model;
  - loose 3D, slicer and archive files are models, grouped by name with the pictures and documents whose names start the same way (as in Phase 2);
  - any other folder is just a folder, and its contents are looked at the same way;
  - files that belong to no model are shown greyed; they can be grouped into one.
- **Views:** *Folders* (the tree as on disk), *List* (every model in a table: name, folder, files, size, where it goes; sortable, with a filter), *Grid* (cards with a preview of each model's main 3D file or picture) and *By category* (grouped by where each will go, "Not sorted yet" first). A filter for To sort, Sorted, Skipped and Imported, and a search by name.
- **Selecting:** click, Ctrl- or Shift-click, or tick; ticking a folder ticks everything in it.
- **Actions on the selection:**
  - *Send to…*: a category and subcategory (or a new one), or Unsorted. For a folder, *Keep its folders as subcategories* makes the folder and the folders inside it subcategories below the chosen place, so a tidy tree comes in as it is.
  - *Group into one model*: the selected models, folders and files become one model (folders become its part folders, files go at its top), named after the folder they're in.
  - *Make one model*: a folder that was read as several models becomes one (its contents become parts).
  - *Split*: a model becomes a model per sub-folder and per group of loose files.
  - *Ungroup*, *Skip*, *Clear*, and name, author and tags.
- **Details pane:** the focused model's files in the parts views (below) and its main 3D file or picture, to recognise it before sorting it.
- **Import sorted** moves (or copies) every model that has a place into the library, writing its `model.json`, and marks it imported. Import some, carry on sorting, import more. The session is saved on this computer (the app's data folder, one per library) and comes back when the Import page opens; *Read again* picks up changes in the folder and keeps what was decided, by each item's path.
- *Add a model folder…*, *Add a file…* and dropping on the window add to the same workspace.

**2. Views of a model's parts** (the model page and the workspace's details pane): *Folders* (the part tree, with the variant switch), *All files* (one list: each file's folder, kind and size, sortable, with a filter), *By type* (3D models, slicer projects, pictures, documents, videos, archives, other) and *Grid* (a preview of every 3D file and picture). The choice is remembered. Previews of single files are drawn by the core's renderer and cached in the app's data folder, not in the library.

**3. Duplicates.**

- *Find duplicates* is a job. Files of the same size are compared by a quick fingerprint (the size and the first and last 64 KB), then those that still match by SHA-256. Hashes are kept in each model's `model.json` (`hashes: {file: {size, modified, sha256}}`), so a file is hashed once until it changes.
- The **Duplicates** page lists models whose files are all the same as another model's, and files found in more than one model.
- *Set aside* moves an extra copy's folder to `_library/set-aside/`, out of the library's lists. It's journalled like a re-layout, so Recent changes on Home can undo it. *Delete set-aside copies…* empties that folder after asking.
- Import's duplicate warning uses sizes, then the same fingerprints.

**4. Changes made outside the app.**

- A watcher on the library folder (the `notify` crate) notices changes made in Explorer or a file manager. When they've settled (2 s) the index is read again (only changed models are re-read) and the page refreshes.
- Network shares don't always report changes, so the library is also checked when the window comes back to the front and every few minutes.
- A model folder moved by hand keeps its id (it's in its `model.json`), so its star and details follow it, and its `model.json` is told its new place.

**Done when:** a folder tree of models can be sorted into the library from one workspace, over more than one sitting; duplicates across the library are found and set aside with undo; and folders changed outside the app show up without reading the library again by hand.

Not in this phase: unpacking 7z and RAR, details fetched from websites.

## Phase 5 notes

Built 2026-10-06, as designed above, with these details:

- **The workspace** (`sort.rs`, page `web/ui/sort.js`): a session of roots (folders read), folders, items (`folder`, `files` or `group`, each with its sources, name, author, tags, place, skip, and once imported `done`) and loose files left over. It's kept in `<data dir>/sorting/<library id>.json`. Commands: `sort_get`, `sort_add {paths, contents}` and `sort_rescan` (jobs), `sort_send {ids, folders, schema, values, keep, keep_self}`, `sort_update`, `sort_group`, `sort_join`, `sort_split`, `sort_clear`, `sort_files`, `sort_preview`, `sort_commit {mode}` (a job, through import's `plan` and `commit_one`). The workspace can't be changed while its folders are read or its models imported.
- **What's a model**, in the walk: a folder with a `model.json`; a folder with 3D, slicer or archive files of its own, unless a sub-folder is plainly a model itself; a folder whose sub-folders are all variants or "parts by kind" folders (STL, Images, Renders…). Loose files are grouped by name with the pictures and documents that start the same way; anything else is left over. NAS clutter (`@eaDir`, `#recycle`, `#snapshot`, `$RECYCLE.BIN`, `__MACOSX`, `Thumbs.db`, hidden files) and links are skipped.
- **Keep folders as subcategories** adds the folder names between the picked folder (or the one below it, unticking "Starting with … itself") and each model to the chosen place; they're made as the models are imported.
- **Decisions carry over** when the folders are read again, by each item's path. A group's sources and imported items stay as they were.
- **Parts views** (`web/ui/parts.js`) are shared by the model page and the workspace; previews of single files are drawn by `thumb::file_preview` into `<data dir>/previews/` and served as `~preview/<name>.png`; workspace files are served as `~sort/<item>/<file>`.
- **Duplicates** (`dupes.rs`, page `web/ui/dupes.js`): files of 3D, slicer and archive kinds (not `_media/`) are compared. Files of one size in two or more models get the quick fingerprint (SHA-256 of the size and the first and last 64 KB; a file of 128 KB or less is hashed whole, so its fingerprint is its SHA-256); those whose fingerprints still match get a full SHA-256. A model whose every such file is found in another model, with the same list of contents, is a copy. Hashes go into `model.json` (`hashes: {file: {size, modified, quick, sha256}}`) when the library can be written; models without a `model.json` keep theirs in `<data dir>/duplicates/<library id>-hashes.json`. The last search is in `<data dir>/duplicates/<library id>.json`; the page shows it with models no longer in the library dropped. Commands: `dupes_find` (a job), `dupes_get`, `dupes_set_aside {ids}` (a job), `dupes_empty`.
- **The copy to keep** is suggested: one in a category before one in Unsorted, then the one with more files, then one with a `model.json`, then the shortest folder path.
- **Setting aside** is a journal of kind `set-aside` run by `relayout::apply` (categories untouched); each folder goes to `_library/set-aside/<where it was>` and keeps its id (`keep_id`: a `model.json` made on the way uses the model's id), so undo brings it back as it was. *Delete set-aside copies* removes the folder and marks those journals `emptied`, which can't be undone or finished.
- **Watching** (`watch.rs`, the `notify` crate): the library folder is watched recursively; changes under `_library/` and hidden files are ignored. After 2 s with no more changes, the index is read again (from the cache, so only changed folders are read) and models holding a changed path are read again too (a file added deep in a part folder doesn't change the model folder's time). A model whose `model.json` names a different place than its folder gets `model::set_place`. The check waits for any running job. The page asks `library_changes` every 4 s for a change counter, and `library_changes {check: true}` when the window comes back to the front (at most every 30 s) and every 5 minutes; a change reloads the lists.
- **Tests:** `sort.rs` (reading a tree, send, group, join, split, keep choices, files of a group), `dupes.rs` (copies found, keep order, hashes kept, a second look reads nothing, set aside, undo, delete), `watch.rs` (one report for a burst, `_library/` ignored), `api.rs` (sorting over two sittings, duplicates through the commands, folders changed outside the app), and page checks 15–19 (rewritten for the workspace), 22b (parts views) and 30–32.
- Not done: undo of an older set-aside, choosing which files decide a duplicate, a model without a `model.json` keeping its id when moved by hand (it gets one from its new path).

## UI pass design

Written before the code (2026-10-07). Phase 6 is on hold. The owner's direction: "The UI still needs work… I think actions need to be consistent across the UI." The review of every page (about 120 buttons, links, tick boxes and keys) found the same action named, placed and undone differently from page to page: Move to category in five forms, Edit details in three, Show in folder in three and missing from three places, four kinds of undo, three view switchers and three sort controls, and four labels on Import for "make one model". The owner chose to do all three steps below, each released as 0.5.x with screenshots.

**The rules every page keeps.**

1. One name, one icon, one key per action, on every page (the word list below).
2. Selecting works the same everywhere: click selects one, Ctrl or Cmd-click adds or removes one, Shift-click selects a run from the last one clicked, tick boxes show on hover and stay once anything is selected, Ctrl+A selects everything shown, Esc clears, arrow keys move (with Shift, extend). Selected things get the same amber highlight.
3. One action row and the same right-click menu. The same buttons in the same order head the details panel and the model page: **Open, Edit details…, Move to category…, Star, Show in folder, More ▾**. A page's own actions follow, or go under More. Right-clicking a card, row, file or sidebar category shows the same actions as a menu. An action that doesn't apply is left out; one that can't be used now (a read-only library, nothing selected) stays visible, greyed, with the reason in its tooltip.
4. One details panel (step 3): picture, name, authors, category, actions, files, alike in the library, on Import and on Duplicates.
5. Every change can be undone the same way: the message that confirms it has an **Undo** button, and Ctrl+Z undoes the last change made in this window. Older changes are in Recent changes on Home.
6. Ask only when it can't be undone: one in-app confirm with a red button that names what it deletes. A change that moves many folders lists the moves first.
7. Long work looks the same: progress and **Stop** on the page that started it and in the status bar.
8. The same controls for views, sorting and paging: one view switcher (icon, label, tooltip), one sort menu with the same words, one "Show more (N left)".
9. Every icon-only button has a tooltip; a greyed button says why.

**Words.**

| Use | Instead of |
| --- | --- |
| Move to category… (Set category on Import, where nothing moves until Import) | Move…, Send to, the unlabelled icon |
| Unsorted | Not sorted, Not sorted yet |
| No category, Has category (Import's filters) | To sort, Sorted |
| Clear category | Not sorted (the button) |
| Combine into one model; its opposite is Split | Group into one model, Make it a model, Make one model, Make it one model, Ungroup |
| Select, Clear selection | Pick, Pick nothing |
| Star, Starred (the sidebar entry) | Favourites |
| Authors | Author (Import's field) |
| Stop (a running job) | Cancel (Cancel only closes a form) |
| Read again (Duplicates keeps Look for duplicates) | Read the folders again, Check again, Look again |
| Delete… (red, only for what can't come back) | mixed colours; Set aside stays for duplicate copies, Remove for taking something out of a list |
| Show more (N left) | Show more, Show all N |
| Name, Recently added, Largest first, Folder, Kind | Largest, By name, By folder, By kind |
| List (a model's files) | All files |
| Search | Find by name, Filter files |
| Open another library… | Open or create another library… |

**Keys** (ignored while typing in a box or while a dialog is open): Enter opens, E edits details, M moves to a category (Set category on Import), S stars, F2 edits details with the name ready to type, Ctrl+A selects all shown, Esc clears the selection (or closes a menu or dialog), arrows move and Shift+arrows extend, `/` goes to the page's search box, Ctrl+Z undoes the last change, Shift+F10 or the menu key opens the right-click menu for the selection.

### Step 1: actions (0.5.1)

- **One list of actions** (`web/ui/actions.js`). Each action is `{ id, label, icon, key, applies(targets), enabled(targets) → true or the reason, run(targets) }`. The library's model actions, in order: Open (not on the model page), Edit details… (several selected: the form for several models), Move to category…, Star (Remove star when all are starred), Show in folder (desktop, one model), then under More: Make a new preview, Copy folder path. An `ActionRow` draws them as buttons (icon and label, the same everywhere) and a `ContextMenu` as a menu; both read the same list, so they can't drift. The details panel, the model page, the several-selected panel and right-click on cards, list rows and Duplicates' model rows all use it.
- **Import's actions** use the same row and order where they match (Edit details focuses the name box, Set category… focuses the category picker, Show in folder shows the source), then Import's own: Combine into one model, Split, Skip or Don't skip, Clear category, Use suggested categories. Right-click on a tree row, list row or card gives the same list.
- **A Category menu** (moved forward from step 3, because Delete needs a home): one **Category ▾** button on every category page and right-click on a sidebar category, with Add subcategory…, Edit category… (now at every level; it opens on the whole category), Rename or move… (a subcategory), and Delete category… or Delete subcategory (red). Edit category no longer has its own Delete button, so unsaved edits aren't thrown away. An empty subcategory is deleted straight away with Undo in the message (it's added back); one with models in it opens a dialog listing the folders that move up a level, as a category change that can be undone.
- **Selection.** The library keeps `picked` (the selected ids) and `selection` (the one last clicked, which the details panel shows and Shift-click runs from). Cards and rows get a tick box. Ctrl+A, Esc, arrows and Shift+arrows work in the grid and list. Import adds Ctrl+A, Esc anywhere on the page and arrow keys; Duplicates adds Ctrl+A and Esc on its group tick boxes. A selection is dropped when it isn't in the results shown (bug 1).
- **Keys** through one handler in the shell; each page registers what its keys do.
- **Messages with Undo.** `toast(text, { ms, action: { label, run } })` draws a button in the message, and `undoable(text, undo)` shows the message and puts the undo on a stack that Ctrl+Z takes from (newest first, about 20 kept, cleared when the library changes). In this step: star; edit details (the old values are saved back); category changes and setting copies aside (the change's journal is undone, as from Home); deleting an empty subcategory; variant names and the library name (set back); and every Import action, through a new core command `sort_undo` that puts back the workspace as it was before the last change (the core keeps the last 20 workspaces in memory; importing clears them, since files have moved). Reading a folder in, Clear imported and Start again can then be undone too, so Start again no longer asks. Move to category, Import, the full edit form for several models and Use as cover get undo in step 2.
- **Bugs fixed:** the details panel showing a model from another place; Delete category drawn in yellow (now red); Delete inside Edit category losing unsaved edits; deleting an empty subcategory without undo; files with no viewer looking clickable (now greyed with a tooltip, and Open in its own app on the file's menu), and videos that play only in another app getting an Open button; author and tag filter chips that couldn't be taken off (click again to remove); the theme button not going back to Follow the system (it now goes System, Light, Dark, Night); Starred without a count (the overview counts starred models); Read again on Home not saying when it fails; messages covering Import's footer.

### Step 2: undo and asking (0.5.2)

- **Journals for every change**, as category changes have now, so each shows in Recent changes on Home and has Undo in its message:
  - *Move to category*: a journal of kind `move` (a list of moves, as a category change has), applied and undone by `relayout`. A new subcategory made on the way is in the journal's categories before and after, so undo removes it again.
  - *Import*: each model imported is a move from outside the library (`from` absolute) to its place, with the mode. Undo moves the files back where they came from (move) or deletes the copies (copy), and marks the workspace items as not imported.
  - *Edit details*, for one or several models: a journal of kind `details` holding each model's `model.json` before; undo writes them back.
  - *Use as cover*: the old cover is kept in the journal's folder and put back on undo.
  - *Delete subcategory* with models in it (through Edit category): a category change already.
- **Move to category shows its moves first**, with the same list of folders as category changes, and **Move** says how many models move.
- **One red confirm** (`ConfirmDialog`) for what can't be undone: Delete set-aside copies uses it instead of the panel on the page, and the button names what it deletes.
- **Stop on every long job**: the status bar says Stop, and reading or opening a large library can be stopped (the index keeps what it read and finishes next time).

### Step 3: layout (0.5.3)

- **One page header**: title and count on the left, the page's main button on the right, other page actions under More. Under it **one toolbar**: search, filters, sort, view.
- **One details panel** for the library, Import and Duplicates: picture, name, authors, tags and category, changed right in the panel (saved when you leave the box, with Undo in the message), the action row, then the files. Several selected shows the count, the shared values and the same row. With nothing selected, the panel sums up the place shown: models, size, authors.
- **Home** becomes the place to start: Recent changes, folders waiting to be sorted, recently added models, and one **Library ▾** menu for Show in folder, Read again, Make missing previews and Open another library…. "How the library is laid out" and "What's coming" move to Settings, under About.
- **One view switcher** (icon and label) on the library, the model page and Import, and **one sort menu**; the library's list view gets column headers that sort when clicked, as on Import.
- **"Searching for … ×"** above the results when a search carries into another place, so an empty category doesn't look broken.

**Done when:** the same action has the same name, place, key, menu entry and undo on every page; every change except deleting for good can be undone from its message or Home; and the page tests check each of these on every page.

## UI pass notes

### Step 1 (0.5.1)

Built as designed, with these differences and lessons:

- `web/ui/actions.js` holds the model actions (`MODEL_ACTIONS`), `ActionRow` (buttons with ids `<prefix>-<action>`, plus `<prefix>-more`), the right-click menu (`openMenu(at, items)`, drawn once in `#menu-root`) and the keys (`onWindowKey` in the shell, `usePageKeys(fn)` for each page). Items in a menu are `{ id, label, icon, key, danger, disabled: reason, run }` or `{ sep: true }`.
- On Import, Edit details puts the cursor in the details pane's name box and Set category… in the category picker, rather than opening dialogs: Import's pane already edits them. The message for Set category says nothing moves yet: "Set 3 models to go to Household. Nothing moves until you press Import."
- An undone change says "Undone: " and the first clause of its message ("Undone: moved Benchy to Office").
- `sort_undo` keeps the workspaces in memory only, so they're gone after a restart; importing and opening another library clear them. `sort_view` says which change can be undone (`undo`, a sequence number); an older one is refused.
- Deleting a subcategory with models in it is a category change with a `label` ("Deleted the subcategory Ruins") so Recent changes names it.
- Dialogs and page keys are set up in layout effects, not after the next paint: Esc pressed just as a dialog opened was missed, and the first box didn't have the cursor yet. A dialog puts the cursor in its first box (or on itself when it has none).
- The Import page says "Reading the workspace…" until its workspace has loaded; before, it showed the empty workspace for a moment (and the tests could act on it).
- Files with no viewer are greyed by a class and a tooltip, not `aria-disabled`, since they can still be right-clicked and opened in their own app.
- Still as before until step 2: Move to category is undone by moving the models back (no journal yet), and the edit form for several models has no undo.
- Tests: `ui_pass` in `desktop_page.py` (checks 33 to 39: the row, menu and More on every page, keys, the Starred count, no selection from another place, filter chips, the theme cycle, the sidebar's Category menu, greyed files and the file menu, Import's menu, keys and Ctrl+Z, and messages clear of Import's footer). 47 checks in all.

### Step 2 (0.5.2)

Built as designed, with these differences and lessons:

- **Three new journal kinds** in `_library/journal/`, next to `category`, `schema`, `delete` and `set-aside`:
  - `move` (Move to category): planned by `relayout::plan_move` and applied like a category change. Each move has `keep_id`, so ids, stars and details stay with the models. `added` lists the subcategories made on the way; undo removes them again only when nothing else is in them by then. Moving models to where they are already is refused ("It's there already: choose another place.").
  - `import`: written by `sort_commit` after the files have moved, with `mode` and, for each model, where it came from (`source`, absolute), its files, its workspace item and what its `model.json` and preview were before. Undo moves the files back with the same SHA-256 checks as importing. For a copy, it deletes the copies, but only when the originals are still there; otherwise it moves them back instead. The workspace items are then marked as not imported (`Session::unmark`) and the workspace's own undo list is cleared.
  - `details` (Edit details for one or several models, Use as cover): each model's `model.json` before and after. A model without a `model.json` gets one first (the same defaults the first save writes), so its id doesn't change between the change and its undo. A replaced cover picture is kept in the journal's folder (`_library/journal/<id>/`), which is pruned with the journal.
- **Which change can be undone:** changes that move folders are still undone newest first. A details change only waits for newer changes to the same model folders, so Home shows Undo on every row and greys the ones that are waiting, with the reason as the tooltip ("Undo the newer change first: …"). `journals` returns `undo: true` or that reason.
- Saving a model's details for the first time gives it a `model.json` and so a new id; the commands return the new ids (`id`, or `ids` as old → new for several) and the page follows them, as it does for stars.
- Move to category shows its preview (the same `ChangePreview` as category changes) and its button says how many models move. The old core command `models_move` is still there (and in the core tests) but the page no longer uses it.
- Import's results say what can be undone and have an Undo button as well as the message.
- **Read again** is a job with Stop. Stopping keeps the index as it was before, rather than half of the new one, and the message says so. Opening a library for the first time (the first read) can't be stopped yet.
- Boxes that copy a value into their own state (Import's name, authors and tags, the search box, the files view's tab and picked file) do it in a layout effect. Preact runs plain effects up to a frame after drawing, so text typed in that moment was put back (the cause of the page test's rare "Hinge and latch" timeout).
- Delete set-aside copies… asks in the one red `ConfirmDialog` (`confirmDialog({ title, text, button, run })`), which says it can't be undone.
- Tests: `ui_pass2` in `desktop_page.py` (checks 40 to 44: Move's preview and Undo, Edit details undone with Ctrl+Z, Import undone from its results, an older details change undone from Home after a newer one, and Read again's message), and `moves_imports_and_details_are_journalled_and_undone` in the core. 52 page checks in all.

### Step 3 (0.5.3)

Built as designed, with these differences and lessons:

- **Shared pieces in `web/ui/layout.js`:** `PageHead` (title, count, a line under it, the page's main button and one menu), `MenuButton` ("Label ▾", items made when it's pressed), `ViewSwitch` (icon and label; the label goes in a narrow window), `SortMenu` ("Sort: Name ▾", items `sort-<id>`), `SearchNote` and `EditBox`. Every page now starts with `PageHead`: Home, the library, a model's page (Back, the name, then its place under it and the action row on the right), Import, Duplicates and Settings.
- **The page's menu is named for what it holds** rather than always More: **Library ▾** on Home, **Category ▾** or **Subcategory ▾** on the library (the sidebar's category menu, so it's on the page too), **More ▾** on Import.
- **The details panel (`details.js`):** `ModelDetails` for one model (the library and Duplicates), `SelectedPanel` for several, and `PlaceSummary` when nothing is selected: the place's name, how many models and their size, and its main authors and tags. `models_query` returns `bytes` (the size of every hit, not just the page shown) for it. Name, authors and tags are `EditBox`es: saved when you leave the box or press Enter, put back with Esc, with "Authors" and "Tags" beside them. Saving goes through the same `details` journal as Edit details, so the message has Undo.
- **Import keeps its own panel** (its items aren't in the library yet, so the edits go into the workspace and its own undo), but it's drawn the same way: the same boxes, then Import's actions (the category picker, Set category, Edit details, Show in folder, Split, Skip) in the panel rather than in a bar above the list. With nothing selected it sums up the workspace. Its filters (No category, Has category…) are a row under the toolbar; the Folders view's Unfold and Fold all stay in the toolbar in place of the sort menu.
- **Duplicates** has the same panel beside its groups: click a model to see it, double-click or Enter to open it, E and M as on the library.
- **Home** shows Recently added (six, newest first, with See all), Waiting to be sorted (Unsorted and folders outside any category, with Sort…), Categories and Recent changes, in a two-column grid that becomes one in a narrow window. How the library is laid out and What's coming are in Settings › About.
- **The search note** names the place and how many were found there, and offers Search all models when the place isn't All models. The note and the place summary use only the answer for the place shown: while the next place's answer comes, the last one stays on screen, and the note briefly said "found in All models" over Household's empty list.
- Settings' section headings were the same size as the page title; they're smaller now.
- Tests: `ui_pass3` in `desktop_page.py` (checks 45 to 50: one header on every page, the view switcher, the sort menu and the list's headers, the search note in another place, editing in the panel with Undo, the place summary and what several share, Home and Settings › About, and the same panel on Duplicates and Import). 58 page checks in all.
