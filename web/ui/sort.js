// The sorting workspace, on the Import page (docs/PLAN.md, "Phase 5 design"):
// folders read as they are on disk, with the models the app proposes in them all
// the way down. Select one or many (click, Ctrl- or Shift-click, tick, Ctrl+A or
// the arrow keys; a selected folder takes everything in it) and set their
// category and subcategory, keeping their folders as subcategories if you like;
// combine files and folders into one model, or split one; then import what has a
// category. Every change here can be undone (the core keeps the workspace as it
// was). The workspace is kept on this computer, so sorting can go on over
// several sittings.
import { html, useState, useEffect, useMemo } from "../lib/html.js";
import { createStore, useStore } from "../lib/store.js";
import { ui, setPref } from "./state.js";
import { ctx, routeHash } from "./context.js";
import { Icon } from "./icons.js";
import { api, followJob, isDesktop, loadOverview, toast, undoable, undoChange } from "./library.js";
import { openMenu, usePageKeys, letter, selectAllKey } from "./actions.js";
import { CategoryPicker } from "./category.js";
import { size } from "./details.js";
import { FilesView, LazyPreview } from "./parts.js";
import { PageHead, ViewSwitch, SortMenu, SearchNote, EditBox } from "./layout.js";
import { TypeTag, TypeTags } from "./filetypes.js";
import { CategoryImport, categoryStage, openCategoryImport } from "./categoryimport.js";

/** The workspace as the core keeps it, and what's selected here. */
export const sorter = createStore({
  session: null,     // sort_get: { roots, folders, items, left, busy, read }
  library: null,     // the library it's for (its path)
  picked: [],        // what's selected, as keys: "i:<item id>", "f:<folder path>", "l:<file path>"
  anchor: null,      // the key clicked last (Shift-click selects from it)
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
const dirName = (p) => p.replace(/[\\/][^\\/]*$/, "");
const under = (p, dir) => !!p && (p === dir || p.startsWith(`${dir}/`) || p.startsWith(`${dir}\\`));
const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;

export const status = (i) => (i.done ? "done" : i.skip ? "skipped" : i.placed ? "sorted" : "todo");
const FILTERS = [["todo", "No category"], ["sorted", "Has category"], ["skipped", "Skipped"], ["done", "Imported"], ["all", "All"]];
const VIEWS = [["folders", "Folders", "folder"], ["list", "List", "table"], ["grid", "Grid", "grid"], ["category", "By category", "grouped"]];
const SORT_BY = [["folder", "Folder"], ["name", "Name"], ["files", "Most files"], ["size", "Largest first"], ["place", "Where it goes"]];

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

/** Put the workspace back as it was before change `rev`. */
const undoTo = (rev) => async () => {
  const session = await api("sort_undo", { rev });
  sorter.set({ session, error: "" });
};

/** A change to the workspace: the core answers with the whole of it. `said(session)`
 *  is the message, with Undo in it. */
async function act(cmd, args, said) {
  try {
    const session = await api(cmd, args);
    sorter.set({ session, error: "" });
    if (said && session.undo != null) undoable(said(session), undoTo(session.undo));
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
  if (!done || done.error) return;
  if (!done.result?.items) return toast("Nothing to sort there.");
  const rev = sorter.get().session?.undo;
  const what = paths.length === 1 ? baseName(paths[0]) : plural(paths.length, "thing", "things");
  if (rev != null) undoable(`Added ${what} to the workspace.`, undoTo(rev));
}

/** Read the folders again, keeping what was decided. */
async function readAgain() {
  const done = await runJob("sort_rescan", {}, "Reading the folders again");
  const rev = sorter.get().session?.undo;
  if (done && !done.error && rev != null) undoable("Read the folders again.", undoTo(rev));
}

const useGuess = (ids, folders = []) => act("sort_update", { ids, folders, patch: { accept: true } }, () => "They'll go to their suggested categories.");

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
      <button type="button" class="linkish sw-use" title="Use the suggested category" onClick=${(e) => { e.stopPropagation(); useGuess([item.id]); }}>Use</button></span>`;
  }
  return html`${err}<span class="sw-chip todo">No category</span>`;
}

const how = (e) => (e.shiftKey ? "range" : e.ctrlKey || e.metaKey ? "toggle" : "one");

function Tick({ k, on, covered, label, pick }) {
  return html`<input type="checkbox" class="sw-tick" tabindex="-1" checked=${on || covered} disabled=${covered} aria-label=${`Select ${label}`}
    onClick=${(e) => { e.stopPropagation(); pick(k, e.shiftKey ? "range" : "tick"); }} />`;
}

function FolderRow({ r, s, sel, pick, menu }) {
  const f = r.folder;
  const on = s.picked.includes(r.key);
  const covered = !on && sel.inPicked(f.parent) ;
  const toggle = (e) => { e.stopPropagation(); sorter.set((st) => ({ open: { ...st.open, [f.path]: !r.open } })); };
  return html`<li class=${`sw-row sw-folder${on ? " picked" : ""}${s.focus === r.key ? " focused" : ""}`} data-key=${r.key} data-name=${f.name}
      style=${`--depth:${r.depth}`} aria-selected=${on || covered ? "true" : "false"} onClick=${(e) => pick(r.key, how(e))} onContextMenu=${(e) => menu(e, r.key)}>
    <${Tick} k=${r.key} on=${on} covered=${covered} label=${f.name} pick=${pick} />
    <button type="button" class="sw-fold" aria-expanded=${r.open ? "true" : "false"} aria-label=${`${r.open ? "Fold" : "Unfold"} ${f.name}`} onClick=${toggle}>${r.open ? "▾" : "▸"}</button>
    <span class="sw-icon sw-mark-folder" title="Folder">${Icon.folder(14)}</span>
    <span class="sw-name">${f.name}</span>
    <span class="sw-meta muted">${r.count ? plural(r.count, "model", "models") : ""}${r.count && r.left ? " · " : ""}${r.left ? plural(r.left, "loose file", "loose files") : ""}</span>
  </li>`;
}

function ItemRow({ item, depth = 0, s, sel, pick, menu, overview, where }) {
  const k = `i:${item.id}`;
  const on = s.picked.includes(k);
  const covered = !on && sel.inPicked(item.parent);
  const sm = item.summary || {};
  return html`<li class=${`sw-row sw-item sw-${status(item)}${on ? " picked" : ""}${s.focus === k ? " focused" : ""}`} data-key=${k} data-name=${item.name}
      style=${`--depth:${depth}`} aria-selected=${on || covered ? "true" : "false"} onClick=${(e) => pick(k, how(e))} onContextMenu=${(e) => menu(e, k)}>
    <${Tick} k=${k} on=${on} covered=${covered} label=${item.name} pick=${pick} />
    <span class="sw-fold-gap"></span>
    <span class="sw-icon sw-mark-model" title=${item.kind === "group" ? "Combined model" : item.kind === "folder" ? "Model folder" : "Model"}>${item.kind === "group" ? Icon.layers(14) : Icon.box(14)}</span>
    <span class="sw-name">${item.name}${item.author ? html` <span class="muted">(${item.author})</span>` : null} <${TypeTags} exts=${item.summary?.exts} />${where ? html`<span class="sw-where muted">${where}</span>` : null}</span>
    <span class="sw-meta muted">${plural(sm.count || 0, "file", "files")} · ${size(sm.bytes || 0)}</span>
    <${Place} item=${item} overview=${overview} />
  </li>`;
}

function LeftRow({ l, depth, s, sel, pick, menu }) {
  const k = `l:${l.path}`;
  const on = s.picked.includes(k);
  const covered = !on && sel.inPicked(l.parent);
  return html`<li class=${`sw-row sw-left${on ? " picked" : ""}${s.focus === k ? " focused" : ""}`} data-key=${k} data-name=${l.name}
      style=${`--depth:${depth}`} aria-selected=${on || covered ? "true" : "false"} onClick=${(e) => pick(k, how(e))} onContextMenu=${(e) => menu(e, k)} title="Not part of any model">
    <${Tick} k=${k} on=${on} covered=${covered} label=${l.name} pick=${pick} />
    <span class="sw-fold-gap"></span>
    <span class="sw-icon sw-mark-file"><${TypeTag} name=${l.name} /></span>
    <span class="sw-name">${l.name}</span>
    <span class="sw-meta muted">${size(l.size)}</span>
  </li>`;
}

function More({ shown, total }) {
  if (shown >= total) return null;
  return html`<button type="button" class="ghost sw-more" onClick=${() => sorter.set((s) => ({ limit: s.limit + 1000 }))}>Show more (${total - shown} left)</button>`;
}

function FoldersView({ rows, s, sel, pick, menu, overview }) {
  const shown = rows.slice(0, s.limit);
  return html`<ul class="sw-tree" id="sort-tree" role="listbox" aria-multiselectable="true">${shown.map((r) =>
    r.type === "folder" ? html`<${FolderRow} key=${r.key} r=${r} s=${s} sel=${sel} pick=${pick} menu=${menu} />`
      : r.type === "item" ? html`<${ItemRow} key=${r.key} item=${r.item} depth=${r.depth} s=${s} sel=${sel} pick=${pick} menu=${menu} overview=${overview} />`
      : html`<${LeftRow} key=${r.key} l=${r.left} depth=${r.depth} s=${s} sel=${sel} pick=${pick} menu=${menu} />`)}</ul>
    <${More} shown=${shown.length} total=${rows.length} />`;
}

const COLS = [["name", "Name"], ["folder", "Folder"], ["files", "Files"], ["size", "Size"], ["place", "Where it goes"]];

function ListView({ items, s, sel, pick, menu, overview, ix, sortBy, setSortBy }) {
  const shown = items.slice(0, s.limit);
  return html`<table class="sw-table" id="sort-list">
    <thead><tr><th></th>${COLS.map(([k, label]) => html`<th key=${k}><button type="button" class="linkish" aria-pressed=${sortBy === k ? "true" : "false"} onClick=${() => setSortBy(k)}>${label}${sortBy === k ? " ▾" : ""}</button></th>`)}</tr></thead>
    <tbody>${shown.map((i) => {
      const k = `i:${i.id}`;
      const on = s.picked.includes(k);
      return html`<tr key=${k} data-key=${k} data-name=${i.name} class=${`${on ? "picked" : ""}${s.focus === k ? " focused" : ""}`} aria-selected=${on ? "true" : "false"} onClick=${(e) => pick(k, how(e))} onContextMenu=${(e) => menu(e, k)}>
        <td><${Tick} k=${k} on=${on} covered=${false} label=${i.name} pick=${pick} /></td>
        <td class="sw-name">${i.name}${i.author ? html` <span class="muted">(${i.author})</span>` : null} <${TypeTags} exts=${i.summary?.exts} /></td>
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

function GridView({ items, s, pick, menu, overview }) {
  const shown = items.slice(0, Math.min(s.limit, 300));
  return html`<div class="grid-view sw-grid" id="sort-grid">${shown.map((i) => {
    const k = `i:${i.id}`;
    const on = s.picked.includes(k);
    return html`<div key=${k} class=${`card sw-card sw-${status(i)}${s.focus === k ? " focused" : ""}`} data-key=${k} data-name=${i.name} aria-selected=${on ? "true" : "false"} onClick=${(e) => pick(k, how(e))} onContextMenu=${(e) => menu(e, k)}>
      <div class="sw-card-top"><${Tick} k=${k} on=${on} covered=${false} label=${i.name} pick=${pick} /></div>
      <div class="thumb sw-thumb"><${LazyPreview} cacheKey=${`sort:${i.id}:${i.summary?.bytes}`} ask=${() => api("sort_preview", { id: i.id })} fallback=${html`<span class="tile-icon">${Icon.box(34)}</span>`} alt=${i.name} /></div>
      <div class="card-name">${i.name}</div>
      <div class="card-sub">${i.author || html`<span>${plural(i.summary?.count || 0, "file", "files")}</span>`}</div>
      <div class="sw-card-types"><${TypeTags} exts=${i.summary?.exts} /></div>
      <div class="sw-card-place"><${Place} item=${i} overview=${overview} /></div>
    </div>`;
  })}</div>
  <${More} shown=${shown.length} total=${items.length} />`;
}

function CategoryView({ items, s, sel, pick, menu, overview, ix }) {
  const groups = new Map();
  const add = (key, label, i) => { if (!groups.has(key)) groups.set(key, { label, items: [] }); groups.get(key).items.push(i); };
  for (const i of items) {
    const st = status(i);
    if (st === "todo") add("0", "No category", i);
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
      <h3><label class="sw-group-pick"><input type="checkbox" checked=${all} aria-label=${`Select everything in ${g.label}`}
        onChange=${(e) => { const ks = g.items.map((i) => `i:${i.id}`); sorter.set((st) => ({ picked: e.target.checked ? [...new Set([...st.picked, ...ks])] : st.picked.filter((x) => !ks.includes(x)) })); }} />
        ${g.label}</label> <span class="muted">${g.items.length}</span></h3>
      <ul class="sw-tree">${shown.map((i) => html`<${ItemRow} key=${i.id} item=${i} s=${s} sel=${sel} pick=${pick} menu=${menu} overview=${overview} where=${folderOf(i, ix)} />`)}</ul>
    </section>`;
  })}</div>
  <${More} shown=${Math.min(s.limit, items.length)} total=${items.length} />`;
}

/** Put the cursor in a box of the details pane or the actions (after they're drawn). */
const focusBox = (sel, select = false) => setTimeout(() => {
  const el = document.querySelector(sel);
  el?.focus();
  if (select) el?.select?.();
}, 0);

/** The folder a selection is in, on disk (for Show in folder), when one thing is selected. */
function folderOfSelection(sel, ix) {
  if (sel.count !== 1) return null;
  if (sel.folders.length) return sel.folders[0];
  if (sel.files.length) return ix.leftByPath.get(sel.files[0])?.parent || dirName(sel.files[0]);
  const i = ix.byId.get(sel.ids[0]);
  if (!i) return null;
  if (i.done) return `${ui.get().library?.path}/${i.done.rel}`;
  return i.kind === "folder" ? i.path : i.parent || dirName(i.sources?.[0] || i.path || "");
}

const depthOf = (p) => p.split(/[\\/]/).filter(Boolean).length;

/** The folders shown at the same depth as `path` in the folder it was added with. */
function levelOf(se, ix, path) {
  const root = ix.rootOf(path);
  const d = depthOf(path);
  return se.folders.filter((f) => ix.rootOf(f.path) === root && depthOf(f.path) === d).map((f) => f.path);
}

/** Make a folder one model (everything in it becomes its parts) and select it. */
async function joinFolder(folder) {
  const r = await act("sort_join", { folder: folder.path }, () => `Made ${folder.name} one model.`);
  if (r?.id) sorter.set({ picked: [`i:${r.id}`], focus: `i:${r.id}`, anchor: `i:${r.id}` });
}

const joinedText = (r) => `Made ${plural(r.ids.length, "folder", "folders")} into models${r.left?.length ? `; not ${r.left.join(", ")}, which ${r.left.length === 1 ? "is" : "are"} grouped with files outside ${r.left.length === 1 ? "it" : "them"}` : ""}.`;

/** What can be done to what's selected: the actions panel's buttons and the
 *  right-click menu. Shared ones first (Edit details, Set category, Show in
 *  folder), as everywhere, then the workspace's own. */
function importActions(s, sel, ix, overview) {
  const n = sel.items.length;
  const one = sel.ids.length === 1 && !sel.folders.length && !sel.files.length ? ix.byId.get(sel.ids[0]) : null;
  const oneFolder = sel.folders.length === 1 && !sel.ids.length && !sel.files.length ? ix.folderByPath.get(sel.folders[0]) : null;
  const select = (id) => sorter.set({ picked: [`i:${id}`], focus: `i:${id}`, anchor: `i:${id}` });
  const named = (r, id) => ix.byId.get(id)?.name || r.items.find((i) => i.id === id)?.name || "one model";
  const patch = (p, said) => act("sort_update", { ids: sel.ids, folders: sel.folders, patch: p }, said);
  const allSkipped = n > 0 && sel.items.every((i) => i.skip);
  const where = isDesktop() ? folderOfSelection(sel, ix) : null;
  const list = [];
  if (one?.done) list.push({ id: "open", label: "Open", icon: "eye", key: "Enter", run: () => { location.hash = routeHash(`model:${one.done.id}`); } });
  if (one && !one.done) list.push({ id: "edit", label: "Edit details", icon: "edit", key: "E", run: () => { sorter.set({ focus: `i:${one.id}` }); focusBox("#details-name"); } });
  if (n) list.push({ id: "set-category", label: "Set category…", icon: "move", key: "M", run: () => focusBox("#send-schema") });
  if (where) list.push({ id: "folder", label: "Show in folder", icon: "folder", run: () => ctx.platform.library.openPath(where) });
  const own = [];
  // which folder is the model: this one, every one at its level, or each of several
  if (oneFolder) {
    own.push({ id: "sort-join", label: "Make this folder one model", icon: "box", title: "Everything in it becomes the model's parts", run: () => joinFolder(oneFolder) });
    const level = levelOf(s.session, ix, oneFolder.path);
    if (level.length > 1) {
      own.push({ id: "sort-join-level", label: `Make every folder at this level a model (${level.length})`, icon: "box",
        title: `${level.slice(0, 12).map(baseName).join(", ")}${level.length > 12 ? "…" : ""}`, run: async () => {
          const r = await act("sort_join", { folder: oneFolder.path, level: true }, joinedText);
          if (r) sorter.set({ picked: [], anchor: null });
        } });
    }
  }
  if (sel.folders.length >= 2 && !sel.ids.length && !sel.files.length) {
    own.push({ id: "sort-join-each", label: "Make each folder a model", icon: "box", run: async () => {
      const r = await act("sort_join", { folders: sel.folders }, joinedText);
      if (r) sorter.set({ picked: [], anchor: null });
    } });
  }
  if (sel.count >= 2 || (sel.files.length === 1 && sel.count === 1)) {
    own.push({ id: "sort-group", label: "Combine into one model", icon: "layers", run: async () => {
      const r = await act("sort_group", { ids: sel.ids, folders: sel.folders, files: sel.files }, (r) => `Combined into one model: ${named(r, r.id)}.`);
      if (r?.id) select(r.id);
    } });
  }
  if (one && !one.done && one.kind !== "files") {
    own.push({ id: "sort-split", label: "Split into models", title: one.kind === "group" ? "Back to what it was combined from" : "Read it as a folder of models: one for each folder in it, and for each group of loose files", run: async () => {
      const r = await act("sort_split", { id: one.id }, () => `Split ${one.name}.`);
      if (r) sorter.set({ picked: [], anchor: null });
    } });
  }
  if (n) own.push({ id: "sort-skip", label: allSkipped ? "Don't skip" : "Skip", run: () => patch({ skip: !allSkipped }, () => (allSkipped ? `${plural(n, "model is", "models are")} not skipped any more.` : `Skipped ${plural(n, "model", "models")}.`)) });
  if (sel.items.some((i) => i.placed)) own.push({ id: "sort-unsend", label: "Clear category", run: () => patch({ clear: true }, () => `Cleared the category of ${plural(sel.items.filter((i) => i.placed).length, "model", "models")}.`) });
  if (sel.items.some((i) => i.guess?.schema && !i.placed)) own.push({ id: "sort-use-guess", label: "Use suggested categories", run: () => useGuess(sel.ids, sel.folders) });
  return { shared: list, own, n };
}

/** What to do with what's selected: set its category, combine it, split it… */
function Actions({ s, sel, ix, overview }) {
  const send = s.send;
  const setSend = (patch) => sorter.set((st) => ({ send: { ...st.send, ...patch } }));
  const { shared, own, n } = importActions(s, sel, ix, overview);
  const clear = () => sorter.set({ picked: [], anchor: null });
  const where = placeLabel(overview, send.schema, send.values);
  const go = async () => {
    const keep = send.keep && !!sel.folders.length && !!send.schema;
    const r = await act("sort_send", { ids: sel.ids, folders: sel.folders, schema: send.schema, values: send.values, keep: send.keep && !!sel.folders.length, keep_self: send.keepSelf },
      (r) => `Set ${plural(r.changed, "model", "models")} to go to ${where}${keep ? ", with their folders" : ""}. Nothing moves until you press Import.`);
    if (r) clear();
  };
  const what = [sel.ids.length ? plural(sel.ids.length, "model", "models") : "", sel.folders.length ? `${plural(sel.folders.length, "folder", "folders")} (${plural(n - sel.ids.length, "model", "models")} in ${sel.folders.length === 1 ? "it" : "them"})` : "", sel.files.length ? plural(sel.files.length, "loose file", "loose files") : ""].filter(Boolean).join(", ");
  return html`<div class="sw-actions" id="sort-actions">
    <div class="sw-actions-head"><strong id="sort-picked">${what ? `${what} selected` : "Nothing selected"}</strong>
      <button type="button" class="ghost" onClick=${clear} aria-label="Clear selection" title="Clear selection (Esc)">${Icon.close(13)}</button></div>
    ${n ? html`<div class="sw-send" onKeyDown=${(e) => { if (e.key === "Enter" && e.target.tagName === "INPUT" && e.target.type === "text") { e.preventDefault(); go(); } }}>
      <span class="field-label">Category</span>
      <${CategoryPicker} overview=${overview} schema=${send.schema} values=${send.values} idPrefix="send" onChange=${(schema, values) => setSend({ schema, values })} />
      ${sel.folders.length && send.schema ? html`<div class="sw-keep">
        <label class="check-row"><input type="checkbox" id="send-keep" checked=${send.keep} onChange=${(e) => setSend({ keep: e.target.checked })} /> Keep ${sel.folders.length === 1 ? "its" : "their"} folders as subcategories</label>
        ${send.keep ? html`<label class="check-row"><input type="checkbox" id="send-keep-self" checked=${send.keepSelf} onChange=${(e) => setSend({ keepSelf: e.target.checked })} /> Starting with ${sel.folders.length === 1 ? `“${baseName(sel.folders[0])}” itself` : "the selected folders themselves"}</label>` : null}
      </div>` : null}
      <button type="button" class="primary" id="send-go" title="Nothing moves until you press Import" onClick=${go}>Set category</button>
    </div>` : null}
    <div class="sw-buttons">
      ${shared.filter((a) => a.id !== "set-category").map((a) => html`<button type="button" class="ghost" key=${a.id} id=${`sort-${a.id}`} title=${a.key ? `${a.label} (${a.key})` : null} onClick=${a.run}>${a.icon ? Icon[a.icon](14) : null} ${a.label}</button>`)}
      ${own.map((a) => html`<button type="button" class="ghost" key=${a.id} id=${a.id} title=${a.title || null} onClick=${a.run}>${a.icon ? Icon[a.icon](14) : null} ${a.label}</button>`)}
    </div>
  </div>`;
}

/** The details panel, as in the library (docs/PLAN.md, "UI pass design", step 3):
 *  what's selected (one model's name, authors and tags changed right here, a
 *  folder, a loose file, or several), where it goes, what can be done to it, then
 *  its files. With nothing selected, what the workspace holds. */
function Details({ s, sel, ix, overview, names, counts }) {
  const k = s.picked.length === 1 ? s.picked[0] : "";
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
  const actions = sel?.count ? html`<${Actions} s=${s} sel=${sel} ix=${ix} overview=${overview} />` : null;
  if (folder) {
    const inside = s.session.items.filter((i) => under(i.parent, folder.path));
    const todo = inside.filter((i) => status(i) === "todo").length;
    return html`<div class="sw-details" id="sort-details">
      <h2>${Icon.folder(18)} ${folder.name}</h2>
      <p class="muted sw-path">${folder.path}</p>
      <p>${plural(inside.length, "model", "models")} in it${inside.length ? `, ${todo} with no category yet` : ""}.</p>
      ${actions}
      <p class="muted sw-hint">If this folder is one model and the folders in it are its parts, choose Make this folder one model; if every folder at this level is a model, choose Make every folder at this level a model. Setting a folder's category sets it for everything in it; tick “Keep its folders as subcategories” to bring its folders along.</p>
    </div>`;
  }
  if (left) {
    return html`<div class="sw-details" id="sort-details">
      <h2>${Icon.file(18)} ${left.name}</h2>
      <p class="muted sw-path">${left.path}</p>
      <p>${size(left.size)}. It isn't part of any model, so it stays where it is unless you make it into one.</p>
      ${actions}
    </div>`;
  }
  if (!item) {
    if (actions) return html`<div class="sw-details" id="sort-details">${actions}</div>`;
    return html`<div class="sw-details sw-details-empty" id="sort-details">
      <h2>${Icon.inbox(18)} Workspace</h2>
      <p id="sort-summary">${plural(counts.all, "model", "models")}: ${counts.todo} with no category, ${counts.sorted} ready to import${counts.skipped ? `, ${counts.skipped} skipped` : ""}${counts.done ? `, ${counts.done} imported` : ""}.</p>
      <p class="muted sw-hint">Click to select, Ctrl-click to add one, Shift-click for a run, Ctrl+A for everything shown; a selected folder takes everything in it. Right-click for what you can do, such as making a folder (or every folder at its level) one model.</p>
    </div>`;
  }
  const save = (field) => (v) => {
    const said = { name: `Renamed ${item.name} to ${v}.`, author: `Saved the authors of ${item.name}.`, tags: `Saved the tags of ${item.name}.` }[field];
    if (v !== (item[field] || "")) act("sort_update", { ids: [item.id], patch: { [field]: v } }, () => said);
  };
  const sm = item.summary || {};
  // the folders above it, from the one that was added: any of them can be the model instead
  const above = [];
  for (let p = item.parent; p && ix.folderByPath.has(p); p = ix.folderByPath.get(p).parent) above.unshift(ix.folderByPath.get(p));
  return html`<div class="sw-details" id="sort-details" data-item=${item.id} key=${item.id}>
    <div class="panel-fields">
      <${EditBox} cls="panel-name" id="details-name" label="Name" title="Name (E or F2)" value=${item.name} off=${!!item.done} onSave=${save("name")} />
      <${EditBox} id="details-author" label="Authors" caption="Authors" placeholder="Authors, separated by commas" value=${item.author} off=${!!item.done} onSave=${save("author")} />
      <${EditBox} id="details-tags" label="Tags" caption="Tags" placeholder="Tags, separated by commas" value=${item.tags} off=${!!item.done} onSave=${save("tags")} />
    </div>
    ${!item.done && above.length ? html`<p class="sw-level" id="sort-level" title="Click a folder above it to make that folder the model instead, with everything in it">
      <span class="field-label">Model folder</span>
      <span class="sw-crumbs">${above.map((f) => html`<button type="button" class="linkish sw-crumb" key=${f.path} data-folder=${f.name} title=${`Make ${f.name} the model, with everything in it`} onClick=${() => joinFolder(f)}>${f.name}</button><span class="muted"> › </span>`)}<b>${item.kind === "folder" ? baseName(item.path) : item.name}</b></span></p>` : null}
    <p class="sw-status"><${Place} item=${item} overview=${overview} />
      ${item.done ? html` <a href=${routeHash(`model:${item.done.id}`)}>${item.done.rel}</a>` : null}</p>
    ${item.error ? html`<p class="form-error">${item.error}</p>` : null}
    ${item.done ? null : actions}
    <p class="muted sw-path">${item.kind === "folder" ? "Folder" : item.kind === "group" ? `Combined from ${plural(item.sources.length, "thing", "things")}` : plural(item.sources.length, "file", "files")}: ${item.kind === "folder" ? item.path : item.sources.map(baseName).join(", ")}${" · "}${plural(sm.count || 0, "file", "files")}${kindsText(sm) ? ` (${kindsText(sm)})` : ""} · ${size(sm.bytes || 0)}${item.has_sidecar ? " · has its own model.json" : ""}</p>
    ${(item.warnings || []).map((w) => html`<${Warning} key=${w.kind} w=${w} item=${item} />`)}
    ${files?.error ? html`<p class="form-error">${files.error}</p>` : files ? html`<${FilesView} src=${{ kind: "sort", id: item.id }} files=${files.files} main=${files.main} names=${names} compact=${true} />` : html`<p class="muted">Reading its files…</p>`}
  </div>`;
}

function Warning({ w, item }) {
  if (w.kind === "several") {
    return html`<p class="imp-warn" data-warn="several">Maybe several models: it has no model files of its own but ${w.parts.length} sub-folders with them (${w.parts.slice(0, 5).join(", ")}${w.parts.length > 5 ? "…" : ""}).${" "}
      <button type="button" class="linkish" onClick=${() => act("sort_split", { id: item.id }, () => `Split ${item.name}.`)}>Split it</button></p>`;
  }
  if (w.kind === "duplicate") return html`<p class="imp-warn" data-warn="duplicate">Looks like <a href=${routeHash(`model:${w.id}`)}>${w.name}</a>, already in the library (same files and sizes).</p>`;
  if (w.kind === "in-library") return html`<p class="imp-note" data-warn="in-library">Already in the library at ${w.rel}; importing moves it.</p>`;
  if (w.kind === "empty") return html`<p class="imp-warn" data-warn="empty">No files: nothing to import.</p>`;
  return null;
}

/** Undo an import: the files go back where they came from (copies are deleted while
 *  the originals are still there), and the workspace has them again. */
async function undoImport(r) {
  try {
    await undoChange(r.journal);
  } finally {
    sorter.set({ results: null });
    await loadSession();
  }
  return r.mode === "copy" ? "Undone: the copies are gone from the library; the originals are where they were." : "Undone: the models are back where they came from.";
}

function Results({ r }) {
  const ok = (r.results || []).filter((x) => !x.error);
  const bad = (r.results || []).filter((x) => x.error);
  return html`<section class="imp-results" id="import-results">
    <h2>${plural(ok.length, "model", "models")} ${r.mode === "copy" ? "copied" : "moved"} in${r.failed ? `, ${r.failed} not` : ""}</h2>
    ${bad.length ? html`<ul class="ls-list">${bad.map((x) => html`<li key=${x.source} class="bad"><span>${x.name}</span><span class="form-error">${x.error}</span></li>`)}</ul>` : null}
    <p><a href=${routeHash("browse:all")}>See all models</a>${r.undo ? html` · <button type="button" class="linkish" id="import-undo" title="Put the files back where they came from" onClick=${r.undo}>Undo</button>` : null} · <button type="button" class="linkish" id="import-results-close" onClick=${() => sorter.set({ results: null })}>Close</button></p>
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
  const stagedCategories = useStore(categoryStage, (st) => ({ open: st.open, library: st.library }));
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
    }[view === "category" ? "folder" : sortBy];
    return list.map((i) => [key(i), i]).sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0)).map((x) => x[1]);
  }, [se, view, s.filter, s.q, sortBy, overview]);
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
  // right-click: what's under the pointer is selected first, unless it already is
  const menu = (e, k) => {
    let st = sorter.get();
    if (!st.picked.includes(k)) { sorter.set({ picked: [k], anchor: k, focus: k }); st = sorter.get(); }
    const { shared, own } = importActions(st, selectionOf(st, ix), ix, overview);
    openMenu(e, [...shared, ...(shared.length && own.length ? [{ sep: true }] : []), ...own]);
  };
  const rowEl = (k) => document.querySelector(`#sort-page [data-key="${CSS.escape(k)}"]`);
  usePageKeys((e) => {
    if (!ix) return;
    const st = sorter.get();
    const l = letter(e);
    const cur = st.focus && order.includes(st.focus) ? st.focus : null;
    const row = cur && view === "folders" ? rows.find((r) => r.key === cur) : null;
    if (selectAllKey(e)) { e.preventDefault(); sorter.set({ picked: [...order], anchor: order[0] || null }); return; }
    if (e.key === "Escape") { if (st.picked.length) sorter.set({ picked: [], anchor: null }); return; }
    if (e.key === "ContextMenu") {
      const at = (cur && rowEl(cur)) || (st.picked[0] && rowEl(st.picked[0]));
      if (at) { const { shared, own } = importActions(st, selectionOf(st, ix), ix, overview); openMenu(at, [...shared, ...(shared.length && own.length ? [{ sep: true }] : []), ...own]); }
      return;
    }
    if (e.key === "Enter" || ((e.key === "ArrowRight" || e.key === "ArrowLeft") && row?.type === "folder")) {
      if (row?.type === "folder") {
        const open = e.key === "Enter" ? !row.open : e.key === "ArrowRight";
        if (open !== row.open) { e.preventDefault(); sorter.set((x) => ({ open: { ...x.open, [row.folder.path]: open } })); }
        return;
      }
      const it = cur?.startsWith("i:") ? ix.byId.get(cur.slice(2)) : null;
      if (e.key === "Enter" && it?.done) { e.preventDefault(); location.hash = routeHash(`model:${it.done.id}`); }
      return;
    }
    if ((l === "e" || e.key === "F2") && cur?.startsWith("i:") && !ix.byId.get(cur.slice(2))?.done) {
      e.preventDefault();
      if (st.picked.length !== 1 || st.picked[0] !== cur) sorter.set({ picked: [cur], anchor: cur }); // the panel shows it
      focusBox("#details-name", e.key === "F2");
      return;
    }
    if (l === "m" && selectionOf(st, ix).items.length) { e.preventDefault(); focusBox("#send-schema"); return; }
    if ((e.key === "ArrowDown" || e.key === "ArrowUp") && order.length && !e.ctrlKey && !e.metaKey && !e.altKey) {
      e.preventDefault();
      const i = cur ? order.indexOf(cur) : -1;
      const next = order[Math.max(0, Math.min(order.length - 1, i < 0 ? 0 : i + (e.key === "ArrowDown" ? 1 : -1)))];
      if (e.shiftKey) {
        const anchor = st.anchor && order.includes(st.anchor) ? st.anchor : cur || next;
        const [a, b] = [order.indexOf(anchor), order.indexOf(next)].sort((x, y) => x - y);
        sorter.set({ picked: order.slice(a, b + 1), focus: next, anchor });
      } else {
        sorter.set({ picked: [next], focus: next, anchor: next });
      }
      setTimeout(() => rowEl(next)?.scrollIntoView({ block: "nearest" }), 0);
    }
  });
  if (!lib) return html`<div class="pages"><h1>Import</h1><p>Open a library first.</p></div>`;
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
    const r = done?.result;
    if (r) {
      // Undo (in the message, on the results and Ctrl+Z) puts the files back where they came from
      const undo = r.journal ? undoable(`${plural(r.imported, "model", "models")} ${r.mode === "copy" ? "copied" : "moved"} in.`, () => undoImport(r)) : null;
      sorter.set({ results: { ...r, undo } });
    }
    await loadOverview();
  };
  const startAgain = () => act("sort_clear", {}, () => "Started again: the workspace is empty. Nothing on disk changed.")
    .then((r) => r && sorter.set({ picked: [], focus: null, results: null }));
  const empty = !se || (!se.roots.length && !se.items.length);
  const wait = busy ? "Wait for the work under way to finish." : false;
  const moreItems = () => [
    ...(!empty ? [{ id: "sort-rescan", label: "Read again", icon: "refresh", disabled: wait, title: "Pick up files added or removed since, keeping what you decided", run: readAgain }] : []),
    ...(counts.done ? [{ id: "sort-forget-done", label: "Clear imported", disabled: wait, title: "Take the imported models off this list",
      run: () => act("sort_clear", { imported: true }, () => `Cleared ${plural(counts.done, "imported model", "imported models")} from the list.`) }] : []),
    ...(!empty ? [{ id: "import-clear", label: "Start again", icon: "close", disabled: wait, title: "Empty the workspace (nothing on disk changes)", run: startAgain }] : []),
  ];
  const count = se && !empty ? `${plural(counts.all, "model", "models")}, ${counts.todo} with no category` : null;
  return html`<div class="sort-page" id="sort-page" data-ready=${se ? "1" : null}>
    <${PageHead} title="Import" count=${count} menu=${{ id: "import-more", items: moreItems, title: "Read or clear the sorting workspace" }}>
      <button type="button" class="primary" id="import-sort" disabled=${busy} onClick=${() => pickAndAdd(true)}>${Icon.folder(15)} Sort a folder…</button>
      <button type="button" class="ghost" id="import-folder" disabled=${busy} onClick=${() => pickAndAdd(false)}>${Icon.folder(15)} Import folder…</button>
      <button type="button" class="ghost" id="import-file" disabled=${busy} onClick=${() => pickAndAdd(false, true)}>${Icon.file(15)} Import file…</button>
      <button type="button" class="ghost" id="import-categories" disabled=${busy} onClick=${openCategoryImport}>${Icon.layers(15)} Import folders as categories…</button>
    </${PageHead}>
    ${lib.read_only ? html`<p class="warn-note">${lib.read_only}</p>` : null}
    ${stagedCategories.open && stagedCategories.library === lib.path ? html`<${CategoryImport} />` : html`
    ${s.error ? html`<p class="form-error" role="alert">${s.error}</p>` : null}
    ${s.job ? html`<${JobBar} job=${s.job} />` : se?.busy ? html`<p class="muted">The workspace is busy reading folders or importing. <button type="button" class="linkish" onClick=${loadSession}>Read again</button></p>` : null}
    ${s.results ? html`<${Results} r=${s.results} />` : null}
    ${!se ? (s.error ? null : html`<p class="muted" id="sort-loading">Reading the workspace…</p>`) : empty ? html`<div class="empty sw-empty">
        <p>Sort a folder of models (on a NAS, a drive, or already inside the library): it's shown as it is, with the models the app finds in it all the way down. Select one or many and set their category; nothing moves until you press Import.</p>
        <p>You can also add model folders and files one by one, or drop them on this window. What you sort here is kept, so you can carry on another time.</p></div>` : html`
      <div class="toolbar sw-bar">
        <input type="search" class="toolbar-search sw-search" id="sort-search" data-search placeholder="Search" aria-label="Search" title="Search (/)" value=${s.q} onInput=${(e) => sorter.set({ q: e.target.value, limit: 400 })} />
        ${view === "folders" ? html`<span class="sw-folds"><button type="button" class="ghost" id="sort-unfold" onClick=${() => sorter.set({ open: Object.fromEntries(se.folders.map((f) => [f.path, true])) })}>Unfold all</button>
          <button type="button" class="ghost" id="sort-fold" onClick=${() => sorter.set({ open: Object.fromEntries(se.folders.filter((f) => f.parent).map((f) => [f.path, false])) })}>Fold all</button></span>`
          : view !== "category" ? html`<${SortMenu} id="sort-by" options=${SORT_BY} value=${sortBy} onChange=${setSortBy} />` : null}
        <${ViewSwitch} views=${VIEWS} value=${view} onChange=${(k) => setPref({ sortView: k })} />
      </div>
      <div class="seg sw-filters" role="group" aria-label="Show">${FILTERS.map(([k, label]) => html`<button type="button" key=${k} data-filter=${k} aria-pressed=${s.filter === k ? "true" : "false"} onClick=${() => sorter.set({ filter: k, limit: 400 })}>${label} <span class="sw-count">${counts[k]}</span></button>`)}</div>
      <${SearchNote} q=${s.q.trim()} found=${`${plural(view === "folders" ? rows.filter((r) => r.type !== "folder").length : items.length, "match", "matches")}`} onClear=${() => sorter.set({ q: "", limit: 400 })} />
      <div class="sw-main">
        <div class="sw-view">
          ${view === "folders" ? html`<${FoldersView} rows=${rows} s=${s} sel=${sel} pick=${pick} menu=${menu} overview=${overview} />`
            : view === "list" ? html`<${ListView} items=${items} s=${s} sel=${sel} pick=${pick} menu=${menu} overview=${overview} ix=${ix} sortBy=${sortBy} setSortBy=${setSortBy} />`
            : view === "grid" ? html`<${GridView} items=${items} s=${s} pick=${pick} menu=${menu} overview=${overview} />`
            : html`<${CategoryView} items=${items} s=${s} sel=${sel} pick=${pick} menu=${menu} overview=${overview} ix=${ix} />`}
          ${(view === "folders" ? !rows.length : !items.length) ? html`<p class="muted sw-none">${s.q ? "Nothing matches." : s.filter === "todo" ? "Everything here has a category." : "Nothing here."}</p>` : null}
        </div>
        <${Details} s=${s} sel=${sel} ix=${ix} overview=${overview} names=${names} counts=${counts} />
      </div>
      <div class="imp-go sw-go">
        <div class="seg" role="group" aria-label="Move or copy">
          <button type="button" id="mode-move" aria-pressed=${s.mode === "move" ? "true" : "false"} onClick=${() => sorter.set({ mode: "move" })}>Move</button>
          <button type="button" id="mode-copy" aria-pressed=${s.mode === "copy" ? "true" : "false"} onClick=${() => sorter.set({ mode: "copy" })}>Copy</button>
        </div>
        <span class="muted imp-mode-note">${s.mode === "move" ? "Files move into the library. Across drives they're copied, checked, then the originals deleted." : "The originals stay where they are; every copy is checked."}</span>
        <button type="button" class="primary" id="import-go" disabled=${!ready || busy || !!lib.read_only} title="Import the models that have a category" onClick=${importSorted}>${!ready ? "Nothing to import yet" : `${s.mode === "copy" ? "Copy" : "Import"} ${plural(ready, "model", "models")}`}</button>
      </div>`}`}
  </div>`;
}

/** Open the Import page sorting a folder (Home's "Sort…" for loose library folders). */
export function sortFolder(path) {
  location.hash = routeHash("import");
  addSources([path], true);
}
