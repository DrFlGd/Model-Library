// Reviewed folder-to-category import. This is a staging view: edits and
// previews never write to the library; only category_import_commit does.
import { html } from "../lib/html.js";
import { createStore, useStore } from "../lib/store.js";
import { ui } from "./state.js";
import { ctx, routeHash } from "./context.js";
import { api, followJob, loadOverview, toast, undoable, undoChange } from "./library.js";
import { treeList } from "./category.js";
import { size } from "./details.js";

export const categoryStage = createStore({
  library: null, open: false, proposal: null, plan: null,
  mode: "copy", busy: false, error: "", results: null, job: null,
});
const saveKey = (path) => `ml-category-staging:${path}`;
const save = (proposal) => {
  const path = ui.get().library?.path;
  if (path) ctx.platform?.store?.prefs?.set(saveKey(path), proposal);
};
const setProposal = (proposal) => {
  categoryStage.set({ proposal, plan: null, error: "", results: null });
  save(proposal);
};
const run = async (name, fn) => {
  categoryStage.set({ busy: true, error: "" });
  try { return await fn(); }
  catch (e) { categoryStage.set({ error: e.message || String(e) }); return null; }
  finally { categoryStage.set({ busy: false }); }
};
export async function openCategoryImport() {
  const path = ui.get().library?.path;
  if (!path) return;
  const st = categoryStage.get();
  if (st.library === path && st.proposal) {
    categoryStage.set({ open: true, error: "" });
    return;
  }
  let saved = ctx.platform?.store?.prefs?.get(saveKey(path), null);
  if (saved?.roots?.length) {
    categoryStage.set({ library: path, open: true, proposal: saved, plan: null, error: "", results: null });
    return;
  }
  const picked = await ctx.platform.library.pickFolder("Choose a folder to import as categories");
  if (!picked) return;
  await run("scan", async () => {
    const fresh = await api("category_import_scan", { paths: [picked] });
    categoryStage.set({ library: path, open: true, mode: "copy" });
    setProposal({ roots: fresh.roots });
  });
}
const pickMore = async () => {
  const picked = await ctx.platform.library.pickFolder("Choose another category folder");
  if (!picked) return;
  await run("scan", async () => {
    const old = categoryStage.get().proposal;
    const paths = [...(old?.roots || []).map((r) => r.source), picked];
    const fresh = await api("category_import_scan", { paths });
    const original = new Map((old?.roots || []).map((r) => [r.source, r]));
    setProposal({ roots: fresh.roots.map((r) => original.get(r.source) || r) });
  });
};
const reset = async () => {
  const roots = categoryStage.get().proposal?.roots;
  if (!roots?.length) return;
  await run("scan", async () => {
    const fresh = await api("category_import_scan", { paths: roots.map((r) => r.source) });
    setProposal({ roots: fresh.roots });
  });
};
const amend = (rootIndex, indexes, patch) => {
  const st = categoryStage.get();
  const clearMappings = (n) => ({ ...n, map_to: null,
    children: (n.children || []).map(clearMappings) });
  const change = (node, steps) => {
    if (!steps.length) {
      const edited = { ...node, ...patch };
      return "target" in patch ? { ...edited, children: (edited.children || []).map(clearMappings) } : edited;
    }
    const [idx, ...rest] = steps;
    return { ...node, children: node.children.map((child, i) => i === idx ? change(child, rest) : child) };
  };
  const proposal = { ...st.proposal, roots: st.proposal.roots.map((r, i) => i === rootIndex ? change(r, indexes) : r) };
  setProposal(proposal);
};
const allPlaces = (overview) => (overview?.schemas || []).flatMap((s) =>
  [{ schema: s.id, values: [], label: s.name },
    ...treeList(s).map((p) => ({ schema: s.id, values: p.path, label: `${s.name} › ${p.path.join(" › ")}` }))]);
const encode = (x) => JSON.stringify(x);
const decode = (v) => { try { return JSON.parse(v); } catch { return null; } };
const plural = (n, word) => `${n} ${word}${n === 1 ? "" : "s"}`;

function Node({ node, rootIndex, indexes = [], depth = 0, overview, rootTarget, inherited = false }) {
  const root = !indexes.length;
  const folder = node.hash == null;
  const model = node.kind === "model";
  const file = !folder;
  const locked = inherited || node.include === false;
  const places = allPlaces(overview);
  const target = root ? node.target : node.map_to;
  const opts = root ? places : places.filter((p) => rootTarget && p.schema === rootTarget.schema).map((p) => ({ ...p, encoded: encode(p.values) }));
  const selected = target == null ? "" : root ? encode([target.schema, target.values || []]) : encode(target);
  const update = (patch) => amend(rootIndex, indexes, patch);
  const direct = folder && node.children?.filter((c) => c.hash != null && c.include !== false).length;
  const parentIsModel = inherited || model;
  return html`<li class="ci-node" data-kind=${node.kind} data-depth=${depth}>
    <div class="ci-row" style=${`--ci-depth:${depth}`}>
      <label class="ci-included"><input type="checkbox" aria-label=${`Include ${node.name}`} checked=${node.include !== false}
        disabled=${inherited} onChange=${(e) => update({ include: e.target.checked })} /></label>
      <span class="ci-kind">${file ? "File" : model ? "Model" : "Category"}</span>
      <input class="ci-name" type="text" aria-label=${`Proposed name for ${node.name}`} value=${node.name}
        disabled=${locked} onInput=${(e) => update({ name: e.target.value })} />
      ${folder && !root ? html`<select aria-label=${`Treat ${node.name} as`} value=${node.kind} disabled=${locked}
        onChange=${(e) => update({ kind: e.target.value, map_to: null })}>
        <option value="category">Category container</option><option value="model">One model (with its folders)</option>
      </select>` : null}
      <span class="muted ci-size">${size(node.bytes || 0)}</span>
    </div>
    ${folder && node.kind === "category" && !locked ? html`<div class="ci-options" style=${`--ci-depth:${depth}`}>
      ${root ? html`<label>Destination
        <select aria-label=${`Destination for ${node.name}`} value=${selected}
          onChange=${(e) => { const v = decode(e.target.value); update({ target: v ? { schema: v[0], values: v[1] } : null }); }}>
          <option value="">New top-level category</option>
          ${opts.map((p) => html`<option key=${encode([p.schema,p.values])} value=${encode([p.schema,p.values])}>Map to existing: ${p.label}</option>`)}
        </select></label>` :
          rootTarget ? html`<label>Existing subcategory
            <select aria-label=${`Existing destination for ${node.name}`} value=${selected}
              onChange=${(e) => update({ map_to: decode(e.target.value) })}>
              <option value="">Create subcategory here</option>
              ${opts.map((p) => html`<option key=${p.encoded} value=${p.encoded}>${p.label}</option>`)}
            </select></label>` : null}
      ${direct ? html`<label class="ci-check"><input type="checkbox" checked=${node.group === true} onChange=${(e) => update({ group: e.target.checked })} />
          Group these ${plural(direct, "file")} as one model</label>
        ${node.group ? html`<input class="ci-group-name" type="text" aria-label="Grouped model name" placeholder="Model name" value=${node.group_name || ""}
          onInput=${(e) => update({ group_name: e.target.value })} />` : null}` : null}
    </div>` : null}
    ${folder && model ? html`<p class="ci-note" style=${`--ci-depth:${depth + 1}`}>All ${plural((node.children || []).length, "entry")} stay inside this model, including its internal folders and companion files.</p>` : null}
    ${folder && !model && node.children?.length ? html`<ol class="ci-children">
      ${node.children.map((child, i) => html`<${Node} key=${child.source} node=${child} rootIndex=${rootIndex}
        indexes=${[...indexes,i]} depth=${depth + 1} overview=${overview} rootTarget=${rootTarget} inherited=${locked} />`)}
    </ol>` : null}
  </li>`;
}
function Proposal({ proposal, overview }) {
  return html`<section class="ci-proposal" aria-label="Proposed category hierarchy">
    <h2>Proposed folder hierarchy</h2>
    <p class="muted">Tick to include, rename or change a folder to one model. Files directly in a category become separate models unless grouped. Mapping to an existing destination is deliberate; nothing has been added to the library.</p>
    <ol class="ci-tree">${proposal.roots.map((root, i) =>
      html`<${Node} key=${root.source} node=${root} rootIndex=${i} overview=${overview} rootTarget=${root.target} />`)}</ol>
  </section>`;
}
function Review({ plan, mode }) {
  const conflicts = plan.conflicts || [];
  return html`<section class="ci-review" aria-label="Category import review">
    <h2>Final review</h2>
    <p>${plural(plan.categories.length, "new category")}, ${plural(plan.subcategories.filter((x) => !x.existing).length, "proposed subcategory")},
      ${plural(plan.models, "model")} · ${size(plan.bytes)} · ${mode === "copy" ? "Copy (originals stay)" : "Move (originals removed only after a checked transfer)"}.</p>
    ${conflicts.length ? html`<div role="alert" class="form-error"><strong>Resolve these conflicts before importing:</strong>
      <ul>${conflicts.map((x,i) => html`<li key=${i}>${x}</li>`)}</ul></div>` : null}
    <div class="ci-destinations"><table><thead><tr><th>Model</th><th>Source</th><th>Destination</th><th>Size</th></tr></thead><tbody>
      ${plan.items.map((it,i) => html`<tr key=${i}><td>${it.name}${it.collision ? html` <span class="muted">(Keep both)</span>` : null}</td>
        <td title=${it.source}>${it.source}</td><td title=${it.dest}>${it.rel}</td><td>${size(it.bytes || 0)}</td></tr>`)}
    </tbody></table></div>
  </section>`;
}
function Results({ results, onReset }) {
  const good = (results.results || []).filter((r) => !r.error);
  const bad = (results.results || []).filter((r) => r.error);
  return html`<section class="ci-results" role="status"><h2>${plural(good.length, "model")} imported</h2>
    ${bad.length ? html`<p class="form-error">${plural(bad.length, "model")} could not be imported.</p>
      <ul>${bad.map((r,i) => html`<li key=${i}>${r.name}: ${r.error}</li>`)}</ul>` : null}
    <p><a href=${routeHash("browse:all")}>See imported models</a> · <button type="button" class="ghost" onClick=${onReset}>Start a new category import</button></p>
  </section>`;
}
export function CategoryImport() {
  const s = useStore(categoryStage);
  const overview = useStore(ui, (v) => v.overview);
  if (!s.proposal) return null;
  const back = () => categoryStage.set({ open: false });
  const review = () => run("plan", async () => {
    const plan = await api("category_import_plan", { proposal: s.proposal });
    categoryStage.set({ plan });
  });
  const commit = () => run("commit", async () => {
    const { job } = await api("category_import_commit", { proposal: s.proposal, mode: s.mode });
    categoryStage.set({ job });
    try {
      const done = await followJob(job, "Importing folders as categories");
      if (done.error) throw new Error(done.error);
      const results = done.result;
      categoryStage.set({ results, plan: null });
      if (results.journal) undoable(`Imported ${plural(results.imported, "model")} as categories.`,
        async () => { await undoChange(results.journal); categoryStage.set({ results: null }); await loadOverview(); });
      await loadOverview();
    } finally { categoryStage.set({ job: null }); }
  });
  const discard = () => {
    const path = s.library;
    ctx.platform?.store?.prefs?.set(saveKey(path), null);
    categoryStage.set({ proposal: null, plan: null, results: null, open: false, error: "" });
  };
  return html`<section id="category-import" class="ci-workspace" aria-label="Import folders as categories">
    <div class="ci-heading"><div><h2>Import folders as categories</h2><p class="muted">Stage a folder tree, review every destination, then commit.</p></div>
      <button type="button" class="ghost" id="ci-back" onClick=${back}>Return to sorting</button></div>
    ${s.error ? html`<p class="form-error" role="alert">${s.error}</p>` : null}
    ${s.busy ? html`<p role="status">Working… ${s.job ? html`<button type="button" class="ghost" onClick=${() => api("job_cancel", { id: s.job })}>Stop at a safe point</button>` : null}</p>` : null}
    ${s.results ? html`<${Results} results=${s.results} onReset=${discard} />` : html`
      <div class="ci-toolbar">
        <button type="button" class="ghost" id="ci-add" disabled=${s.busy} onClick=${pickMore}>Add another folder…</button>
        <button type="button" class="ghost" id="ci-rescan" disabled=${s.busy} onClick=${reset}>Scan again (reset edits)</button>
        <button type="button" class="ghost" id="ci-discard" disabled=${s.busy} onClick=${discard}>Discard stage</button>
      </div>
      <${Proposal} proposal=${s.proposal} overview=${overview} />
      <div class="ci-footer">
        <div class="seg" role="group" aria-label="Import originals">
          <button type="button" aria-pressed=${s.mode === "copy"} onClick=${() => categoryStage.set({ mode: "copy" })}>Copy</button>
          <button type="button" aria-pressed=${s.mode === "move"} onClick=${() => categoryStage.set({ mode: "move" })}>Move</button>
        </div>
        <button type="button" class="primary" id="ci-review" disabled=${s.busy} onClick=${review}>Review destinations</button>
      </div>
      ${s.plan ? html`<${Review} plan=${s.plan} mode=${s.mode} />
        <button type="button" class="primary" id="ci-commit" disabled=${s.busy || !!s.plan.conflicts?.length || !s.plan.models}
          onClick=${commit}>Confirm and import ${plural(s.plan.models, "model")}</button>` : null}
    `}
  </section>`;
}
