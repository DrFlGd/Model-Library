// A model's own page (docs/PLAN.md, "Phase 3 design"): its name, place and
// actions, and its files to look at (parts.js: the 3D view, pictures, documents
// and videos, and its files as folders, one list, by type or as a grid, with a
// switch between variant folders), in the command bar, the Files pane and the
// inspector's Details and Media (docs/PLAN.md, "Inspector rails design").
import { html, useState, useEffect, useLayoutEffect, useRef } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, setPref } from "./state.js";
import { routeHash, schemaScope } from "./context.js";
import { api, toast } from "./library.js";
import { variantsIn, inVariant, MESH, SelectionRows } from "./parts.js";
import { FilePanel } from "./filepanel.js";
import { WorkspaceDetails } from "./workspace-details.js";
import { WorkspaceStage } from "./stage.js";
import { fileSel, reset, clear } from "./filesel.js";
import { useFileSelection, fileActions, installExtractKeys } from "./extract.js";
import { MODEL_ACTIONS, menuItems, usePageKeys, runKey, letter, editDetails, writable, typing } from "./actions.js";
import { CommandBar } from "./layout.js";
import { size } from "./details.js";
import { Inspector, useWidth } from "./inspector.js";
import { Icon } from "./icons.js";

const CTX = { page: "model" };

const MEDIA = new Set(["image", "doc", "video"]);
/** The least room the 3D view gets beside the panels, and a folded panel's rail. */
const STAGE = 440, RAIL = 36;

export function ModelPage({ id }) {
  const s = useStore(ui, (st) => ({ rev: st.catalogRev, favs: st.favs, overview: st.overview, readOnly: !!st.library?.read_only, names: st.library?.variant_folders || [] }));
  const [m, setM] = useState(null);
  const [error, setError] = useState("");
  const [narrow, setNarrow] = useState(() => matchMedia("(max-width: 899px)").matches);

  useEffect(() => {
    const mq = matchMedia("(max-width: 899px)");
    const change = () => { setNarrow(mq.matches); ui.set({ mobilePanel: null, navOpen: false }); };
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
  // the category panel beside the menu follows the model's place
  const placeKey = m?.schema ? JSON.stringify([m.schema, m.path || []]) : null;
  useEffect(() => {
    ui.set({ modelPlace: placeKey ? { schema: m.schema, path: m.path || [] } : null });
    return () => ui.set({ modelPlace: null });
  }, [placeKey]);
  usePageKeys((e) => {
    if (!m) return;
    const l = letter(e);
    if (e.key === "F2") { e.preventDefault(); const ok = writable(); if (ok === true) editDetails([m], true); else toast(ok); }
    else if (l === "e" || l === "m" || l === "s") { e.preventDefault(); runKey(MODEL_ACTIONS, l.toUpperCase(), [m], CTX); }
  });
  const src = m ? { kind: "model", id: m.id, rel: m.rel, model: m } : null;
  const picking = useFileSelection(src, m);
  const back = () => (history.length > 1 ? history.back() : (location.hash = routeHash("browse:all")));
  if (error) return html`<${CommandBar} id="mp-head" back=${{ id: "mp-back", run: back }} title="Model" /><div class="pages"><p class="warn-note" role="alert">${error}</p></div>`;
  if (!m) return html`<${CommandBar} id="mp-head" back=${{ id: "mp-back", run: back }} title="" /><div class="pages"></div>`;
  const starred = s.favs.includes(m.id);
  const star = MODEL_ACTIONS.find((a) => a.id === "star");
  const edit = MODEL_ACTIONS.find((a) => a.id === "edit");
  const ok = writable();
  const bytes = (m.files_list || []).reduce((n, f) => n + (f.size || 0), 0);
  return html`<${CommandBar} id="mp-head" back=${{ id: "mp-back", run: back }} up=${placeCrumbs(m, s.overview)}
      title=${html`<h1 id="mp-name" title=${m.name}>${m.name}</h1><button type="button" class="ghost bar-star" id="mp-star" aria-pressed=${starred ? "true" : "false"}
        disabled=${ok !== true} title=${ok !== true ? ok : starred ? "Remove star (S)" : "Star (S)"} aria-label=${starred ? "Remove star" : "Star"} onClick=${() => star.run([m], CTX)}>${Icon.star(16, starred)}</button>`}
      count=${html`<span class="page-count" id="mp-count">${m.authors.length ? `by ${m.authors.join(", ")} · ` : ""}${m.files_list?.length || 0} ${m.files_list?.length === 1 ? "file" : "files"} · ${size(bytes)}</span>`}
      primary=${html`<button type="button" class="primary compact" id="mp-edit" disabled=${ok !== true} title=${ok !== true ? ok : "Edit details (E)"} onClick=${() => edit.run([m], CTX)}>${Icon.edit(15)} Edit details…</button>`}
      more=${{ id: "mp-more", items: () => menuItems(MODEL_ACTIONS, [m], CTX).filter((it) => it.id !== "edit" && it.id !== "star") }}
      selection=${picking} />
    <div class="model-page" id="model-page" data-model=${m.id}>
      <${ModelWorkspace} key=${m.id} model=${m} names=${s.names} narrow=${narrow} onModelChange=${setM} />
    </div>`;
}

/** Where a model is filed, as the bar's links above its name. */
function placeCrumbs(m, overview) {
  const schema = overview?.schemas?.find((x) => x.id === m.schema);
  if (!schema) return [["Unsorted", routeHash("browse:unsorted")]];
  return [[schema.name, routeHash(`browse:${schemaScope(schema.id)}`)], ...(m.path || []).map((v, i) => [v, routeHash(`browse:${schemaScope(schema.id, m.path.slice(0, i + 1))}`)])];
}

function ModelWorkspace({ model, names, narrow, onModelChange }) {
  const prefs = useStore(ui);
  const [variant, setVariant] = useState(undefined);
  const src = { kind: "model", id: model.id, rel: model.rel, model };
  const variants = variantsIn(model.files_list, names);
  const chosen = variant === undefined ? variants[0] || null : variant;
  const files = model.files_list.filter((f) => inVariant(f.rel, chosen, names));
  // The 3D view keeps at least STAGE px: when the window can't fit it beside the
  // panels, the inspector folds first, then the files panel; opening one then
  // isn't kept, and they come back when the window grows.
  const box = useRef(null);
  const room = useWidth(box);
  const fileW = Math.max(220, Math.min(480, prefs.filePanelWidth || 300));
  const paneW = Math.max(260, Math.min(520, prefs.inspectorWidth || 340));
  const filesFit = !room || room - fileW - RAIL >= STAGE;
  const [crowdOpen, setCrowdOpen] = useState(false);
  useLayoutEffect(() => setCrowdOpen(false), [filesFit]);
  const open = narrow ? prefs.mobilePanel === "files" : filesFit ? prefs.filePanelOpen !== false : crowdOpen;
  const paneFits = !room || room - (open ? fileW : RAIL) - paneW - RAIL >= STAGE;
  const toggle = () => narrow ? ui.set({ mobilePanel: open ? null : "files", navOpen: false }) : filesFit ? setPref({ filePanelOpen: !open }) : setCrowdOpen(!open);
  useLayoutEffect(() => {
    setVariant(undefined); ui.set({ mobilePanel: null });
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
  useEffect(() => {
    const key = (e) => {
      if (e.defaultPrevented || typing(e) || ui.get().dialog || ui.get().menu) return;
      if (e.key === "[") { e.preventDefault(); toggle(); }
      if (e.key === "Escape") { clear(); if (narrow) ui.set({ mobilePanel: null }); }
    };
    addEventListener("keydown", key);
    return () => removeEventListener("keydown", key);
  });
  const contextActions = () => fileActions(src, model);
  const media = (model.files_list || []).filter((f) => MEDIA.has(f.kind));
  return html`<div class="model-workspace" ref=${box}>
    ${narrow && open ? html`<button type="button" class="file-panel-shade" aria-label="Close panel" onClick=${() => ui.set({ mobilePanel: null })}></button>` : null}
    <${FilePanel} src=${src} model=${model} files=${files} variants=${variants} variant=${chosen} setVariant=${setVariant} open=${open} toggle=${toggle} narrow=${narrow} contextActions=${contextActions} />
    <section class="workspace-viewing-area"><${WorkspaceStage} src=${src} model=${model} files=${files} contextActions=${contextActions} />
      ${model.details?.notes ? html`<details class="workspace-notes"><summary>Notes</summary><p>${model.details.notes}</p></details>` : null}
    </section>
    <${Inspector} page="model" open=${false} crowded=${!narrow && !paneFits} tabs=${[
      { id: "details", label: "Details", icon: "info", body: html`<div id="workspace-details-panel"><${WorkspaceDetails} model=${model} onModelChange=${onModelChange} /></div>` },
      { id: "media", label: "Media", icon: "image", empty: !media.length, body: media.length
        ? html`<div class="media-list" id="media-list"><${SelectionRows} src=${src} files=${media} name=${model.name} view="all" contextActions=${contextActions} /></div>`
        : html`<p class="muted" id="media-list">No pictures, documents, readmes or videos in this model.</p>` },
    ]} />
  </div>`;
}
