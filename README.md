# Model Library

A desktop app for organising large collections of 3D model files: thousands of models, terabytes of STL, 3MF, slicer projects and more.

- **Schemas lay out the folders.** A schema such as *Wargames: Game > Faction > Model* decides where an imported model goes, so a Hive Tyrant lands in `Wargames/Warhammer 40k/Tyranid/Hive Tyrant (Author Name)/`. The library makes sense in any file manager, with or without the app.
- **Details travel with the files.** Each model folder keeps a `model.json` (author, release date, source, tags…) and its pictures, PDFs, videos and readmes, so a library can be moved, synced or opened by a newer version of the app.
- **Desktop first, Docker later.** Windows and Linux builds now; the same backend will run as a server.

The design and the phased plan are in [docs/PLAN.md](docs/PLAN.md). Built on the app shell and interface of [Claude Grid Workshop](https://github.com/DrFlGd/Claude-Grid-Workshop).

## Status

**Phase 2:** importing. Sort a messy folder (or drop folders and files on the window) into categories, moved or copied with every copy checked; move models between categories; browse, search and edit their details. Viewing models and their parts comes in Phase 3.

Installers are on the [releases page](https://github.com/DrFlGd/Model-Library/releases) once CI publishes one (Windows: the `-setup.exe`, unsigned, so SmartScreen asks once; Linux: the `.deb` or `.AppImage`).

## Building

- `desktop/core`: the Rust core (library folder, commands, `modlib-cli`). `cargo test -p modlib-core` in `desktop/`.
- `web/`: the interface (Preact + htm, no build step).
- The app: `python3 tools/build_desktop.py --out build/desktop`, then `npx @tauri-apps/cli@2 build` in `desktop/`.
- The page against the real backend, in a browser: `modlib-cli serve --ui build/desktop/ui --home /tmp/home`, then `python3 tests/desktop_page.py …` (see its header).
