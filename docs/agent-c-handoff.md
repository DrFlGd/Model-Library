# Agent C handoff — PDFs, ZIPs and external opening

Base: main at 6eef962c7c210a64353891e2999ba871025b128b.
Branch: agent-c/pdf-archives-external. Not merged.

## Coverage

C1: PDF stage controls (page, zoom, fit), ordinary PDFs over the library protocol, PDF ZIP entries via bounded 64 MiB reads and cached browser Blob URLs. Loading and errors are visible. Embedded PDF renderer may handle page/zoom URL fragments differently on WebView2 and WebKitGTK.

C2: Model-level Compress to ZIP and Extract archive actions provide preview of paths, bytes and SHA-256 sources. Writes stage before publication; all resulting uncompressed content is reopened and hash-verified. After verification a separate, opt-in cleanup job moves exact source files to the existing journal recovery directory. The usual journal_undo first proves every source has an intact live copy or a SHA-256-verified recovery copy, and refuses to delete any output if the source set is incomplete; interrupted journals can be safely undone from Home. Journal retention protects unfinished work and directories containing recovery data. The live model.json and _thumbs remain outside compression; an explanatory _model-library/archive-manifest.json lives inside the ZIP but is skipped during extraction. Model identity/category are unchanged.

C3: Recognise .3ds and .max as model files; .7z is already recognised as an archive for import and external opening, but is NOT unpacked in-app. Explicit Open externally uses platform OS association; ZIP entries are temporary, bounded copies and users are told external edits do not update archives.

## Contracts and shared edits

- archive_plan: {id,action:compress|extract,file} -> file manifest, SHA-256, outputs and required space.
- archive_execute: {id,action,file,reviewed_plan:<full archive_plan response>} -> rechecks the exact reviewed source hashes, input/output paths, model identity and destinations, then launches a background job with verified outputs. Immediate remove_sources:true is rejected; cleanup is a separate request.
- archive_cleanup: {journal} -> a separate, only-after-verification job moving exact original files to recovery.
- model_pdf_entry: {id|sort,file,entry} -> bounded PDF bytes.
- model_external_entry: {id|sort,file,entry} -> managed {path,temporary:true}.
- Journal kind: archive-op; recovery path under _library/journal/ID/sources.

Shared files: desktop/core/src/api.rs, lib.rs, model.rs, relayout.rs, web/ui/actions.js, dialogs.js, parts.js, stage.js, contents.js, library.js, filetypes.js, web/styles.css and CHANGELOG.md. New isolated modules: desktop/core/src/archive_ops.rs, web/ui/archive-actions.js, web/ui/document-viewer.js.

## Design decisions, limitations and integration

ZIP only in-app; no nested archive traversal. Bound size to 8 GiB expanded and 20,000 files; PDF entry in-app limit is 64 MiB; external temporary copy limit is 128 MiB. No overwrite on extraction. Case-insensitive conflicts, unsafe paths, links and changed inputs are rejected. Managed temporary external copies expire at a subsequent opening after 30 days. Archive planning currently hashes synchronously at the API boundary; measure responsiveness for larger models before raising limits.

The system PDF engine, rather than vendored PDF.js, is used. The browser PDF viewer uses standard Fit/FitH fragment parameters and offers Open externally for ordinary and ZIP-backed files. Plainly corrupt ZIP PDFs are rejected before creating the frame. Native PDF rendering, page/zoom support and resize screenshots **must still be checked on Windows WebView2 and Linux WebKitGTK**. Browser iframe smoke checks do not prove native support. Agent A's resizing and Agent B's mutation/journal contracts require integration review; F can optionally consume PDF thumbnails, and D owns arbitrary-extension explicit import UI.

Dedicated Rust unit tests cover verified ZIP round-trip, optional cleanup/undo, cancellation, corrupt/malicious archives, changed output conflicts, last-copy preservation for compressed/extracted sources, missing recovery copies, reviewed-plan drift/replaced ZIP, and recovery journal retention after 30 changes. The acceptance suite now checks real two-page PDF controls in Chromium, PDF-in-ZIP, corrupt ZIP PDF and a changed reviewed plan. Review fresh CI test results before integration; the prior reviewed head had two stale acceptance assertions. No releases or main merges performed.
