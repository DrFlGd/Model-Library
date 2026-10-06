// Subcategory trees (docs/PLAN.md, "Subcategory tree design"): choosing where a
// model goes (a category, or Unsorted, and a subcategory at any depth, maybe a new
// one), used by the Import page and the Move to category dialog; and the tree
// editor of the New and Edit category dialogs.
import { html, useState, useEffect, useLayoutEffect } from "../lib/html.js";
import { Icon } from "./icons.js";

const same = (a, b) => a.trim().toLowerCase() === b.trim().toLowerCase();

/** Every subcategory of a category from the overview, top first: { path, depth, count }. */
export function treeList(sc) {
  const out = [];
  const walk = (nodes, path) => {
    for (const n of nodes || []) {
      const p = [...path, n.value];
      out.push({ path: p, depth: path.length, count: n.count });
      walk(n.children, p);
    }
  };
  walk(sc?.tree, []);
  return out;
}

/** The overview node at `path` ([] is the category itself). */
export function nodeAt(sc, path) {
  let node = { children: sc?.tree || [], count: sc?.count || 0 };
  for (const v of path) {
    node = node.children.find((x) => same(x.value, v));
    if (!node) return null;
  }
  return node;
}

const parts = (t) => t.split("/").map((s) => s.trim()).filter(Boolean);
const KEY = "\u001f";

/** `schema` (id or null), `values` (the path of subcategories, maybe ending in new
 *  ones); `onChange(schema, values)`. A list of the category's subcategories, and a
 *  box for a new one inside the chosen one ("/" makes several levels). */
export function CategoryPicker({ overview, schema, values = [], onChange, idPrefix = "" }) {
  const schemas = overview?.schemas || [];
  const sc = schemas.find((s) => s.id === schema) || null;
  const known = (p) => !!nodeAt(sc, p);
  // the longest known start of `values` is chosen; the rest is new
  const splitAt = () => {
    let n = values.length;
    while (n > 0 && !known(values.slice(0, n))) n--;
    return n;
  };
  const [fresh, setFresh] = useState(() => values.slice(splitAt()).join(" / "));
  const fp = parts(fresh);
  const tail = values.slice(values.length - fp.length);
  const ours = fp.length <= values.length && fp.every((p, i) => p === tail[i]) && known(values.slice(0, values.length - fp.length));
  const at = ours ? values.length - fp.length : splitAt();
  const shown = ours ? fresh : values.slice(at).join(" / ");
  useEffect(() => { if (!ours) setFresh(shown); }, [ours, shown]);
  const sel = values.slice(0, at);
  const id = (s) => (idPrefix ? `${idPrefix}-${s}` : null);
  return html`<div class="cat-picker">
    <select class="cat-schema" id=${id("schema")} aria-label="Category" value=${schema || ""}
      onChange=${(e) => { setFresh(""); onChange(e.target.value || null, []); }}>
      <option value="">Unsorted</option>
      ${schemas.map((s) => html`<option value=${s.id} key=${s.id}>${s.name}</option>`)}
    </select>
    ${sc ? html`<select class="cat-place" id=${id("place")} aria-label="Subcategory" value=${sel.join(KEY)}
        onChange=${(e) => onChange(schema, [...(e.target.value ? e.target.value.split(KEY) : []), ...parts(shown)])}>
        <option value="">${`Top of ${sc.name}`}</option>
        ${treeList(sc).map((n) => html`<option value=${n.path.join(KEY)} key=${n.path.join(KEY)}>${n.path.join(" › ")}</option>`)}
      </select>
      <input type="text" class="cat-new" id=${id("new")} maxlength="160" aria-label="New subcategory inside it" placeholder="New subcategory (optional)"
        value=${shown} onInput=${(e) => { setFresh(e.target.value); onChange(schema, [...sel, ...parts(e.target.value)]); }} />` : null}
  </div>`;
}

let uidSeq = 0;
const uid = () => `st${++uidSeq}`;

/** Editor nodes from a category's overview tree: each keeps its path in the library. */
export function editorTree(sc) {
  const conv = (nodes, path) => (nodes || []).map((n) => {
    const p = [...path, n.value];
    return { uid: uid(), name: n.value, orig: p, count: n.count, children: conv(n.children, p) };
  });
  return conv(sc?.tree, []);
}

/** A new node left without a name and with nothing in it is dropped. */
const kept = (nodes) => nodes.filter((n) => n.name.trim() || n.orig || n.children.length);

/** What the core takes: [{ name, orig?, subcategories }]. */
export function treeSpec(nodes) {
  return kept(nodes).map((n) => ({ name: n.name.trim(), ...(n.orig ? { orig: n.orig } : {}), subcategories: treeSpec(n.children) }));
}

/** Whether every node that's kept has a name. */
export const treeReady = (nodes) => kept(nodes).every((n) => n.name.trim() && treeReady(n.children));

/** The first branch, top to bottom (for a preview of where models go). */
export const firstBranch = (nodes) => (nodes.length ? [nodes[0].name.trim() || "…", ...firstBranch(nodes[0].children)] : []);

/** Find a node: [the list it's in, its index, its parent (or null)]. */
function find(nodes, id, parent = null) {
  for (let i = 0; i < nodes.length; i++) {
    if (nodes[i].uid === id) return [nodes, i, parent];
    const r = find(nodes[i].children, id, nodes[i]);
    if (r) return r;
  }
  return null;
}

/** The node to focus once it's drawn (as soon as it is, so typing straight on lands in it). */
let focusNext = null;
const focus = (id) => { focusNext = id; };

/** The library paths in a removed branch (what was there moves up). */
const origs = (n) => [...(n.orig ? [n.orig] : []), ...n.children.flatMap(origs)];

/** The subcategory tree of a category: add a subcategory at the top or inside any
 *  one, rename, move one in (under the one above it) or out a level, remove one.
 *  `nodes` as from `editorTree`; `onChange(nodes, removed)`, `removed` being the
 *  library paths of a branch just removed. */
export function SubcategoryTree({ nodes, onChange, id = "subcat-tree", counts = false }) {
  useLayoutEffect(() => {
    if (!focusNext) return;
    const el = document.querySelector(`[data-uid="${focusNext}"]`);
    if (el) { el.focus(); focusNext = null; }
  });
  const edit = (fn) => {
    const copy = JSON.parse(JSON.stringify(nodes));
    const removed = fn(copy);
    onChange(copy, removed || []);
  };
  const add = (inside) => {
    const n = { uid: uid(), name: "", orig: null, count: 0, children: [] };
    edit((t) => {
      if (!inside) { t.push(n); return; }
      const [list, i] = find(t, inside);
      list[i].children.push(n);
    });
    focus(n.uid);
  };
  const after = (id) => {
    const n = { uid: uid(), name: "", orig: null, count: 0, children: [] };
    edit((t) => { const [list, i] = find(t, id); list.splice(i + 1, 0, n); });
    focus(n.uid);
  };
  const rename = (id, name) => edit((t) => { const [list, i] = find(t, id); list[i].name = name; });
  const remove = (id) => edit((t) => { const [list, i] = find(t, id); const [n] = list.splice(i, 1); return origs(n); });
  const indent = (id) => edit((t) => { const [list, i] = find(t, id); const [n] = list.splice(i, 1); list[i - 1].children.push(n); });
  const outdent = (id) => edit((t) => {
    const [list, i, parent] = find(t, id);
    const [n] = list.splice(i, 1);
    const [plist, pi] = find(t, parent.uid);
    plist.splice(pi + 1, 0, n);
  });
  const row = (n, i, list, depth, path) => {
    const p = [...path, n.name.trim()];
    const twin = list.some((m, j) => j !== i && m.name.trim() && same(m.name, n.name));
    return html`<li key=${n.uid} class="st-node">
      <div class="st-row" data-path=${p.join("/")} style=${`padding-left:${depth * 22}px`}>
        <span class="st-dot" aria-hidden="true">${depth ? "└" : "•"}</span>
        <input type="text" class="st-name" data-uid=${n.uid} maxlength="80" placeholder="Subcategory name" aria-label=${`Subcategory ${p.join(" › ") || "name"}`} value=${n.name}
          onInput=${(e) => rename(n.uid, e.target.value)}
          onKeyDown=${(e) => { if (e.key === "Enter") { e.preventDefault(); after(n.uid); } }} />
        ${counts ? html`<span class="st-count muted" title="Models in it and below it">${n.count || ""}</span>` : null}
        ${twin ? html`<span class="st-twin warn-text" title="Another subcategory here has this name: the two will be merged">merges</span>` : null}
        <button type="button" class="ghost st-add" title="Add a subcategory inside this one" aria-label=${`Add inside ${n.name || "this one"}`} onClick=${() => add(n.uid)}>${Icon.plus(13)}</button>
        <button type="button" class="ghost st-out" title="Move out a level" aria-label=${`Move ${n.name || "this one"} out a level`} disabled=${!depth} onClick=${() => outdent(n.uid)}>←</button>
        <button type="button" class="ghost st-in" title="Move into the one above" aria-label=${`Move ${n.name || "this one"} into the one above`} disabled=${!i} onClick=${() => indent(n.uid)}>→</button>
        <button type="button" class="ghost st-remove" title=${n.count ? `Remove it: its ${n.count} ${n.count === 1 ? "model moves" : "models move"} up a level` : "Remove it"} aria-label=${`Remove ${n.name || "this one"}`} onClick=${() => remove(n.uid)}>${Icon.close(13)}</button>
      </div>
      ${n.children.length ? html`<ul class="st-list">${n.children.map((c, j) => row(c, j, n.children, depth + 1, p))}</ul>` : null}
    </li>`;
  };
  return html`<div class="st" id=${id}>
    ${nodes.length ? html`<ul class="st-list st-top">${nodes.map((n, i) => row(n, i, nodes, 0, []))}</ul>` : html`<p class="muted st-empty">No subcategories yet: models go straight into the top folder.</p>`}
    <div><button type="button" class="ghost" id=${`${id}-add`} onClick=${() => add(null)}>${Icon.plus(13)} Add a subcategory</button></div>
  </div>`;
}
