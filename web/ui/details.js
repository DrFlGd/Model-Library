// The selected model's details beside the browse view: its cover, name, authors,
// category, details from model.json, the schema's fields, and its files (parts
// keep their sub-folders). Editing opens the details dialog.
import { html, useState, useEffect } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui } from "./state.js";
import { routeHash, schemaScope } from "./context.js";
import { Icon } from "./icons.js";
import { api, isDesktop, libraryUrl, setStar, showModelFolder, toast } from "./library.js";

const KIND_LABEL = { model: ["3D file", "3D files"], slicer: ["slicer file", "slicer files"], image: ["picture", "pictures"], doc: ["document", "documents"], video: ["video", "videos"], archive: ["archive", "archives"], other: ["other", "other"] };

export function size(bytes) {
  if (bytes == null) return "";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let n = bytes, i = 0;
  while (n >= 1024 && i < units.length - 1) { n /= 1024; i++; }
  return `${n < 10 && i ? n.toFixed(1) : Math.round(n)} ${units[i]}`;
}

/** A schema field's value as text. */
export function fieldText(field, value) {
  if (value == null || value === "") return "";
  if (field?.type === "yes-no") return value === true || value === "yes" ? "Yes" : "No";
  return String(value);
}

export function ModelDetails({ id }) {
  const s = useStore(ui, (st) => ({ rev: st.catalogRev, favs: st.favs, overview: st.overview, readOnly: !!st.library?.read_only }));
  const [m, setM] = useState(null);
  const [error, setError] = useState("");
  useEffect(() => {
    setError("");
    if (!id) { setM(null); return; }
    let live = true;
    api("model_get", { id }).then((v) => { if (live) setM(v); }, (e) => { if (live) { setM(null); setError(e.message || String(e)); } });
    return () => { live = false; };
  }, [id, s.rev]);
  if (!id) return html`<aside class="inspector" aria-label="Model details"><div class="insp-empty"><p>Select a model to see its details and files.</p></div></aside>`;
  if (error) return html`<aside class="inspector" aria-label="Model details"><p class="warn-note" role="alert">${error}</p></aside>`;
  if (!m) return html`<aside class="inspector" aria-label="Model details"></aside>`;
  const schema = s.overview?.schemas?.find((x) => x.id === m.schema);
  const fav = s.favs.includes(m.id);
  const d = m.details || {};
  const source = d.source?.url || (typeof d.source === "string" ? d.source : "");
  const fields = (schema?.fields || []).filter((f) => fieldText(f, m.fields?.[f.key]));
  const star = async () => {
    try { await setStar(m, !fav); } catch (e) { toast(`Couldn't star it: ${e.message || e}`, 6000); }
  };
  const cover = m.files?.cover;
  return html`<aside class="inspector" aria-label="Model details" id="model-details">
    ${cover ? html`<span class="insp-thumb"><img src=${libraryUrl(`${m.rel}/${cover}`)} alt="" /></span>` : null}
    <div>
      <h2 class="insp-title" id="details-name">${m.name}</h2>
      ${m.authors.length ? html`<p class="insp-sub" id="details-authors">by ${m.authors.join(", ")}</p>` : null}
      <p class="insp-sub">${schema ? html`<a href=${routeHash(`browse:${schemaScope(schema.id)}`)}>${schema.name}</a>${m.path.map((v, i) => html` › <a href=${routeHash(`browse:${schemaScope(schema.id, m.path.slice(0, i + 1))}`)}>${v}</a>`)}`
        : html`<a href=${routeHash("browse:unsorted")}>Unsorted</a>`}</p>
    </div>
    <div class="insp-actions">
      <button type="button" class="ghost" id="details-star" aria-pressed=${fav ? "true" : "false"} onClick=${star} disabled=${s.readOnly}>${Icon.star(15, fav)} ${fav ? "Starred" : "Star"}</button>
      <button type="button" class="ghost" id="details-edit" onClick=${() => ui.set({ dialog: { type: "edit-model", model: m, schema } })} disabled=${s.readOnly}>${Icon.edit(15)} Edit details…</button>
      ${isDesktop() ? html`<button type="button" class="ghost" title="Show in folder" aria-label="Show in folder" onClick=${() => showModelFolder(m)}>${Icon.folder(15)}</button>` : null}
    </div>
    <dl class="insp-dl" id="details-list">
      ${d.released ? html`<dt>Released</dt><dd>${d.released}</dd>` : null}
      ${source ? html`<dt>Source</dt><dd><a href=${source} target="_blank" rel="noopener">${source.replace(/^https?:\/\/(www\.)?/, "")}</a></dd>` : null}
      ${d.license ? html`<dt>Licence</dt><dd>${d.license}</dd>` : null}
      ${fields.map((f) => html`<dt key=${f.key}>${f.label}</dt><dd data-field=${f.key}>${fieldText(f, m.fields[f.key])}</dd>`)}
      <dt>Added</dt><dd>${(m.added || "").slice(0, 10)}</dd>
      <dt>Folder</dt><dd class="preview-path">${m.rel}</dd>
    </dl>
    ${m.tags?.length ? html`<div class="insp-chips" id="details-tags">${m.tags.map((t) => html`<span class="chip" key=${t}>#${t}</span>`)}</div>` : null}
    ${d.notes ? html`<div class="insp-section"><h3>Notes</h3><p class="insp-note">${d.notes}</p></div>` : null}
    <div class="insp-section">
      <h3>Files <span class="muted">${m.files.count} · ${size(m.files.bytes)}</span></h3>
      <div class="insp-kinds">${Object.entries(m.files.kinds || {}).map(([k, n]) => html`<span class="chip" key=${k}>${n} ${(KIND_LABEL[k] || [k, k])[n === 1 ? 0 : 1]}</span>`)}</div>
      <ul class="insp-files" id="details-files">${(m.files_list || []).map((f) => html`<li key=${f.rel}><span>${f.rel}</span><span class="muted">${size(f.size)}</span></li>`)}</ul>
    </div>
    ${!m.sidecar ? html`<p class="muted insp-later">No model.json yet: the name and author come from the folder name. Editing or starring the model writes one.</p>` : null}
  </aside>`;
}
