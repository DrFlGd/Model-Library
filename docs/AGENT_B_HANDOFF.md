# Agent B handoff — model and file operations

**Design source:** Model Library agent implementation requirements v1.0, 9 October 2026.  
**Ownership:** B1, B2, B3 and B4; related shared mutation contract extensions (B owns, C/D/E consume).  
**Branch:** `agent-b-model-file-ops-20261009`  
**Base:** `6eef962c7c210a64353891e2999ba871025b128b` (`main`, 0.5.5)  
**Review:** [draft PR #6](https://github.com/DrFlGd/Model-Library/pull/6); no merge or release.

## Requirements addressed

- **B1 — full-selection extraction:** file selection has explicit visible-files, entire-model, this-level and recursive-folder scopes. Complete model selections are now accepted; overlapping parent/child selections are normalised before planning. Preserve path structure, copy original contents on Copy, and retire an empty source model into journal recovery on full Move. New model IDs are distinct; known and unknown descriptive fields are inherited; Undo restores the former source model and folder hierarchy.
- **B2 — Send to another model:** file-level context and action bar offer searchable target model selection across sorted and Unsorted models, Move/Copy, existing internal destination folders, and a preview of files, bytes and paths. Name conflicts offer Cancel, Keep both and Skip with Windows case-insensitive checks. ZIP entries can be copied out; moving individual entries is rejected explicitly. The target model keeps its ID and metadata; undo restores sources and removes only operation-created targets.
- **B3 — Add files:** discoverable model-page Add files action launches a native multi-file picker without extension restrictions. Users review destination root/internal folder and conflicts before Copy. Sources stay unchanged and the target remains the same model. Undo removes additions after hash validation.
- **B4 — recoverable model deletion:** Delete model(s) in the same shared model action system, with a detailed confirmation. Complete folders including model.json, covers and generated thumbnails move to `_library/journal/<journal-id>/models/`. An SHA-256 source manifest guards execution and restoration; bulk Undo checks all target locations before making changes. No recovery expiry or permanent delete is introduced.

## Command and data contracts

- `model_extract_plan({id,files,entries,name,schema,values})` and `model_extract({...,mode})`: existing contract, now accepts `files:[""]` for entire model; root and parent/child overlaps are expanded safely. Full Move may retire the source model.
- `model_files_plan({kind:"send",id,target,files,entries,folder,mode,conflict})` or `model_files_plan({kind:"add",target,sources,folder,conflict})`: returns an operation ID, source/target IDs and paths, file manifest (`from,to,sha256,bytes,entry`), collisions, source/destination sidecar snapshots, total counts/bytes and cleanup instructions. `conflict` is `cancel` (default), `keep_both`, or `skip`.
- `model_files_apply({action,review})`: reconstructs the current plan, compares relevant manifests and metadata against the user's exact review and launches a cancellable journalled job. Result includes `journal`, `count` and `refresh` IDs.
- `models_delete_plan({ids})`: returns model names, IDs, paths, count/bytes and complete hash manifests. `models_delete({action:{ids},review})` revalidates review, then launches a cancellable job with result `journal`, `deleted`, `refresh`.
- `journal_undo({id})`: existing entry point handles `file_transfer` and `model_delete` journals through the new domain rollback handlers; it still guards against newer dependent mutations.
- `pick_files({title})`: Tauri platform command (multi-select, unrestricted file types). Exposed to UI as `ctx.platform.library.pickFiles(title)`. It is not available in plain-browser mode.

These planners and journals extend the existing `relayout` and `extract` mechanisms, not a second transaction store. All file content is SHA-256 verified. Multi-file additions publish files individually after staging; **this does not promise whole-operation filesystem atomicity**.

## Changed files / integration hotspots

- Core: `desktop/core/src/extract.rs`, `desktop/core/src/fileops.rs` (new), `desktop/core/src/delete.rs` (new), `desktop/core/src/lib.rs`, `desktop/core/src/api.rs`, `desktop/core/src/relayout.rs`.
- Desktop: `desktop/src-tauri/src/main.rs`.
- UI: `web/platform-desktop.js`, `web/ui/fileops.js` (new), `web/ui/actions.js`, `web/ui/dialogs.js`, `web/ui/extract.js`, `web/ui/filesel.js`, `web/ui/filepanel.js`, `web/ui/contents.js`, `web/ui/modelpage.js`.
- Tests: `desktop/core/src/extract.rs` and new Rust unit tests in `fileops.rs` and `delete.rs`; `tests/desktop_page.py`; `tests/model_workspace_panel.py`.
- Documentation: `CHANGELOG.md`, this handoff.

**Cross-agent coordination:** A owns panel placement and size; keep selection-scope and Add/Send action controls when integrating its model workspace. C should use B's reviewed-file journal framework for archive cleanup, not rebuild mutation journalling. D can call these conflict/review contracts for imported file contents; E owns category relocation. F should invalidate cached automatic card previews whenever B's `refresh` IDs are reported. Explicit cover metadata is not overridden by B. All integration should preserve unknown `model.json` keys.

## Verification and failure paths

Unit tests authored: full-model root and overlapping selection, Move retirement, Copy and exact Undo; mixed external additions with originals preserved, folder transfer and last-file retirement, case/name conflicts, ZIP-entry Copy-only, destination interference, tampered Undo, and bulk model deletion with recovery conflict. UI acceptance checks authored for full-model extraction/Undo, visible/whole selection scopes, Add files button and searchable Send dialog.

**Execution evidence is to be taken from draft PR #6's Desktop CI checks**. This environment can edit the GitHub repository but does not have a working Rust toolchain or network access for a local clone, so local `cargo test`, app launching and original screenshots were not run here. The draft PR is not an integration approval. Capture actual screenshots on the CI Windows/Linux runners or in the integrator's desktop visual pass before merging.

## Design choices and limitations

- Default conflict action is Cancel; Keep both uses a `(2)`-style renamed filename and Skip omits colliding files. No unverified Replace mode is supplied.
- External Add always copies; individual ZIP entries are bounded to 512 MiB per read and may only be copied. Whole ZIPs transfer like ordinary files.
- Recovery remains in the journal with **no automatic expiry**. Transfer staging also preserves copies needed for source restoration, so capacity planning should account for temporary/recovery duplication on large libraries.
- File plans calculate full SHA-256 manifests synchronously and can be expensive for very large files; a future progress/cancellation surface for the *planning phase* is a possible improvement. The apply phase runs as a cancellable progress job. Interrupted publication that cannot be unambiguously assigned to this operation remains in an explicit stopped/recovery state instead of deleting an unrelated file.
- Generic journal replay is not supported for specialised file transfer/deletion journals; Undo/recovery is the supported action.
- CI and cross-platform integration outcomes, including screenshots, must be recorded before changing this draft PR to ready for merge.
