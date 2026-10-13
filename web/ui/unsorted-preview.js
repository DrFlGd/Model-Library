// Files and selected-file viewing alongside the Unsorted collection, in the
// inspector's Files tab; the model's actions and its selected files' actions are
// in the command bar. This is a collection preview, not a second model workspace.
import { html, useState, useEffect, useLayoutEffect } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, setPref } from "./state.js";
import { api } from "./library.js";
import { SelectionRows } from "./parts.js";
import { WorkspaceStage } from "./stage.js";
import { reset } from "./filesel.js";
import { ViewSwitch } from "./layout.js";
import { fileActions } from "./extract.js";

export function UnsortedPreview({ id }) {
  const rev = useStore(ui, (s) => s.catalogRev);
  const view = useStore(ui, (s) => s.filePanelView || "folders");
  const [model, setModel] = useState(null);
  const [error, setError] = useState("");
  useLayoutEffect(() => { setModel(null); setError(""); }, [id]);
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
  return html`<div class="unsorted-preview" aria-label="Unsorted model's files" id="unsorted-preview">
    ${error ? html`<p class="form-error" role="alert">${error}</p>` : null}
    ${!m ? html`<p class="muted">${error ? "" : "Reading model files…"}</p>` : html`
      <${ViewSwitch} id="unsorted-file-views" label="Show the files as" views=${[["folders", "Folders", "folder"], ["all", "All files", "list"], ["type", "By type", "grouped"]]} value=${view} onChange=${(filePanelView) => setPref({ filePanelView })} />
      <div class="unsorted-file-tree"><${SelectionRows} key=${m.id} src=${src} files=${m.files_list || []} name=${m.name} view=${view} contextActions=${() => fileActions(src, m)} /></div>
      <div class="unsorted-preview-stage"><${WorkspaceStage} key=${m.id} src=${src} model=${m} files=${m.files_list || []} contextActions=${() => fileActions(src, m)} /></div>
    `}
  </div>`;
}
