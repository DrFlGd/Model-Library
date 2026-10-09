// The details panel beside the library, Duplicates and (in its own form) Import
// (docs/PLAN.md, "UI pass design", step 3): one model's picture, its name,
// authors and tags changed right in the panel (saved when you leave the box, with
// Undo in the message), its category, the same action row as everywhere, then its
// details and files. Several selected: the count, what they share and the same
// row. Nothing selected: what the place shown holds.
import { html, useState, useEffect } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui } from "./state.js";
import { routeHash, schemaScope } from "./context.js";
import { api, saveDetails, recorded, toast } from "./library.js";
import { ActionRow } from "./actions.js";
import { EditBox } from "./layout.js";
import { Icon } from "./icons.js";
import { TypeTag } from "./filetypes.js";

const KIND_LABEL = { model: ["3D file", "3D files"], slicer: ["slicer file", "slicer files"], image: ["picture", "pictures"], doc: ["document", "documents"], video: ["video", "videos"], archive: ["archive", "archives"], other: ["other", "other"] };
const KIND_ICON = { model: "box", slicer: "stack", image: "image", doc: "file", video: "image", archive: "archive", other: "file" };
const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;

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

// Shared lazy multi-file automatic composition and explicit cover support.
import { Cover } from "./modelcover.js";
export { Cover };

/** Where a model is: its category and subcategories as links, or Unsorted. */
export function PlaceLinks({ m, overview }) {
  const schema = overview?.schemas?.find((x) => x.id === m.schema);
  if (!schema) return html`<a href=${routeHash("browse:unsorted")}>Unsorted</a>`;
  return html`<a href=${routeHash(`browse:${schemaScope(schema.id)}`)}>${schema.name}</a>${(m.path || []).map((v, i) =>
    html` › <a href=${routeHash(`browse:${schemaScope(schema.id, m.path.slice(0, i + 1))}`)}>${v}</a>`)}`;
}

const placeText = (overview, m) => {
  const schema = overview?.schemas?.find((x) => x.id === m.schema);
  return schema ? [schema.name, ...(m.path || [])].join(" › ") : "Unsorted";
};

/** One model. `ctx`: the page it's on, for its actions. */
export function ModelDetails({ id, empty = null, ctx = { page: "browse" } }) {
  const s = useStore(ui, (st) => ({ rev: st.catalogRev, favs: st.favs, overview: st.overview, readOnly: !!st.library?.read_only }));
  const [m, setM] = useState(null);
  const [error, setError] = useState("");
  const [nonce, setNonce] = useState(0);
  useEffect(() => {
    setError("");
    if (!id) { setM(null); return; }
    let live = true;
    api("model_get", { id }).then((v) => { if (live) setM(v); }, (e) => { if (live) { setM(null); setError(e.message || String(e)); } });
    return () => { live = false; };
  }, [id, s.rev]);
  if (!id) return html`<aside class="inspector" aria-label="Details" id="place-summary">${empty || html`<div class="insp-empty"><p>Select a model to see its details and files.</p></div>`}</aside>`;
  if (error) return html`<aside class="inspector" aria-label="Model details"><p class="warn-note" role="alert">${error}</p></aside>`;
  if (!m) return html`<aside class="inspector" aria-label="Model details"></aside>`;
  const schema = s.overview?.schemas?.find((x) => x.id === m.schema);
  const d = m.details || {};
  const source = d.source?.url || (typeof d.source === "string" ? d.source : "");
  const fields = (schema?.fields || []).filter((f) => fieldText(f, m.fields?.[f.key]));
  const save = (field) => async (v) => {
    const was = field === "name" ? m.name : (m[field] || []).join(", ");
    if (v === was) return;
    try {
      const r = await saveDetails(m, { [field]: v });
      recorded(field === "name" ? `Renamed ${m.name} to ${r.name}.` : `Saved the ${field} of ${r.name}.`, r.journal);
    } catch (e) {
      toast(e.message || String(e), 6000);
      setNonce(nonce + 1); // the box shows the saved value again
    }
  };
  return html`<aside class="inspector" aria-label="Model details" id="model-details" data-model=${m.id}>
    <${Cover} model=${m} cls="insp-thumb" />
    <div class="panel-fields" key=${nonce}>
      <${EditBox} id="details-name" cls="panel-name" label="Name" value=${m.name} off=${s.readOnly} onSave=${save("name")} />
      <${EditBox} id="details-authors" label="Authors" caption="Authors" placeholder="Authors, separated by commas" value=${m.authors.join(", ")} off=${s.readOnly} onSave=${save("authors")} />
      <${EditBox} id="details-tags" label="Tags" caption="Tags" placeholder="Tags, separated by commas" value=${(m.tags || []).join(", ")} off=${s.readOnly} onSave=${save("tags")} />
    </div>
    <p class="insp-sub panel-place" id="details-place">${Icon.layers(13)} <${PlaceLinks} m=${m} overview=${s.overview} /></p>
    <${ActionRow} targets=${[m]} ctx=${ctx} idPrefix="details" />
    <dl class="insp-dl" id="details-list">
      ${d.released ? html`<dt>Released</dt><dd>${d.released}</dd>` : null}
      ${source ? html`<dt>Source</dt><dd><a href=${source} target="_blank" rel="noopener">${source.replace(/^https?:\/\/(www\.)?/, "")}</a></dd>` : null}
      ${d.license ? html`<dt>Licence</dt><dd>${d.license}</dd>` : null}
      ${fields.map((f) => html`<dt key=${f.key}>${f.label}</dt><dd data-field=${f.key}>${fieldText(f, m.fields[f.key])}</dd>`)}
      <dt>Added</dt><dd>${(m.added || "").slice(0, 10)}</dd>
      <dt>Folder</dt><dd class="preview-path">${m.rel}</dd>
    </dl>
    ${d.notes ? html`<div class="insp-section"><h3>Notes</h3><p class="insp-note">${d.notes}</p></div>` : null}
    <div class="insp-section">
      <h3>Files <span class="muted">${m.files.count} · ${size(m.files.bytes)}</span></h3>
      <div class="insp-kinds">${Object.entries(m.files.kinds || {}).map(([k, n]) => html`<span class="chip" key=${k}>${n} ${(KIND_LABEL[k] || [k, k])[n === 1 ? 0 : 1]}</span>`)}</div>
      <ul class="insp-files" id="details-files">${(m.files_list || []).map((f) => html`<li key=${f.rel}><span class="insp-file"><${TypeTag} name=${f.rel} /> <span class="insp-file-name">${f.rel}</span></span><span class="muted">${size(f.size)}</span></li>`)}</ul>
    </div>
    ${!m.sidecar ? html`<p class="muted insp-later">No model.json yet: the name and author come from the folder name. Changing a detail or starring the model writes one.</p>` : null}
  </aside>`;
}

/** What every one of `lists` has (in the first one's order). */
const common = (lists) => (lists.length ? lists[0].filter((x) => lists.every((l) => l.includes(x))) : []);

/** Several models selected: the count, what they share, and the same row. */
export function SelectedPanel({ models, ctx = { page: "browse" } }) {
  const overview = useStore(ui, (st) => st.overview);
  const authors = common(models.map((m) => m.authors || []));
  const tags = common(models.map((m) => m.tags || []));
  const places = [...new Set(models.map((m) => placeText(overview, m)))];
  const bytes = models.reduce((a, m) => a + (m.files?.bytes || 0), 0);
  return html`<aside class="inspector" aria-label="Selected models" id="picked-panel">
    <h2 class="insp-title">${models.length} models selected</h2>
    <p class="insp-sub">${size(bytes)} in all.</p>
    <dl class="insp-dl" id="picked-shared">
      <dt>Authors</dt><dd>${authors.length ? authors.join(", ") : html`<span class="muted">${models.some((m) => m.authors?.length) ? "Different" : "None"}</span>`}</dd>
      <dt>Tags</dt><dd>${tags.length ? tags.map((t) => `#${t}`).join(" ") : html`<span class="muted">${models.some((m) => m.tags?.length) ? "Different" : "None"}</span>`}</dd>
      <dt>Category</dt><dd>${places.length === 1 ? places[0] : html`<span class="muted">${plural(places.length, "place", "different places")}</span>`}</dd>
    </dl>
    <${ActionRow} targets=${models} ctx=${ctx} idPrefix="picked" />
    <div><button type="button" class="ghost" id="picked-clear" title="Clear selection (Esc)" onClick=${() => ui.set({ picked: [] })}>Clear selection</button></div>
    <ul class="insp-files">${models.map((m) => html`<li key=${m.id}><span>${m.name}</span><span class="muted">${placeText(overview, m)}</span></li>`)}</ul>
    <p class="muted insp-later">Ctrl or Cmd-click adds or removes one, Shift-click selects a run, Ctrl+A selects everything shown and Esc clears.</p>
  </aside>`;
}

/** Nothing selected: what the place shown holds (`result`: the search's answer). */
export function PlaceSummary({ name, result }) {
  if (!result) return null;
  const authors = result.facets?.authors || [];
  const tags = result.facets?.tags || [];
  return html`<div class="place-summary">
    <h2 class="insp-title">${name}</h2>
    <p id="place-counts">${plural(result.total, "model", "models")}${result.bytes ? ` · ${size(result.bytes)}` : ""}</p>
    ${authors.length ? html`<div class="insp-section"><h3>Authors</h3><ul class="insp-files">${authors.slice(0, 8).map((a) => html`<li key=${a.value}><span>${a.value}</span><span class="muted">${a.count}</span></li>`)}</ul></div>` : null}
    ${tags.length ? html`<div class="insp-section"><h3>Tags</h3><div class="insp-chips">${tags.slice(0, 12).map((t) => html`<span class="chip" key=${t.value}>#${t.value}</span>`)}</div></div>` : null}
    <p class="muted insp-later">Select a model to see its details and files. Ctrl+A selects everything shown.</p>
  </div>`;
}
