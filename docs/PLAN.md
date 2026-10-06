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
| 6 | Docker: server platform, auth, image | The same library browses from a browser on another machine |
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
