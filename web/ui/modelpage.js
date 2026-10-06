// A model's own page (docs/PLAN.md, "Phase 3 design"): its name, place and
// actions, and its files to look at (parts.js: the 3D view, pictures, documents
// and videos, and its files as folders, one list, by type or as a grid, with a
// switch between variant folders).
import { html, useState, useEffect } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui } from "./state.js";
import { routeHash, schemaScope } from "./context.js";
import { Icon } from "./icons.js";
import { api, isDesktop, setStar, showModelFolder, toast } from "./library.js";
import { FilesView } from "./parts.js";

export function ModelPage({ id }) {
  const s = useStore(ui, (st) => ({ rev: st.catalogRev, favs: st.favs, overview: st.overview, readOnly: !!st.library?.read_only, names: st.library?.variant_folders || [] }));
  const [m, setM] = useState(null);
  const [error, setError] = useState("");
  const selection = useStore(ui, (st) => st.selection);
  useEffect(() => { ui.set({ selection: id, picked: [] }); }, [id]);
  useEffect(() => {
    // saving or moving a model can give it a new id: follow it
    if (selection && selection !== id && m) location.replace(routeHash(`model:${selection}`));
  }, [selection]);
  useEffect(() => {
    let live = true;
    api("model_get", { id }).then((v) => { if (live) { setM(v); setError(""); } }, (e) => { if (live) setError(e.message || String(e)); });
    return () => { live = false; };
  }, [id, s.rev]);
  if (error) return html`<div class="pages"><p class="warn-note" role="alert">${error}</p></div>`;
  if (!m) return html`<div class="pages"></div>`;
  const schema = s.overview?.schemas?.find((x) => x.id === m.schema);
  const fav = s.favs.includes(m.id);
  const back = () => (history.length > 1 ? history.back() : (location.hash = routeHash("browse:all")));
  return html`<div class="model-page" id="model-page" data-model=${m.id}>
    <div class="mp-head">
      <button type="button" class="ghost" onClick=${back} aria-label="Back">‹ Back</button>
      <div class="mp-title"><h1 id="mp-name">${m.name}</h1>
        <p class="insp-sub">${m.authors.length ? `by ${m.authors.join(", ")} · ` : ""}${schema ? html`<a href=${routeHash(`browse:${schemaScope(schema.id)}`)}>${schema.name}</a>${m.path.map((v, i) => html` › <a href=${routeHash(`browse:${schemaScope(schema.id, m.path.slice(0, i + 1))}`)}>${v}</a>`)}` : html`<a href=${routeHash("browse:unsorted")}>Unsorted</a>`}</p></div>
      <div class="insp-actions">
        <button type="button" class="ghost" aria-pressed=${fav ? "true" : "false"} disabled=${s.readOnly} onClick=${() => setStar(m, !fav).catch((e) => toast(String(e.message || e)))}>${Icon.star(15, fav)} ${fav ? "Starred" : "Star"}</button>
        <button type="button" class="ghost" disabled=${s.readOnly} onClick=${() => ui.set({ dialog: { type: "edit-model", model: m, schema } })}>${Icon.edit(15)} Edit details…</button>
        <button type="button" class="ghost" disabled=${s.readOnly} onClick=${() => ui.set({ dialog: { type: "move-models", models: [m] } })}>${Icon.move(15)} Move…</button>
        ${isDesktop() ? html`<button type="button" class="ghost" onClick=${() => showModelFolder(m)}>${Icon.folder(15)} Show in folder</button>` : null}
      </div>
    </div>
    <${FilesView} src=${{ kind: "model", id: m.id, rel: m.rel }} files=${m.files_list} main=${m.main} model=${m} names=${s.names}
      side=${m.details?.notes ? html`<div class="insp-section"><h3>Notes</h3><p class="insp-note">${m.details.notes}</p></div>` : null} />
  </div>`;
}
