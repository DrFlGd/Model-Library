// Files and selected-file viewing alongside the Unsorted collection.
// This is a collection preview, not a second model workspace implementation.
import { html, useState, useEffect, useLayoutEffect } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, setPref } from "./state.js";
import { api } from "./library.js";
import { routeHash } from "./context.js";
import { ModelDetails } from "./details.js";
import { SelectionRows } from "./parts.js";
import { WorkspaceStage } from "./stage.js";
import { reset } from "./filesel.js";
import { ViewSwitch } from "./layout.js";
import { ExtractionBar, fileActions } from "./extract.js";
import { ActionRow } from "./actions.js";

export function UnsortedPreview({ id, narrow = false, onClose }) {
  const rev = useStore(ui, (s) => s.catalogRev);
  const view = useStore(ui, (s) => s.filePanelView || "folders");
  const [model, setModel] = useState(null);
  const [error, setError] = useState("");
  const [tab, setTab] = useState("files");
  useLayoutEffect(() => { setModel(null); setError(""); setTab("files"); }, [id]);
  useEffect(() => {
    let live = true;
    api("model_get", { id }).then((m) => { if (live) { setModel(m); setError(""); } },
      (e) => { if (live) { setModel(null); setError(e.message || String(e)); } });
    return () => { live = false; };
  }, [id, rev]);
  const m = model?.id === id ? model : null;
  useLayoutEffect(() => {
    if (!m) return;
    const src = { kind: "model", id: m.id, rel: m.rel, model: m };
    reset(src, m.main ? m.main.entry ? `z:${m.main.file}!${m.main.entry}` : `f:${m.main.file}` : "d:");
  }, [m?.id]);
  const src = m ? { kind: "model", id: m.id, rel: m.rel, model: m } : null;
  return html`<aside class=${`unsorted-preview${narrow ? " mobile-preview" : ""}`} aria-label="Unsorted model preview" id="unsorted-preview">
    <header class="unsorted-preview-head">
      <strong title=${m?.name || ""}>${m?.name || "Model"}</strong>
      ${m ? html`<${ActionRow} targets=${[m]} ctx=${{ page: "browse" }} idPrefix="unsorted" />` : null}
      ${narrow ? html`<button type="button" class="ghost" onClick=${onClose} aria-label="Close file preview">Close</button>` : null}
    </header>
    <div class="unsorted-preview-tabs"><${ViewSwitch} id="unsorted-tabs" label="Preview panel" views=${[["files", "Files", "folder"], ["details", "Details", "info"]]} value=${tab} onChange=${setTab} /></div>
    ${error ? html`<p class="form-error" role="alert">${error}</p>` : null}
    ${!m ? html`<p class="muted">${error ? "" : "Reading model files…"}</p>` : html`
      ${tab === "details" ? html`<${ModelDetails} id=${m.id} row=${false} />` : html`
        <${ViewSwitch} id="unsorted-file-views" label="Show the files as" views=${[["folders", "Folders", "folder"], ["all", "All files", "list"], ["type", "By type", "grouped"]]} value=${view} onChange=${(filePanelView) => setPref({ filePanelView })} />
        <div class="unsorted-file-tree"><${SelectionRows} key=${m.id} src=${src} files=${m.files_list || []} name=${m.name} view=${view} contextActions=${() => fileActions(src, m)} /></div>
        <${ExtractionBar} src=${src} model=${m} />
        <div class="unsorted-preview-stage"><${WorkspaceStage} key=${m.id} src=${src} model=${m} files=${m.files_list || []} contextActions=${() => fileActions(src, m)} /></div>
      `}
    `}
  </aside>`;
}
