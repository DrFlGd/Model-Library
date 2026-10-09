# Agent E — category restructuring handoff

Design basis: `Model-Library-Design-Requirements(5).md`, requirements **E1** and **E2**, 9 October 2026. This is an isolated implementation for central code review. It is **not** merged into main and no release is authorised.

## Branch and base

- Repository: `DrFlGd/Model-Library`
- Branch: `agent-e/category-restructure-20261009`
- Base: `6eef962c7c210a64353891e2999ba871025b128b` (main at branch creation)
- Review: https://github.com/DrFlGd/Model-Library/pull/3 (draft)

## Delivered requirements and behaviour

- **E1**: Every nested subcategory's Delete action opens a reviewed choice:
  - **Move contents up one level** relocates direct models to the parent and reparents the removed node's children beneath that parent, retaining the deeper hierarchy.
  - **Move contents to Unsorted** relocates all descendant models to Unsorted and removes the selected subcategory subtree.
  - Top-level categories and the protected Unsorted destination cannot enter this subcategory removal flow. Both modes preserve model ID, descriptive metadata and internal files. The planner rejects user-owned loose files, unexpected folders, symlinks, vanished nodes and conflicting reparented child names instead of losing them.
- **E2**: A Merge categories… action is available from category and subcategory menus. The review supports two or more independent sources, including different categories/parents, merged into an existing target (including a selected source retained as target), a named new subcategory or a new top-level category. Overlapping child nodes require a deliberate Cancel / Merge matching children / Keep both with numbered names choice; all mapped child paths and model destination paths are shown. Model folder clashes get numbered non-overwriting names (including case-only Windows collisions). Invalid target-inside-source and ancestor/descendant selections are blocked.
- **Shared lifecycle**: Changes are planned against the actual index and filesystem, reviewed in the existing `ChangePreview`, and replanned when executing. They reuse `relayout::start/apply/undo` and the existing job/progress/cancellation, newest-first Undo and journal-based recovery. New multi-schema before/after snapshots are applied only after relocation succeeds; Undo reverses model moves and restores hierarchy. No independent transaction system was added.
- **Proposed category validation contract for D**: `restructure::validate_tree(paths: &[Vec<String>]) -> anyhow::Result<()>` checks proposed nested paths, missing parents, unsafe category names and case-insensitive duplicates without creating library folders; `restructure::plan(lib, ix, change)` checks a concrete existing library.

## Command/data contract

Existing commands `relayout_plan({change})`, `relayout_apply({change})`, `journal_undo({id})` are reused. The two accepted `change` forms are:

```json
{"kind":"restructure","operation":"remove-up","source":{"schema":"id","path":["Parent","Child"]}}
{"kind":"restructure","operation":"remove-unsorted","source":{"schema":"id","path":["Parent","Child"]}}
{"kind":"restructure","operation":"merge","sources":[{"schema":"a","path":[]},{"schema":"b","path":["Nested"]}],"target":{"schema":"a","path":[]},"child_conflicts":"rename"}
```

For merge, `target` can instead be `{"schema":"id","parent":["Existing"],"name":"New child"}` or `{"name":"New category"}`. Source `path:[]` represents the whole category. Existing target identity is retained. The internal journal contains `schemas:[{id,before,after}]` and ordinary `moves`; plan summary adds `nodes`, `files`, `bytes`, `child_collisions`, `node_mappings` and sample destination model paths.

## Changed files

- `desktop/core/src/restructure.rs` — independent domain planner, validation and core tests (new)
- `desktop/core/src/lib.rs` — module export
- `desktop/core/src/relayout.rs` — reuse of journal, multi-schema apply/Undo and preview reporting
- `web/ui/categories.js` — delete/merge actions, review dialogs and mapping previews
- `web/ui/dialogs.js` — dialog registration
- `tests/desktop_page.py` — adjusted existing deletion/menu checks and new dialog screenshots
- `CHANGELOG.md` — Unreleased entry
- `docs/AGENT_E_HANDOFF.md` — this handoff (new)

## Testing and screenshots

Core unit tests added in `restructure.rs` cover nested removal with Undo/ID preservation; Unsorted removal and unmanaged-file rejection; cross-category conflicting child names with explicit resolution; merge into selected existing target; merge into named new subcategory; merge into a new top-level category; case-only model folder collisions; and invalid ancestor/source cycles.

Frontend JavaScript syntax checked for the touched UI modules. `tests/desktop_page.py` now captures `18b-delete-subcategory-modes.png` and `18c-merge-categories.png` when the Linux acceptance suite executes. The project GitHub Actions draft-PR check is the authoritative Rust, acceptance, Linux/Windows and screenshot run; record its final outcome before integration. This environment has no local Rust toolchain and cannot resolve GitHub for a local clone.

## Design choices and limitations

- Unsorted is the existing destination, not another category.
- Child-name collisions are resolved globally per merge review (Cancel/Merge/Rename), not through a separate per-child manual mapping editor; review shows each planned path.
- Existing non-overwriting model-directory numbering is retained, now with portable case-only collision handling. There is no Replace mode.
- The underlying `relayout` operation commits model folders individually, not as a whole-tree atomic filesystem transaction. Stopped changes remain in the existing journal for Finish/Undo; users should not be told the entire tree is atomic.
- The existing journal retains its history limit and newest-first Undo rule. Coordinating with Agent B's planned shared mutation contract is an integration task. No new library format number, index identity scheme or migration is required.
- Run end-to-end failure/interruption and Undo checks on Windows and Linux before approving integration, especially if Agent B changes the relocation/journal contract.

## PR review follow-up (E-1 / E-2)

The 9 October review requested two changes, both now implemented in this branch:

- **E-1:** `validate_destination_node` checks *each* new or reparented destination category path against the original filesystem and pre-change schema tree. An occupied path is accepted only if it is a retained, existing category with matching exact spelling; model folders (with/without sidecars), unindexed/empty folders, symlinks and case-only aliases cannot become subcategory containers. This check is used by merge, remove-up, and selected/new targets before folder moves are planned. The error asks the user to resolve or rename the conflicting folder.
- **E-2:** Merge collisions with `child_conflicts: "merge"` adopt the exact full path of the already-existing target node, not the source spelling; the same canonicalisation is applied to an existing selected destination and a named-new destination's parent. Child mappings and `model.json` locations therefore use the target's case at all depths.

Added Rust regression tests for merge and remove-up against occupied model folders with sidecars, no-sidecar model folders, unmanaged empty folders and case-only aliases, without mutating the library; multiple-depth case canonicalisation; lower-case user-selected target paths; exact destination and reversible Undo. These tests are run by the draft PR's GitHub Actions checks.

The review's failed Linux report upload on the **original** head was a concurrent report-branch update and not a feature test failure. The latest review-fix commit has new CI results; verify the latest run before central integration.

