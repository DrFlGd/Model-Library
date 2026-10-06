// The sorting workspace, on the Import page (docs/PLAN.md, "Phase 5 design"):
// folders read as they are on disk, with the models the app proposes in them all
// the way down. Pick one or many (click, Ctrl- or Shift-click, or tick; a ticked
// folder takes everything in it) and send them to a category and subcategory,
// keeping their folders as subcategories if you like; group files and folders
// into one model, make a folder one model or split one; then import what's
// sorted. The workspace is kept on this computer, so sorting can go on over
// several sittings.
import { html, useState, useEffect, useMemo } from "../lib/html.js";
import { createStore, useStore } from "../lib/store.js";
import { ui, setPref } from "./state.js";
import { ctx, routeHash } from "./context.js";
import { Icon } from "./icons.js";
import { api, followJob, loadOverview, toast } from "./library.js";
import { CategoryPicker } from "./category.js";
import { size } from "./details.js";
import { FilesView, LazyPreview } from "./parts.js";

/** The workspace as the core keeps it, and what's picked here. */
export const sorter = createStore({
  session: null,     // sort_get: { roots, folders, items, left, busy, read }
  library: null,     // the library it's for (its path)
  picked: [],        // keys: "i:<item id>", "f:<folder path>", "l:<file path>"
  anchor: null,      // the key clicked last (Shift-click picks from it)
  focus: null,       // the key whose details are shown
  filter: "todo",    // todo | sorted | skipped | done | all
  q: "",
  open: {},          // folders folded or unfolded: { path: bool }
  limit: 400,        // rows drawn (more on request)
  job: null,         // { id, label, progress }
  results: null,     // the last import's result
  error: "",
  mode: "move",
  send: { schema: null, values: [], keep: false, keepSelf: true },
});

const KIND = { model: "3D", slicer: "slicer", image: "pictures", doc: "documents", video: "videos", archive: "archives", other: "other" };
const kindsText = (s) => Object.entries(s?.kinds || {}).map(([k, n]) => `${n} ${KIND[k] || k}`).join(", ");
const baseName = (p) => p.split(/[\\/]/).filter(Boolean).pop() || p;
const under = (p, dir) => !!p && (p === dir || p.startsWith(`${dir}/`) || p.startsWith(`${dir}\\`));
const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;

export const status = (i) => (i.done ? "done" : i.skip ? "skipped" : i.placed ? "sorted" : "todo");
const FILTERS = [["todo", "To sort"], ["sorted", "Sorted"], ["skipped", "Skipped"], ["done", "Imported"], ["all", "All"]];
const VIEWS = [["folders", "Folders", "folder"], ["list", "List", "table"], ["grid", "Grid", "grid"], ["category", "By category", "grouped"]];

/** "Category › sub › sub", or Unsorted. */
function placeLabel(overview, schema, values) {
  if (!schema) return "Unsorted";
  const sc = overview?.schemas?.find((s) => s.id === schema);
  return [sc?.name || schema, ...(values || [])].join(" › ");
}

/** Read the workspace from the core. */
export async function loadSession() {
  const library = ui.get().library?.path || null;
  if (!library) return;
  try {
    const session = await api("sort_get");
    sorter.set((s) => ({ session, library, ...(s.library !== library ? { picked: [], focus: null, anchor: null, open: {}, results: null } : {}) }));
  } catch (e) {
    sorter.set({ error: e.message || String(e) });
  }
}

/** A change to the workspace: the core answers with the whole of it. */
async function act(cmd, args) {
  try {
    const session = await api(cmd, args);
    sorter.set({ session, error: "" });
    return session;
  } catch (e) {
    toast(e.message || String(e), 7000);
    return null;
  }
}

/** Work that takes a while (reading folders, importing), followed to the end. */
async function runJob(cmd, args, label) {
  sorter.set({ error: "", results: null });
  try {
    const { job } = await api(cmd, args);
    sorter.set({ job: { id: job, label, progress: {} } });
    const done = await followJob(job, label, (j) => sorter.set({ job: { id: job, label, progress: j.progress || {} } }));
    if (done.error) sorter.set({ error: done.error });
    return done;
  } catch (e) {
    sorter.set({ error: e.message || String(e) });
    return null;
  } finally {
    sorter.set({ job: null });
    await loadSession();
  }
}

/** Add folders or files to the workspace: `contents` sorts what's in a folder;
 *  otherwise each is one model. */
export async function addSources(paths, contents) {
  if (!paths?.length) return;
  const done = await runJob("sort_add", { paths, contents }, contents ? `Reading ${baseName(paths[0])}` : "Adding");
  if (done && !done.error && !done.result?.items) toast("Nothing to sort there.");
}

const useGuess = (ids, folders = []) => act("sort_update", { ids, folders, patch: { accept: true } });

/** The workspace's lists, by where they're shown. */
function indexOf(se) {
  const kids = new Map();
  const slot = (p) => {
    const k = p || "";
    if (!kids.has(k)) kids.set(k, { folders: [], items: [], left: [] });
    return kids.get(k);
  };
  const shown = new Set(se.folders.map((f) => f.path));
  // what's in a folder that's gone (after an import) is shown at the top
  const at = (p) => (p && shown.has(p) ? p : null);
  for (const f of se.folders) slot(at(f.parent)).folders.push(f);
  for (const i of se.items) slot(at(i.parent)).items.push(i);
  for (const l of se.left) slot(at(l.parent)).left.push(l);
  const byName = (a, b) => a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" });
  for (const k of kids.values()) { k.folders.sort(byName); k.items.sort(byName); k.left.sort(byName); }
  return {
    kids,
    byId: new Map(se.items.map((i) => [i.id, i])),
    folderByPath: new Map(se.folders.map((f) => [f.path, f])),
    leftByPath: new Map(se.left.map((l) => [l.path, l])),
    rootOf: (p) => se.roots.map((r) => r.path).filter((r) => under(p, r)).sort((a, b) => b.length - a.length)[0] || null,
  };
}

/** What's picked, as the core takes it: items, folders (and everything in
 *  them), loose files; and every item that covers. */
function selectionOf(s, ix) {
  const folders = s.picked.filter((k) => k.startsWith("f:")).map((k) => k.slice(2)).filter((p) => ix.folderByPath.has(p));
  const inPicked = (p) => folders.some((f) => under(p, f));
  const ids = s.picked.filter((k) => k.startsWith("i:")).map((k) => k.slice(2)).filter((id) => ix.byId.has(id) && !inPicked(ix.byId.get(id).parent));
  const files = s.picked.filter((k) => k.startsWith("l:")).map((k) => k.slice(2)).filter((p) => ix.leftByPath.has(p) && !inPicked(ix.leftByPath.get(p).parent));
  const inside = folders.length ? s.session.items.filter((i) => inPicked(i.parent)) : [];
  const items = [...ids.map((id) => ix.byId.get(id)), ...inside].filter((i) => !i.done);
  return { ids, folders, files, items, inPicked, count: ids.length + folders.length + files.length };
}

function matcher(s) {
  const words = s.q.toLowerCase().split(/\s+/).filter(Boolean);
  const text = (i) => `${i.name} ${i.author} ${i.tags} ${baseName(i.path)}`.toLowerCase();
  return {
    item: (i) => (s.filter === "all" || status(i) === s.filter) && words.every((w) => text(i).includes(w)),
    left: (l) => (s.filter === "all" || s.filter === "todo") && words.every((w) => l.name.toLowerCase().includes(w)),
    any: words.length > 0,
  };
}

/** The Folders view's rows, top to bottom (only unfolded folders' contents). */
function folderRows(ix, s, m) {
  const rows = [];
  const counts = new Map();
  const count = (path) => {
    if (counts.has(path)) return counts.get(path);
    const k = ix.kids.get(path);
    let n = [0, 0];
    if (k) {
      n = [k.items.filter(m.item).length, k.left.filter(m.left).length];
      for (const f of k.folders) { const c = count(f.path); n = [n[0] + c[0], n[1] + c[1]]; }
    }
    counts.set(path, n);
    return n;
  };
  const walk = (parent, depth) => {
    const k = ix.kids.get(parent || "");
    if (!k) return;
    for (const f of k.folders) {
      const [n, left] = count(f.path);
      if (!n && !left) continue;
      const open = s.open[f.path] ?? (m.any || depth < 2);
      rows.push({ key: `f:${f.path}`, type: "folder", depth, folder: f, count: n, left, open });
      if (open) walk(f.path, depth + 1);
    }
    for (const i of k.items) if (m.item(i)) rows.push({ key: `i:${i.id}`, type: "item", depth, item: i });
    for (const l of k.left) if (m.left(l)) rows.push({ key: `l:${l.path}`, type: "left", depth, left: l });
  };
  walk(null, 0);
  return rows;
}

function Place({ item, overview }) {
  if (item.done) return html`<span class="sw-chip done" title=${item.done.rel || ""}>Imported</span>`;
  if (item.skip) return html`<span class="sw-chip skipped">Skipped</span>`;
  const err = item.error ? html`<span class="sw-chip bad" title=${item.error}>${Icon.alert(12)} Not imported</span>` : null;
  if (item.placed) return html`${err}<span class="sw-chip placed">→ ${placeLabel(overview, item.schema, item.values)}</span>`;
  if (item.guess?.schema) {
    return html`${err}<span class="sw-chip guess">Suggested: ${placeLabel(overview, item.guess.schema, item.guess.values)}
      <button type="button" class="linkish sw-use" onClick=${(e) => { e.stopPropagation(); useGuess([item.id]); }}>Use</button></span>`;
  }
  return html`${err}<span class="sw-chip todo">Not sorted</span>`;
}

const how = (e) => (e.shiftKey ? "range" : e.ctrlKey || e.metaKey ? "toggle" : "one");

function Tick({ k, on, covered, label, pick }) {
  return html`<input type="checkbox" class="sw-tick" checked=${on || covered} disabled=${covered} aria-label=${`Pick ${label}`}
    onClick=${(e) => { e.stopPropagation(); pick(k, e.shiftKey ? "range" : "tick"); }} />`;
}

function FolderRow({ r, s, sel, pick }) {
  const f = r.folder;
  const on = s.picked.includes(r.key);
  const covered = !on && sel.inPicked(f.parent) ;
  const toggle = (e) => { e.stopPropagation(); sorter.set((st) => ({ open: { ...st.open, [f.path]: !r.open } })); };
  return html`<li class=${`sw-row sw-folder${on ? " picked" : ""}${s.focus === r.key ? " focused" : ""}`} data-key=${r.key} data-name=${f.name}
      style=${`--depth:${r.depth}`} aria-selected=${on || covered ? "true" : "false"} onClick=${(e) => pick(r.key, how(e))}>
    <${Tick} k=${r.key} on=${on} covered=${covered} label=${f.name} pick=${pick} />
    <button type="button" class="sw-fold" aria-expanded=${r.open ? "true" : "false"} aria-label=${`${r.open ? "Fold" : "Unfold"} ${f.name}`} onClick=${toggle}>${r.open ? "▾" : "▸"}</button>
    <span class="sw-icon">${Icon.folder(14)}</span>
    <span class="sw-name">${f.name}</span>
    <span class="sw-meta muted">${r.count ? plural(r.count, "model", "models") : ""}${r.count && r.left ? " · " : ""}${r.left ? plural(r.left, "loose file", "loose files") : ""}</span>
  </li>`;
}

function ItemRow({ item, depth = 0, s, sel, pick, overview, where }) {
  const k = `i:${item.id}`;
  const on = s.picked.includes(k);
  const covered = !on && sel.inPicked(item.parent);
  const sm = item.summary || {};
  return html`<li class=${`sw-row sw-item sw-${status(item)}${on ? " picked" : ""}${s.focus === k ? " focused" : ""}`} data-key=${k} data-name=${item.name}
      style=${`--depth:${depth}`} aria-selected=${on || covered ? "true" : "false"} onClick=${(e) => pick(k, how(e))}>
    <${Tick} k=${k} on=${on} covered=${covered} label=${item.name} pick=${pick} />
    <span class="sw-fold-gap"></span>
    <span class="sw-icon">${item.kind === "group" ? Icon.layers(14) : Icon.box(14)}</span>
    <span class="sw-name">${item.name}${item.author ? html` <span class="muted">(${item.author})</span>` : null}${where ? html`<span class="sw-where muted">${where}</span>` : null}</span>
    <span class="sw-meta muted">${plural(sm.count || 0, "file", "files")} · ${size(sm.bytes || 0)}</span>
    <${Place} item=${item} overview=${overview} />
  </li>`;
}

function LeftRow({ l, depth, s, sel, pick }) {
  const k = `l:${l.path}`;
  const on = s.picked.includes(k);
  const covered = !on && sel.inPicked(l.parent);
  return html`<li class=${`sw-row sw-left${on ? " picked" : ""}${s.focus === k ? " focused" : ""}`} data-key=${k} data-name=${l.name}
      style=${`--depth:${depth}`} aria-selected=${on || covered ? "true" : "false"} onClick=${(e) => pick(k, how(e))} title="Not part of any model">
    <${Tick} k=${k} on=${on} covered=${covered} label=${l.name} pick=${pick} />
    <span class="sw-fold-gap"></span>
    <span class="sw-icon">${Icon.file(14)}</span>
    <span class="sw-name">${l.name}</span>
    <span class="sw-meta muted">${size(l.size)}</span>
  </li>`;
}

function More({ shown, total }) {
  if (shown >= total) return null;
  return html`<button type="button" class="ghost sw-more" onClick=${() => sorter.set((s) => ({ limit: s.limit + 1000 }))}>Show ${Math.min(1000, total - shown)} more (${total - shown} not shown)</button>`;
}

function FoldersView({ rows, s, sel, pick, overview }) {
  const shown = rows.slice(0, s.limit);
  return html`<ul class="sw-tree" id="sort-tree" role="listbox" aria-multiselectable="true">${shown.map((r) =>
    r.type === "folder" ? html`<${FolderRow} key=${r.key} r=${r} s=${s} sel=${sel} pick=${pick} />`
      : r.type === "item" ? html`<${ItemRow} key=${r.key} item=${r.item} depth=${r.depth} s=${s} sel=${sel} pick=${pick} overview=${overview} />`
      : html`<${LeftRow} key=${r.key} l=${r.left} depth=${r.depth} s=${s} sel=${sel} pick=${pick} />`)}</ul>
    <${More} shown=${shown.length} total=${rows.length} />`;
}

const COLS = [["name", "Name"], ["folder", "Folder"], ["files", "Files"], ["size", "Size"], ["place", "Where it goes"]];

function ListView({ items, s, sel, pick, overview, ix, sortBy, setSortBy }) {
  const shown = items.slice(0, s.limit);
  return html`<table class="sw-table" id="sort-list">
    <thead><tr><th></th>${COLS.map(([k, label]) => html`<th key=${k}><button type="button" class="linkish" aria-pressed=${sortBy === k ? "true" : "false"} onClick=${() => setSortBy(k)}>${label}${sortBy === k ? " ▾" : ""}</button></th>`)}</tr></thead>
    <tbody>${shown.map((i) => {
      const k = `i:${i.id}`;
      const on = s.picked.includes(k);
      return html`<tr key=${k} data-key=${k} data-name=${i.name} class=${`${on ? "picked" : ""}${s.focus === k ? " focused" : ""}`} aria-selected=${on ? "true" : "false"} onClick=${(e) => pick(k, how(e))}>
        <td><${Tick} k=${k} on=${on} covered=${false} label=${i.name} pick=${pick} /></td>
        <td class="sw-name">${i.name}${i.author ? html` <span class="muted">(${i.author})</span>` : null}</td>
        <td class="muted">${folderOf(i, ix)}</td>
        <td class="num">${i.summary?.count || 0}</td>
        <td class="num">${size(i.summary?.bytes || 0)}</td>
        <td><${Place} item=${i} overview=${overview} /></td></tr>`;
    })}</tbody></table>
    <${More} shown=${shown.length} total=${items.length} />`;
}

/** Where an item is, from the folder that was added. */
function folderOf(i, ix) {
  if (!i.parent) return "";
  const root = ix.rootOf(i.parent);
  const parts = (root ? [baseName(root), i.parent.slice(root.length)] : [i.parent]).join("/").split(/[\\/]/).filter(Boolean);
  return parts.join(" › ");
}

function GridView({ items, s, pick, overview }) {
  const shown = items.slice(0, Math.min(s.limit, 300));
  return html`<div class="grid-view sw-grid" id="sort-grid">${shown.map((i) => {
    const k = `i:${i.id}`;
    const on = s.picked.includes(k);
    return html`<div key=${k} class=${`card sw-card sw-${status(i)}${s.focus === k ? " focused" : ""}`} data-key=${k} data-name=${i.name} aria-selected=${on ? "true" : "false"} onClick=${(e) => pick(k, how(e))}>
      <div class="sw-card-top"><${Tick} k=${k} on=${on} covered=${false} label=${i.name} pick=${pick} /></div>
      <div class="thumb sw-thumb"><${LazyPreview} cacheKey=${`sort:${i.id}:${i.summary?.bytes}`} ask=${() => api("sort_preview", { id: i.id })} fallback=${html`<span class="tile-icon">${Icon.box(34)}</span>`} alt=${i.name} /></div>
      <div class="card-name">${i.name}</div>
      <div class="card-sub">${i.author || html`<span>${plural(i.summary?.count || 0, "file", "files")}</span>`}</div>
      <div class="sw-card-place"><${Place} item=${i} overview=${overview} /></div>
    </div>`;
  })}</div>
  <${More} shown=${shown.length} total=${items.length} />`;
}

function CategoryView({ items, s, sel, pick, overview, ix }) {
  const groups = new Map();
  const add = (key, label, i) => { if (!groups.has(key)) groups.set(key, { label, items: [] }); groups.get(key).items.push(i); };
  for (const i of items) {
    const st = status(i);
    if (st === "todo") add("0", "Not sorted yet", i);
    else if (st === "skipped") add("8", "Skipped", i);
    else if (st === "done") add("9", "Imported", i);
    else { const label = placeLabel(overview, i.schema, i.values); add(`${i.schema ? "1" : "7"}${label}`, label, i); }
  }
  const keys = [...groups.keys()].sort((a, b) => a.localeCompare(b));
  let budget = s.limit;
  return html`<div class="sw-groups" id="sort-groups">${keys.map((k) => {
    const g = groups.get(k);
    const shown = g.items.slice(0, Math.max(0, budget));
    budget -= shown.length;
    const all = g.items.every((i) => s.picked.includes(`i:${i.id}`));
    return html`<section key=${k} class="sw-group" data-group=${g.label}>
      <h3><label class="sw-group-pick"><input type="checkbox" checked=${all} aria-label=${`Pick everything in ${g.label}`}
        onChange=${(e) => { const ks = g.items.map((i) => `i:${i.id}`); sorter.set((st) => ({ picked: e.target.checked ? [...new Set([...st.picked, ...ks])] : st.picked.filter((x) => !ks.includes(x)) })); }} />
        ${g.label}</label> <span class="muted">${g.items.length}</span></h3>
      <ul class="sw-tree">${shown.map((i) => html`<${ItemRow} key=${i.id} item=${i} s=${s} sel=${sel} pick=${pick} overview=${overview} where=${folderOf(i, ix)} />`)}</ul>
    </section>`;
  })}</div>
  <${More} shown=${Math.min(s.limit, items.length)} total=${items.length} />`;
}

/** What to do with what's picked: send it somewhere, group it, split it… */
function Actions({ s, sel, ix, overview }) {
  const send = s.send;
  const setSend = (patch) => sorter.set((st) => ({ send: { ...st.send, ...patch } }));
  const n = sel.items.length;
  const one = sel.ids.length === 1 && !sel.folders.length && !sel.files.length ? ix.byId.get(sel.ids[0]) : null;
  const oneFolder = sel.folders.length === 1 && !sel.ids.length && !sel.files.length ? ix.folderByPath.get(sel.folders[0]) : null;
  const clear = () => sorter.set({ picked: [], anchor: null });
  const where = placeLabel(overview, send.schema, send.values);
  const go = async () => {
    const r = await act("sort_send", { ids: sel.ids, folders: sel.folders, schema: send.schema, values: send.values, keep: send.keep && !!sel.folders.length, keep_self: send.keepSelf });
    if (r) { toast(`Sent ${plural(r.changed, "model", "models")} to ${where}${send.keep && sel.folders.length && send.schema ? ", with their folders" : ""}.`); clear(); }
  };
  const group = async () => {
    const r = await act("sort_group", { ids: sel.ids, folders: sel.folders, files: sel.files });
    if (r?.id) { sorter.set({ picked: [`i:${r.id}`], focus: `i:${r.id}`, anchor: `i:${r.id}` }); toast(`Grouped into one model: ${ix.byId.get(r.id)?.name || r.items.find((i) => i.id === r.id)?.name}.`); }
  };
  const join = async () => {
    const r = await act("sort_join", { folder: oneFolder.path });
    if (r?.id) { sorter.set({ picked: [`i:${r.id}`], focus: `i:${r.id}`, anchor: `i:${r.id}` }); toast(`${oneFolder.name} is one model now.`); }
  };
  const split = async () => {
    const r = await act("sort_split", { id: one.id });
    if (r) { clear(); toast(one.kind === "group" ? `Ungrouped ${one.name}.` : `Split ${one.name}.`); }
  };
  const patch = (p, msg) => act("sort_update", { ids: sel.ids, folders: sel.folders, patch: p }).then((r) => r && toast(msg));
  const anyGuess = sel.items.some((i) => i.guess?.schema && !i.placed);
  const allSkipped = n > 0 && sel.items.every((i) => i.skip);
  const what = [sel.ids.length ? plural(sel.ids.length, "model", "models") : "", sel.folders.length ? `${plural(sel.folders.length, "folder", "folders")} (${plural(n - sel.ids.length, "model", "models")} in ${sel.folders.length === 1 ? "it" : "them"})` : "", sel.files.length ? plural(sel.files.length, "loose file", "loose files") : ""].filter(Boolean).join(", ");
  return html`<div class="sw-actions" id="sort-actions" onKeyDown=${(e) => { if (e.key === "Escape") clear(); }}>
    <div class="sw-actions-head"><strong id="sort-picked">${what || "Nothing picked"}</strong>
      <button type="button" class="ghost" onClick=${clear} aria-label="Pick nothing">${Icon.close(13)}</button></div>
    ${n ? html`<div class="sw-send" onKeyDown=${(e) => { if (e.key === "Enter" && e.target.tagName === "INPUT" && e.target.type === "text") { e.preventDefault(); go(); } }}>
      <span class="field-label">Send to</span>
      <${CategoryPicker} overview=${overview} schema=${send.schema} values=${send.values} idPrefix="send" onChange=${(schema, values) => setSend({ schema, values })} />
      ${sel.folders.length && send.schema ? html`<div class="sw-keep">
        <label class="check-row"><input type="checkbox" id="send-keep" checked=${send.keep} onChange=${(e) => setSend({ keep: e.target.checked })} /> Keep ${sel.folders.length === 1 ? "its" : "their"} folders as subcategories</label>
        ${send.keep ? html`<label class="check-row"><input type="checkbox" id="send-keep-self" checked=${send.keepSelf} onChange=${(e) => setSend({ keepSelf: e.target.checked })} /> Starting with ${sel.folders.length === 1 ? `“${baseName(sel.folders[0])}” itself` : "the picked folders themselves"}</label>` : null}
      </div>` : null}
      <button type="button" class="primary" id="send-go" onClick=${go}>Send ${plural(n, "model", "models")}</button>
    </div>` : null}
    <div class="sw-buttons">
      ${anyGuess ? html`<button type="button" class="ghost" id="sort-use-guess" onClick=${() => useGuess(sel.ids, sel.folders)}>Use suggested places</button>` : null}
      ${sel.count >= 2 || (sel.files.length === 1 && sel.count === 1) ? html`<button type="button" class="ghost" id="sort-group" onClick=${group}>${Icon.layers(14)} ${sel.count === 1 ? "Make it a model" : "Group into one model"}</button>` : null}
      ${oneFolder ? html`<button type="button" class="ghost" id="sort-join" onClick=${join}>Make one model</button>` : null}
      ${one && !one.done && one.kind !== "files" ? html`<button type="button" class="ghost" id="sort-split" onClick=${split}>${one.kind === "group" ? "Ungroup" : "Split"}</button>` : null}
      ${n ? html`<button type="button" class="ghost" id="sort-skip" onClick=${() => patch({ skip: !allSkipped }, allSkipped ? "Not skipped any more." : `Skipped ${plural(n, "model", "models")}.`)}>${allSkipped ? "Don't skip" : "Skip"}</button>` : null}
      ${sel.items.some((i) => i.placed) ? html`<button type="button" class="ghost" id="sort-unsend" onClick=${() => patch({ clear: true }, "Not sorted any more.")}>Not sorted</button>` : null}
    </div>
  </div>`;
}

/** The focused model's details and files, a folder's, or a loose file's. */
function Details({ s, ix, overview, names }) {
  const k = s.focus || "";
  const item = k.startsWith("i:") ? ix.byId.get(k.slice(2)) : null;
  const folder = k.startsWith("f:") ? ix.folderByPath.get(k.slice(2)) : null;
  const left = k.startsWith("l:") ? ix.leftByPath.get(k.slice(2)) : null;
  const [files, setFiles] = useState(null);
  const sig = item ? `${item.id}:${item.summary?.count}:${item.summary?.bytes}:${item.sources.length}` : "";
  useEffect(() => {
    setFiles(null);
    if (!item) return;
    let live = true;
    api("sort_files", { id: item.id }).then((r) => { if (live) setFiles(r); }, (e) => { if (live) setFiles({ error: e.message || String(e) }); });
    return () => { live = false; };
  }, [sig]);
  if (folder) {
    const inside = s.session.items.filter((i) => under(i.parent, folder.path));
    const todo = inside.filter((i) => status(i) === "todo").length;
    return html`<div class="sw-details" id="sort-details">
      <h2>${Icon.folder(18)} ${folder.name}</h2>
      <p class="muted sw-path">${folder.path}</p>
      <p>${plural(inside.length, "model", "models")} in it${inside.length ? `, ${todo} not sorted yet` : ""}.</p>
      <div class="sw-buttons">
        <button type="button" class="ghost" onClick=${() => sorter.set({ picked: [k], anchor: k })}>Pick everything in it</button>
        <button type="button" class="ghost" id="details-join" onClick=${async () => { const r = await act("sort_join", { folder: folder.path }); if (r?.id) sorter.set({ picked: [`i:${r.id}`], focus: `i:${r.id}` }); }}>Make it one model</button>
      </div>
      <p class="muted sw-hint">Pick a folder and send it to a category to send everything in it; tick “Keep its folders as subcategories” to bring its folders along.</p>
    </div>`;
  }
  if (left) {
    return html`<div class="sw-details" id="sort-details">
      <h2>${Icon.file(18)} ${left.name}</h2>
      <p class="muted sw-path">${left.path}</p>
      <p>${size(left.size)}. It isn't part of any model, so it stays where it is unless you group it into one.</p>
      <div class="sw-buttons"><button type="button" class="ghost" onClick=${async () => { const r = await act("sort_group", { files: [left.path] }); if (r?.id) sorter.set({ picked: [`i:${r.id}`], focus: `i:${r.id}` }); }}>Make it a model</button></div>
    </div>`;
  }
  if (!item) return html`<div class="sw-details sw-details-empty" id="sort-details"><p class="muted">Pick a model to see its files and details here.</p></div>`;
  const save = (field) => (e) => {
    const v = e.target.value.trim();
    if (v !== (item[field] || "")) act("sort_update", { ids: [item.id], patch: { [field]: v } });
  };
  const sm = item.summary || {};
  return html`<div class="sw-details" id="sort-details" data-item=${item.id} key=${item.id}>
    <div class="sw-fields">
      <input type="text" class="sw-name-input" id="details-name" aria-label="Name" value=${item.name} disabled=${!!item.done} onChange=${save("name")} />
      <input type="text" id="details-author" aria-label="Author" placeholder="Author" value=${item.author} disabled=${!!item.done} onChange=${save("author")} />
      <input type="text" id="details-tags" aria-label="Tags" placeholder="Tags, separated by commas" value=${item.tags} disabled=${!!item.done} onChange=${save("tags")} />
    </div>
    <p class="sw-status"><${Place} item=${item} overview=${overview} />
      ${item.done ? html` <a href=${routeHash(`model:${item.done.id}`)}>${item.done.rel}</a>` : null}</p>
    ${item.error ? html`<p class="form-error">${item.error}</p>` : null}
    <p class="muted sw-path">${item.kind === "folder" ? "Folder" : item.kind === "group" ? `Grouped from ${plural(item.sources.length, "thing", "things")}` : plural(item.sources.length, "file", "files")}: ${item.kind === "folder" ? item.path : item.sources.map(baseName).join(", ")}
      · ${plural(sm.count || 0, "file", "files")}${kindsText(sm) ? ` (${kindsText(sm)})` : ""} · ${size(sm.bytes || 0)}${item.has_sidecar ? " · has its own model.json" : ""}</p>
    ${(item.warnings || []).map((w) => html`<${Warning} key=${w.kind} w=${w} item=${item} />`)}
    ${files?.error ? html`<p class="form-error">${files.error}</p>` : files ? html`<${FilesView} src=${{ kind: "sort", id: item.id }} files=${files.files} main=${files.main} names=${names} compact=${true} />` : html`<p class="muted">Reading its files…</p>`}
  </div>`;
}

function Warning({ w, item }) {
  if (w.kind === "several") {
    return html`<p class="imp-warn" data-warn="several">Maybe several models: it has no model files of its own but ${w.parts.length} sub-folders with them (${w.parts.slice(0, 5).join(", ")}${w.parts.length > 5 ? "…" : ""}).${" "}
      <button type="button" class="linkish" onClick=${() => act("sort_split", { id: item.id })}>Split it</button></p>`;
  }
  if (w.kind === "duplicate") return html`<p class="imp-warn" data-warn="duplicate">Looks like <a href=${routeHash(`model:${w.id}`)}>${w.name}</a>, already in the library (same files and sizes).</p>`;
  if (w.kind === "in-library") return html`<p class="imp-note" data-warn="in-library">Already in the library at ${w.rel}; importing moves it.</p>`;
  if (w.kind === "empty") return html`<p class="imp-warn" data-warn="empty">No files: nothing to import.</p>`;
  return null;
}

function Results({ r }) {
  const ok = (r.results || []).filter((x) => !x.error);
  const bad = (r.results || []).filter((x) => x.error);
  return html`<section class="imp-results" id="import-results">
    <h2>${plural(ok.length, "model", "models")} ${r.mode === "copy" ? "copied" : "moved"} in${r.failed ? `, ${r.failed} not` : ""}</h2>
    ${bad.length ? html`<ul class="ls-list">${bad.map((x) => html`<li key=${x.source} class="bad"><span>${x.name}</span><span class="form-error">${x.error}</span></li>`)}</ul>` : null}
    <p><a href=${routeHash("browse:all")}>See all models</a> · <button type="button" class="linkish" onClick=${() => sorter.set({ results: null })}>Close</button></p>
  </section>`;
}

function JobBar({ job }) {
  const p = job.progress || {};
  const pct = p.total_bytes ? Math.round((100 * (p.bytes || 0)) / p.total_bytes) : null;
  return html`<div class="imp-progress" id="import-progress">
    ${pct != null ? html`<progress max="100" value=${pct}></progress>` : html`<progress></progress>`}
    <span class="sw-job-text">${p.items ? `${Math.min((p.item || 0) + 1, p.items)} of ${p.items}: ${p.name || ""} (${pct}%)` : p.read ? `${plural(p.read, "folder", "folders")} read: ${baseName(p.name || "")}` : `${job.label}…`}</span>
    <button type="button" class="ghost" id="import-stop" onClick=${() => api("job_cancel", { id: job.id })}>Stop</button></div>`;
}

export function ImportPage() {
  const s = useStore(sorter, (st) => st);
  const overview = useStore(ui, (st) => st.overview);
  const lib = useStore(ui, (st) => st.library);
  const view = useStore(ui, (st) => st.sortView || "folders");
  const names = lib?.variant_folders || [];
  const [sortBy, setSortBy] = useState("folder");
  useEffect(() => { if (lib) loadSession(); }, [lib?.path]);
  const se = s.session && s.library === lib?.path ? s.session : null;
  const ix = useMemo(() => (se ? indexOf(se) : null), [se]);
  const m = matcher(s);
  const rows = useMemo(() => (ix && view === "folders" ? folderRows(ix, s, m) : []), [ix, view, s.filter, s.q, s.open]);
  const items = useMemo(() => {
    if (!se || view === "folders") return [];
    const list = se.items.filter(m.item);
    const key = {
      name: (i) => i.name.toLowerCase(),
      folder: (i) => `${(i.parent || "").toLowerCase()}\u0000${i.name.toLowerCase()}`,
      files: (i) => -(i.summary?.count || 0),
      size: (i) => -(i.summary?.bytes || 0),
      place: (i) => `${status(i)}\u0000${placeLabel(overview, i.schema, i.values)}`,
    }[view === "list" ? sortBy : "folder"];
    return list.map((i) => [key(i), i]).sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0)).map((x) => x[1]);
  }, [se, view, s.filter, s.q, sortBy, overview]);
  if (!lib) return html`<div class="pages"><h1>Import</h1><p>Open a library first.</p></div>`;
  const order = view === "folders" ? rows.slice(0, s.limit).map((r) => r.key) : items.slice(0, s.limit).map((i) => `i:${i.id}`);
  const pick = (k, how) => {
    const st = sorter.get();
    if (how === "toggle" || how === "tick") {
      const picked = st.picked.includes(k) ? st.picked.filter((x) => x !== k) : [...st.picked, k];
      sorter.set({ picked, anchor: k, ...(how === "toggle" ? { focus: k } : {}) });
    } else if (how === "range" && st.anchor && order.includes(st.anchor) && order.includes(k)) {
      const [a, b] = [order.indexOf(st.anchor), order.indexOf(k)].sort((x, y) => x - y);
      sorter.set({ picked: order.slice(a, b + 1), focus: k });
    } else {
      sorter.set({ picked: [k], anchor: k, focus: k });
    }
  };
  const sel = ix ? selectionOf(s, ix) : null;
  const counts = { todo: 0, sorted: 0, skipped: 0, done: 0, all: se?.items.length || 0 };
  for (const i of se?.items || []) counts[status(i)]++;
  const ready = counts.sorted;
  const p = ctx.platform;
  const busy = !!s.job || !!se?.busy;
  const pickAndAdd = async (contents, file) => {
    const path = file ? await p.library.pickFile("Choose a model file or archive") : await p.library.pickFolder(contents ? "Choose a folder to sort" : "Choose a model's folder");
    if (path) await addSources([path], contents);
  };
  const importSorted = async () => {
    const done = await runJob("sort_commit", { mode: s.mode, force_copy: !!window.__forceCopy }, s.mode === "copy" ? "Copying models in" : "Moving models in");
    if (done?.result) sorter.set({ results: done.result });
    await loadOverview();
  };
  const startAgain = () => {
    if (!confirm("Forget everything in the workspace? Nothing on disk changes; what's imported stays imported.")) return;
    act("sort_clear", {}).then(() => sorter.set({ picked: [], focus: null, results: null }));
  };
  const empty = !se || (!se.roots.length && !se.items.length);
  return html`<div class="sort-page" id="sort-page">
    <div class="sw-head">
      <h1>Import</h1>
      <div class="home-actions">
        <button type="button" class="primary" id="import-sort" disabled=${busy} onClick=${() => pickAndAdd(true)}>${Icon.folder(15)} Sort a folder…</button>
        <button type="button" class="ghost" id="import-add-folder" disabled=${busy} onClick=${() => pickAndAdd(false)}>Add a model folder…</button>
        <button type="button" class="ghost" id="import-add-file" disabled=${busy} onClick=${() => pickAndAdd(false, true)}>Add a file…</button>
        ${!empty ? html`<button type="button" class="ghost" id="sort-rescan" disabled=${busy} title="Pick up files added or removed since, keeping what you decided" onClick=${() => runJob("sort_rescan", {}, "Reading the folders again")}>${Icon.refresh(14)} Read again</button>` : null}
        ${counts.done ? html`<button type="button" class="ghost" id="sort-forget-done" disabled=${busy} onClick=${() => act("sort_clear", { imported: true })}>Clear imported</button>` : null}
        ${!empty ? html`<button type="button" class="ghost" id="import-clear" disabled=${busy} onClick=${startAgain}>Start again</button>` : null}
      </div>
    </div>
    ${lib.read_only ? html`<p class="warn-note">${lib.read_only}</p>` : null}
    ${s.error ? html`<p class="form-error" role="alert">${s.error}</p>` : null}
    ${s.job ? html`<${JobBar} job=${s.job} />` : se?.busy ? html`<p class="muted">The workspace is busy reading folders or importing. <button type="button" class="linkish" onClick=${loadSession}>Check again</button></p>` : null}
    ${s.results ? html`<${Results} r=${s.results} />` : null}
    ${empty ? html`<div class="empty sw-empty">
        <p>Sort a folder of models (on a NAS, a drive, or already inside the library): it's shown as it is, with the models the app finds in it all the way down. Pick one or many and send them to a category; nothing moves until you press Import.</p>
        <p>You can also add model folders and files one by one, or drop them on this window. What you sort here is kept, so you can carry on another time.</p></div>` : html`
      <div class="sw-bar">
        <div class="seg" role="group" aria-label="View">${VIEWS.map(([k, label, icon]) => html`<button type="button" key=${k} data-view=${k} aria-pressed=${view === k ? "true" : "false"} title=${label} onClick=${() => setPref({ sortView: k })}>${Icon[icon](14)} <span class="sw-view-label">${label}</span></button>`)}</div>
        <div class="seg sw-filters" role="group" aria-label="Show">${FILTERS.map(([k, label]) => html`<button type="button" key=${k} data-filter=${k} aria-pressed=${s.filter === k ? "true" : "false"} onClick=${() => sorter.set({ filter: k, limit: 400 })}>${label} <span class="sw-count">${counts[k]}</span></button>`)}</div>
        <input type="search" class="sw-search" id="sort-search" placeholder="Find by name" aria-label="Find by name" value=${s.q} onInput=${(e) => sorter.set({ q: e.target.value, limit: 400 })} />
        ${view === "folders" ? html`<span class="sw-folds"><button type="button" class="ghost" onClick=${() => sorter.set({ open: Object.fromEntries(se.folders.map((f) => [f.path, true])) })}>Unfold all</button>
          <button type="button" class="ghost" onClick=${() => sorter.set({ open: Object.fromEntries(se.folders.filter((f) => f.parent).map((f) => [f.path, false])) })}>Fold all</button></span>` : null}
      </div>
      <div class="sw-main">
        <div class="sw-view">
          ${s.picked.length ? html`<${Actions} s=${s} sel=${sel} ix=${ix} overview=${overview} />` : html`<p class="muted sw-tip">Click to pick, Ctrl-click to pick more, Shift-click for a run; a picked folder takes everything in it.</p>`}
          ${view === "folders" ? html`<${FoldersView} rows=${rows} s=${s} sel=${sel} pick=${pick} overview=${overview} />`
            : view === "list" ? html`<${ListView} items=${items} s=${s} sel=${sel} pick=${pick} overview=${overview} ix=${ix} sortBy=${sortBy} setSortBy=${setSortBy} />`
            : view === "grid" ? html`<${GridView} items=${items} s=${s} pick=${pick} overview=${overview} />`
            : html`<${CategoryView} items=${items} s=${s} sel=${sel} pick=${pick} overview=${overview} ix=${ix} />`}
          ${(view === "folders" ? !rows.length : !items.length) ? html`<p class="muted sw-none">${s.q ? "Nothing matches." : s.filter === "todo" ? "Everything here is sorted." : "Nothing here."}</p>` : null}
        </div>
        <${Details} s=${s} ix=${ix} overview=${overview} names=${names} />
      </div>
      <div class="imp-go sw-go">
        <div class="seg" role="group" aria-label="Move or copy">
          <button type="button" id="mode-move" aria-pressed=${s.mode === "move" ? "true" : "false"} onClick=${() => sorter.set({ mode: "move" })}>Move</button>
          <button type="button" id="mode-copy" aria-pressed=${s.mode === "copy" ? "true" : "false"} onClick=${() => sorter.set({ mode: "copy" })}>Copy</button>
        </div>
        <span class="muted imp-mode-note">${s.mode === "move" ? "Files move into the library. Across drives they're copied, checked, then the originals deleted." : "The originals stay where they are; every copy is checked."}</span>
        <button type="button" class="primary" id="import-go" disabled=${!ready || busy || !!lib.read_only} onClick=${importSorted}>${!ready ? "Nothing sorted to import" : `${s.mode === "copy" ? "Copy" : "Import"} ${plural(ready, "sorted model", "sorted models")}`}</button>
      </div>`}
  </div>`;
}

/** Folders and files dropped on the window: each one is a model. */
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
