// A model's own page (docs/PLAN.md, "Phase 3 design"): its name, place and
// actions, and its files to look at (parts.js: the 3D view, pictures, documents
// and videos, and its files as folders, one list, by type or as a grid, with a
// switch between variant folders).
import { html, useState, useEffect, useLayoutEffect } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, setPref } from "./state.js";
import { routeHash } from "./context.js";
import { api, toast } from "./library.js";
import { variantsIn, inVariant, MESH } from "./parts.js";
import { FilePanel } from "./filepanel.js";
import { WorkspaceStage } from "./stage.js";
import { fileSel, reset, clear } from "./filesel.js";
import { ExtractionBar, extractionAction, installExtractKeys } from "./extract.js";
import { ActionRow, MODEL_ACTIONS, usePageKeys, runKey, letter, editDetails, writable } from "./actions.js";
import { PageHead } from "./layout.js";
import { PlaceLinks } from "./details.js";

const CTX = { page: "model" };

export function ModelPage({ id }) {
  const s = useStore(ui, (st) => ({ rev: st.catalogRev, favs: st.favs, overview: st.overview, readOnly: !!st.library?.read_only, names: st.library?.variant_folders || [] }));
  const [m, setM] = useState(null);
  const [error, setError] = useState("");
  const [narrow, setNarrow] = useState(() => matchMedia("(max-width: 899px)").matches);
  const [drawer, setDrawer] = useState(false);
  useEffect(() => {
    const mq = matchMedia("(max-width: 899px)");
    const change = () => { setNarrow(mq.matches); setDrawer(false); };
    mq.addEventListener("change", change);
    return () => mq.removeEventListener("change", change);
  }, []);
  const selection = useStore(ui, (st) => st.selection);
  useEffect(() => { ui.set({ selection: id, anchor: id, picked: [id] }); }, [id]);
  useEffect(() => {
    // saving or moving a model can give it a new id: follow it
    if (selection && selection !== id && m) location.replace(routeHash(`model:${selection}`));
  }, [selection]);
  useEffect(() => {
    let live = true;
    api("model_get", { id }).then((v) => { if (live) { setM(v); setError(""); } }, (e) => { if (live) setError(e.message || String(e)); });
    return () => { live = false; };
  }, [id, s.rev]);
  usePageKeys((e) => {
    if (!m) return;
    const l = letter(e);
    if (e.key === "F2") { e.preventDefault(); const ok = writable(); if (ok === true) editDetails([m], true); else toast(ok); }
    else if (l === "e" || l === "m" || l === "s") { e.preventDefault(); runKey(MODEL_ACTIONS, l.toUpperCase(), [m], CTX); }
  });
  if (error) return html`<div class="pages"><p class="warn-note" role="alert">${error}</p></div>`;
  if (!m) return html`<div class="pages"></div>`;
  const back = () => (history.length > 1 ? history.back() : (location.hash = routeHash("browse:all")));
  return html`<div class="model-page" id="model-page" data-model=${m.id}>
    <${PageHead} id="mp-head" title=${html`<button type="button" class="ghost back-btn" id="mp-back" onClick=${back} title="Back (Alt+←)">‹ Back</button><h1 id="mp-name">${m.name}</h1>`}
      sub=${html`${m.authors.length ? `by ${m.authors.join(", ")} · ` : ""}<${PlaceLinks} m=${m} overview=${s.overview} />`}>
      ${narrow ? html`<button type="button" class="ghost" id="files-drawer-button" aria-controls="file-panel" aria-expanded=${drawer} onClick=${() => setDrawer(!drawer)}>Files</button>` : null}
      <${ActionRow} targets=${[m]} ctx=${CTX} idPrefix="mp" />
    </${PageHead}>
    <${ModelWorkspace} key=${m.id} model=${m} names=${s.names} narrow=${narrow} drawer=${drawer} setDrawer=${setDrawer} />
  </div>`;
}

function ModelWorkspace({ model, names, narrow, drawer, setDrawer }) {
  const prefs = useStore(ui);
  const [variant, setVariant] = useState(undefined);
  const src = { kind: "model", id: model.id, rel: model.rel, model };
  const variants = variantsIn(model.files_list, names);
  const chosen = variant === undefined ? variants[0] || null : variant;
  const files = model.files_list.filter((f) => inVariant(f.rel, chosen, names));
  const open = narrow ? drawer : prefs.filePanelOpen !== false;
  const toggle = () => narrow ? setDrawer(!drawer) : setPref({ filePanelOpen: !open });
  useLayoutEffect(() => {
    setVariant(undefined); setDrawer(false);
    const main = model.main;
    reset(src, main ? main.entry ? `z:${main.file}!${main.entry}` : `f:${main.file}` : "d:");
  }, [model.id]);
  useLayoutEffect(() => {
    const s = fileSel.get();
    const shownFile = s.shown.slice(2).split("!")[0];
    if (s.shown !== "d:" && !files.some((f) => f.rel === shownFile || f.rel.startsWith(shownFile + "/"))) {
      const first = files.find((f) => MESH.test(f.rel));
      reset(src, first ? `f:${first.rel}` : "d:");
    }
  }, [chosen, model.files_list]);
  useEffect(() => installExtractKeys(), []);
  usePageKeys((e) => {
    if (e.key === "[") { e.preventDefault(); toggle(); }
    if (e.key === "Escape") { clear(); if (narrow) setDrawer(false); }
  });
  const contextActions = () => [extractionAction(src, model)];
  return html`<div class="model-workspace">
    ${narrow && open ? html`<button type="button" class="file-panel-shade" aria-label="Close files" onClick=${toggle}></button>` : null}
    <${FilePanel} src=${src} model=${model} files=${files} variants=${variants} variant=${chosen} setVariant=${setVariant} open=${open} toggle=${toggle} narrow=${narrow} contextActions=${contextActions} />
    <section class="workspace-viewing-area"><${ExtractionBar} src=${src} model=${model} /><${WorkspaceStage} src=${src} model=${model} files=${files} />
      ${model.details?.notes ? html`<details class="workspace-notes"><summary>Notes</summary><p>${model.details.notes}</p></details>` : null}
    </section>
  </div>`;
}
