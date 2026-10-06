// The Import page (docs/PLAN.md, "Phase 2 design"): choose a messy folder to
// sort, or add model folders and files (or drop them on the window); review what
// the core proposes, give each a category, then move or copy them in.
import { html, useState, useEffect, useRef } from "../lib/html.js";
import { createStore, useStore } from "../lib/store.js";
import { ui } from "./state.js";
import { ctx, routeHash } from "./context.js";
import { Icon } from "./icons.js";
import { api, followJob, loadOverview, toast } from "./library.js";
import { CategoryPicker } from "./category.js";
import { size } from "./details.js";

/** The import being prepared (kept while you look elsewhere in the app). */
export const importer = createStore({ items: [], left: [], mode: "move", busy: false, job: null, results: null, error: "" });

let keySeq = 0;
const KIND = { model: "3D", slicer: "slicer", image: "pictures", doc: "documents", video: "videos", archive: "archives", other: "other" };

function fromScan(it, overview) {
  const sc = overview?.schemas?.find((s) => s.id === it.guess?.schema);
  const values = sc ? [...(it.guess.values || [])] : [];
  return { ...it, key: ++keySeq, author: it.author || "", schema: sc ? sc.id : null, values, skip: it.warnings.some((w) => w.kind === "empty"), picked: false };
}

/** Ask the core to propose models: `contents` sorts a folder's contents. */
export async function addSources(paths, contents) {
  if (!paths?.length) return;
  importer.set({ busy: true, error: "", results: null });
  try {
    const r = await api("import_scan", { paths, contents });
    const overview = ui.get().overview || (await loadOverview());
    const known = new Set(importer.get().items.map((i) => i.source));
    const fresh = r.items.filter((i) => !known.has(i.source)).map((i) => fromScan(i, overview));
    importer.set((s) => ({ items: [...s.items, ...fresh], left: [...s.left, ...r.left_behind] }));
    if (!fresh.length) toast("Nothing new to import there.");
  } catch (e) {
    importer.set({ error: e.message || String(e) });
  } finally {
    importer.set({ busy: false });
  }
}

const update = (key, patch) => importer.set((s) => ({ items: s.items.map((i) => (i.key === key ? { ...i, ...patch } : i)) }));

async function split(item) {
  importer.set({ busy: true });
  try {
    const r = await api("import_scan", { paths: [item.source], contents: true });
    const overview = ui.get().overview;
    const parts = r.items.map((i) => fromScan(i, overview));
    importer.set((s) => {
      const at = s.items.findIndex((i) => i.key === item.key);
      const items = [...s.items];
      items.splice(at, 1, ...parts);
      return { items, left: [...s.left, ...r.left_behind] };
    });
  } catch (e) {
    toast(`Couldn't split it: ${e.message || e}`, 6000);
  } finally {
    importer.set({ busy: false });
  }
}

const planItem = (i) => ({ source: i.source, files: i.files, schema: i.schema, values: i.values, name: i.name, author: i.author });

function Warning({ w, item }) {
  if (w.kind === "several") return html`<p class="imp-warn" data-warn="several">Maybe several models: it has no model files of its own but ${w.parts.length} sub-folders with them (${w.parts.slice(0, 5).join(", ")}${w.parts.length > 5 ? "…" : ""}). Keep it as one model with parts, or${" "}<button type="button" class="linkish imp-split" onClick=${() => split(item)}>split it into ${w.parts.length}</button>.</p>`;
  if (w.kind === "duplicate") return html`<p class="imp-warn" data-warn="duplicate">Looks like <a href=${routeHash(w.rel.startsWith("Unsorted/") ? "browse:unsorted" : "browse:all")}>${w.name}</a>, already in the library (same files and sizes).</p>`;
  if (w.kind === "in-library") return html`<p class="imp-note" data-warn="in-library">Already in the library at ${w.rel}; importing moves it.</p>`;
  if (w.kind === "empty") return html`<p class="imp-warn" data-warn="empty">No files: nothing to import.</p>`;
  return null;
}

function ItemRow({ item, plan, overview, index }) {
  const s = item.summary || {};
  const kinds = Object.entries(s.kinds || {}).map(([k, n]) => `${n} ${KIND[k] || k}`).join(", ");
  return html`<li class=${`imp-row${item.skip ? " skipped" : ""}`} data-source=${item.source}>
    <div class="imp-check"><input type="checkbox" class="imp-pick" checked=${item.picked} aria-label=${`Pick ${item.name}`} onChange=${(e) => update(item.key, { picked: e.target.checked })} /></div>
    <div class="imp-body">
      <div class="imp-top">
        <input type="text" class="imp-name" aria-label="Name" value=${item.name} onInput=${(e) => update(item.key, { name: e.target.value })} />
        <input type="text" class="imp-author" aria-label="Author" placeholder="Author" value=${item.author} onInput=${(e) => update(item.key, { author: e.target.value })} />
        <button type="button" class="ghost imp-skip" aria-pressed=${item.skip ? "true" : "false"} onClick=${() => update(item.key, { skip: !item.skip })}>${item.skip ? "Skipped" : "Skip"}</button>
      </div>
      <${CategoryPicker} overview=${overview} schema=${item.schema} values=${item.values} idPrefix=${`imp-${index}`} onChange=${(schema, values) => update(item.key, { schema, values })} />
      <input type="text" class="imp-tags" aria-label="Tags" placeholder="Tags, separated by commas" value=${item.tags} onInput=${(e) => update(item.key, { tags: e.target.value })} />
      <p class="imp-meta muted"><span title=${item.source}>${item.folder ? "Folder" : item.files.length > 1 ? `${item.files.length} files` : "File"}: ${item.source.split(/[\\/]/).pop()}</span> · ${s.count || 0} ${s.count === 1 ? "file" : "files"}${kinds ? ` (${kinds})` : ""} · ${size(s.bytes || 0)}${item.has_sidecar ? " · has its own model.json" : ""}</p>
      ${item.warnings.map((w) => html`<${Warning} w=${w} item=${item} key=${w.kind} />`)}
      ${item.skip ? null : plan?.error ? html`<p class="form-error imp-dest">${plan.error}</p>` : plan?.rel ? html`<p class="imp-dest"><span class="muted">Goes to</span> <code class="preview-path">${plan.rel}</code></p>` : null}
    </div>
  </li>`;
}

/** Give the picked rows the same category, author or tags. */
function BatchBar({ items, overview }) {
  const picked = items.filter((i) => i.picked);
  const [schema, setSchema] = useState(null);
  const [values, setValues] = useState([]);
  const [author, setAuthor] = useState("");
  const [tags, setTags] = useState("");
  const all = items.length > 0 && picked.length === items.length;
  const apply = () => {
    importer.set((s) => ({ items: s.items.map((i) => (i.picked ? { ...i, schema, values: [...values], ...(author.trim() ? { author } : {}), ...(tags.trim() ? { tags } : {}) } : i)) }));
    toast(`Set for ${picked.length} ${picked.length === 1 ? "model" : "models"}.`);
  };
  return html`<div class="imp-batch">
    <label class="imp-all"><input type="checkbox" id="imp-all" checked=${all} onChange=${(e) => importer.set((s) => ({ items: s.items.map((i) => ({ ...i, picked: e.target.checked })) }))} /> ${picked.length ? `${picked.length} picked` : "Pick all"}</label>
    <${CategoryPicker} overview=${overview} schema=${schema} values=${values} idPrefix="batch" onChange=${(s, v) => { setSchema(s); setValues(v); }} />
    <input type="text" id="batch-author" placeholder="Author" aria-label="Author for the picked" value=${author} onInput=${(e) => setAuthor(e.target.value)} />
    <input type="text" id="batch-tags" placeholder="Tags" aria-label="Tags for the picked" value=${tags} onInput=${(e) => setTags(e.target.value)} />
    <button type="button" class="ghost" id="batch-apply" disabled=${!picked.length} onClick=${apply}>Set for picked</button>
  </div>`;
}

function Results({ results }) {
  const ok = results.filter((r) => !r.error);
  const bad = results.filter((r) => r.error);
  return html`<section class="imp-results" id="import-results">
    <h2>${ok.length} ${ok.length === 1 ? "model" : "models"} ${results.mode === "copy" ? "copied" : "moved"} in${bad.length ? `, ${bad.length} not` : ""}</h2>
    <ul class="ls-list">${results.map((r) => html`<li key=${r.source} class=${r.error ? "bad" : "ok"}><span>${r.name}</span>
      ${r.error ? html`<span class="form-error">${r.error}</span>` : html`<span class="muted">${r.rel}${r.note ? ` (${r.note})` : ""}</span>`}</li>`)}</ul>
    <p><a href=${routeHash("browse:all")}>See all models</a></p>
  </section>`;
}

export function ImportPage() {
  const s = useStore(importer, (st) => st);
  const overview = useStore(ui, (st) => st.overview);
  const lib = useStore(ui, (st) => st.library);
  const [plans, setPlans] = useState([]);
  const seq = useRef(0);
  const live = s.items.filter((i) => !i.skip);
  // where each goes, worked out by the core as you type
  const planKey = JSON.stringify(live.map(planItem));
  useEffect(() => {
    if (!live.length) { setPlans([]); return; }
    const n = ++seq.current;
    const t = setTimeout(() => api("import_plan", { items: live.map(planItem) }).then((p) => { if (n === seq.current) setPlans(p); }, () => {}), 200);
    return () => clearTimeout(t);
  }, [planKey]);
  const planFor = (item) => { const i = live.indexOf(item); return i >= 0 && plans.length === live.length ? plans[i] : null; };
  const blocked = !live.length || plans.length !== live.length || plans.some((p) => p.error);
  const p = ctx.platform;
  const pickAndAdd = async (contents, file) => {
    const path = file ? await p.library.pickFile("Choose a model file or archive") : await p.library.pickFolder(contents ? "Choose a folder to sort" : "Choose a model's folder");
    if (path) await addSources([path], contents);
  };
  const go = async () => {
    importer.set({ error: "", results: null });
    try {
      const { job } = await api("import_commit", { items: live.map((i) => ({ ...planItem(i), tags: i.tags })), mode: s.mode, force_copy: !!window.__forceCopy });
      importer.set({ job: { id: job, progress: {} } });
      const done = await followJob(job, s.mode === "copy" ? "Copying models in" : "Moving models in", (j) => importer.set({ job: j }));
      const results = Object.assign(done.result?.results || [], { mode: done.result?.mode });
      const failed = new Set(results.filter((r) => r.error).map((r) => r.source));
      importer.set((st) => ({ job: null, results, mode: "move", error: done.error || "", items: st.items.filter((i) => i.skip || failed.has(i.source)), left: failed.size ? st.left : [] }));
      await loadOverview();
    } catch (e) {
      importer.set({ job: null, error: e.message || String(e) });
    }
  };
  const job = s.job;
  const prog = job?.progress || {};
  const pct = prog.total_bytes ? Math.round((100 * (prog.bytes || 0)) / prog.total_bytes) : 0;
  if (!lib) return html`<div class="pages"><h1>Import</h1><p>Open a library first.</p></div>`;
  return html`<div class="pages import-page">
    <h1>Import</h1>
    <p class="muted">Sort a folder of downloads (on a NAS, a drive, or already inside the library) or add models one by one. You can also drop folders and files on this window. Nothing moves until you press Import.</p>
    ${lib.read_only ? html`<p class="warn-note">${lib.read_only}</p>` : null}
    <div class="home-actions">
      <button type="button" class="primary" id="import-sort" disabled=${s.busy || !!job} onClick=${() => pickAndAdd(true)}>${Icon.folder(15)} Sort a folder…</button>
      <button type="button" class="ghost" id="import-add-folder" disabled=${s.busy || !!job} onClick=${() => pickAndAdd(false)}>Add a model folder…</button>
      <button type="button" class="ghost" id="import-add-file" disabled=${s.busy || !!job} onClick=${() => pickAndAdd(false, true)}>Add a file…</button>
      ${s.items.length ? html`<button type="button" class="ghost" id="import-clear" disabled=${!!job} onClick=${() => importer.set({ items: [], left: [], results: null, error: "" })}>Start again</button>` : null}
    </div>
    ${s.busy ? html`<p class="muted">Looking through the folder…</p>` : null}
    ${s.error ? html`<p class="form-error" role="alert">${s.error}</p>` : null}
    ${s.results ? html`<${Results} results=${s.results} />` : null}
    ${s.items.length ? html`
      <${BatchBar} items=${s.items} overview=${overview} />
      <ol class="imp-list" id="import-list">${s.items.map((item, i) => html`<${ItemRow} key=${item.key} item=${item} index=${i} plan=${planFor(item)} overview=${overview} />`)}</ol>
      ${s.left.length ? html`<details class="imp-left"><summary>${s.left.length} loose ${s.left.length === 1 ? "file isn't" : "files aren't"} part of any model and will stay where ${s.left.length === 1 ? "it is" : "they are"}</summary>
        <ul>${s.left.map((f) => html`<li key=${f}>${f}</li>`)}</ul></details>` : null}
      <div class="imp-go">
        <div class="seg" role="group" aria-label="Move or copy">
          <button type="button" id="mode-move" aria-pressed=${s.mode === "move" ? "true" : "false"} onClick=${() => importer.set({ mode: "move" })}>Move</button>
          <button type="button" id="mode-copy" aria-pressed=${s.mode === "copy" ? "true" : "false"} onClick=${() => importer.set({ mode: "copy" })}>Copy</button>
        </div>
        <span class="muted imp-mode-note">${s.mode === "move" ? "Files move into the library (no duplicates). Across drives they're copied, checked, then the originals deleted." : "The originals stay where they are; every copy is checked."}</span>
        ${job ? html`<div class="imp-progress" id="import-progress"><progress max="100" value=${pct}></progress>
            <span>${prog.items ? `${Math.min((prog.item || 0) + 1, prog.items)} of ${prog.items}: ${prog.name || ""}` : "Starting…"} (${pct}%)</span>
            <button type="button" class="ghost" id="import-stop" onClick=${() => api("job_cancel", { id: job.id })}>Stop</button></div>`
          : html`<button type="button" class="primary" id="import-go" disabled=${blocked || !!lib.read_only} onClick=${go}>${!live.length ? "Nothing to import" : `${s.mode === "copy" ? "Copy" : "Import"} ${live.length} ${live.length === 1 ? "model" : "models"}`}</button>`}
      </div>` : !s.results ? html`<div class="empty">Choose where your models are to begin.</div>` : null}
  </div>`;
}

/** Folders dropped on the window: each one is a model. */
export function onDropped(paths) {
  if (!paths?.length || !ui.get().library) return;
  if (ui.get().route !== "import") location.hash = routeHash("import");
  addSources(paths, false);
}

/** Open the Import page sorting a folder (Home's "Sort…" for loose library folders). */
export function sortFolder(path) {
  location.hash = routeHash("import");
  addSources([path], true);
}
