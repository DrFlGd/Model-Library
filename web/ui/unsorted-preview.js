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
      <strong>Model preview</strong>
      ${narrow ? html`<button type="button" class="ghost" onClick=${onClose} aria-label="Close file preview">Close</button>` : null}
    </header>
    <div class="unsorted-preview-tabs" role="group" aria-label="Preview panel">
      <button type="button" class="ghost" aria-pressed=${tab === "files"} onClick=${() => setTab("files")}>Files</button>
      <button type="button" class="ghost" aria-pressed=${tab === "details"} onClick=${() => setTab("details")}>Details</button>
    </div>
    ${error ? html`<p class="form-error" role="alert">${error}</p>` : null}
    ${!m ? html`<p class="muted">${error ? "" : "Reading model files…"}</p>` : html`
      <div class="unsorted-preview-title">
        <strong>${m.name}</strong><a href=${routeHash(`model:${m.id}`)}>Open workspace ›</a>
      </div>
      ${tab === "details" ? html`<${ModelDetails} id=${m.id} />` : html`
        <${ViewSwitch} id="unsorted-file-views" views=${[["folders", "Folders", "folder"], ["all", "List", "list"], ["type", "By type", "grouped"]]} value=${view} onChange=${(filePanelView) => setPref({ filePanelView })} />
        <div class="unsorted-file-tree"><${SelectionRows} key=${m.id} src=${src} files=${m.files_list || []} name=${m.name} view=${view} /></div>
        <div class="unsorted-preview-stage"><${WorkspaceStage} key=${m.id} src=${src} model=${m} files=${m.files_list || []} /></div>
      `}
    `}
  </aside>`;
}
