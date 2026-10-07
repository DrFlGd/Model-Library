// A model's own page (docs/PLAN.md, "Phase 3 design"): its name, place and
// actions, and its files to look at (parts.js: the 3D view, pictures, documents
// and videos, and its files as folders, one list, by type or as a grid, with a
// switch between variant folders).
import { html, useState, useEffect } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui } from "./state.js";
import { routeHash } from "./context.js";
import { api, toast } from "./library.js";
import { FilesView } from "./parts.js";
import { ActionRow, MODEL_ACTIONS, usePageKeys, runKey, letter, editDetails, writable } from "./actions.js";
import { PageHead } from "./layout.js";
import { PlaceLinks } from "./details.js";

const CTX = { page: "model" };

export function ModelPage({ id }) {
  const s = useStore(ui, (st) => ({ rev: st.catalogRev, favs: st.favs, overview: st.overview, readOnly: !!st.library?.read_only, names: st.library?.variant_folders || [] }));
  const [m, setM] = useState(null);
  const [error, setError] = useState("");
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
      <${ActionRow} targets=${[m]} ctx=${CTX} idPrefix="mp" />
    </${PageHead}>
    <${FilesView} src=${{ kind: "model", id: m.id, rel: m.rel }} files=${m.files_list} main=${m.main} model=${m} names=${s.names}
      side=${m.details?.notes ? html`<div class="insp-section"><h3>Notes</h3><p class="insp-note">${m.details.notes}</p></div>` : null} />
  </div>`;
}
