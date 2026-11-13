// The browse view: one place of the library (all models, Unsorted, Starred,
// a collection, or a schema and some of its categories), searched in the core, shown as a
// grid or a list, with the selected model's details beside it. Selecting works
// as everywhere (docs/PLAN.md, "UI pass design"): click, Ctrl or Cmd-click,
// Shift-click, tick boxes, Ctrl+A, Esc and the arrow keys; right-click gives the
// same actions as the details panel.
import { html, useState, useEffect, useLayoutEffect, useRef } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, setPref } from "./state.js";
import { routeHash, schemaScope } from "./context.js";
import { Icon } from "./icons.js";
import { api, toast } from "./library.js";
import { ModelDetails, SelectedPanel, PlaceSummary, Cover, size } from "./details.js";
import { MODEL_ACTIONS, menuItems, openMenu, usePageKeys, runKey, letter, selectAllKey, editDetails, writable } from "./actions.js";
import { categoryItems } from "./categories.js";
import { CommandBar, ViewSwitch, SortMenu, SearchNote } from "./layout.js";
import { Inspector } from "./inspector.js";
import { fileSel, clear as clearFiles } from "./filesel.js";
import { useFileSelection } from "./extract.js";
import { UnsortedPreview } from "./unsorted-preview.js";
import { CatIcon, iconOf } from "./caticons.js";
import { valueMenu } from "./values.js";
import { pressModel, moveInCollection } from "./drag.js";
import { collectionOf, collectionItems, savedSearchOf, savedSearchItems, scopeLabel, placeGone } from "./collections.js";

/** Open a model's own page. */
const openModel = (m) => { location.hash = routeHash(`model:${m.id}`); };

const PAGE = 300;
const SORTS = [["name", "Name"], ["added", "Recently added"], ["size", "Largest first"], ["author", "Author"], ["released", "Released"], ["changed", "Recently changed"], ["files", "Most files"]];
/** A collection's page has its own order first (docs/PLAN.md, "C: collections in your own order"). */
const COLL_SORTS = [["order", "Your order"], ...SORTS];
/** The date a list row shows, following the sort. */
const DATE_COL = { released: ["released", "Released"], changed: ["changed", "Changed"] };
const VIEWS = [["grid", "Grid", "grid"], ["list", "List", "list"]];
const BROWSE = { page: "browse" };

/** The place's title for the command bar: its name large (with its icon), the
 *  categories above it as small links. */
function titleOf(scope, overview) {
  if (scope === "all") return { title: "All models" };
  if (scope === "unsorted") return { title: "Unsorted" };
  if (scope === "favs") return { title: "Starred" };
  const c = collectionOf(scope, overview);
  if (c) return { title: html`<h1 class="crumbs" title=${c.name || "Collection"}>${Icon.bookmark(20)}<span class="crumbs-name">${c.name || "Collection"}</span></h1>` };
  const sv = savedSearchOf(scope, overview);
  if (sv) return { title: html`<h1 class="crumbs" title=${sv.name ? `Saved search: ${sv.q}` : "Saved search"}>${Icon.search(20)}<span class="crumbs-name">${sv.name || "Saved search"}</span></h1>` };
  const [id, ...values] = scope.slice(7).split("/").map(decodeURIComponent);
  const schema = overview?.schemas?.find((s) => s.id === id);
  const crumbs = [[schema?.name || id, []], ...values.map((v, i) => [v, values.slice(0, i + 1)])];
  const [label] = crumbs[crumbs.length - 1];
  return {
    up: crumbs.slice(0, -1).map(([name, path]) => [name, routeHash(`browse:${schemaScope(id, path)}`)]),
    title: html`<h1 class="crumbs" title=${crumbs.map(([n]) => n).join(" › ")}><${CatIcon} icon=${iconOf(schema, values)} size=${20} fallback="layers" /><span class="crumbs-name">${label}</span></h1>`,
  };
}

/** The place's name in words (the last of its crumbs). */
function placeName(scope, overview) {
  if (scope === "all") return "All models";
  if (scope === "unsorted") return "Unsorted";
  if (scope === "favs") return "Starred";
  const c = collectionOf(scope, overview);
  if (c) return c.name || "this collection";
  const sv = savedSearchOf(scope, overview);
  if (sv) return sv.name || "this saved search";
  const [id, ...values] = scope.slice(7).split("/").map(decodeURIComponent);
  return values.length ? values[values.length - 1] : overview?.schemas?.find((s) => s.id === id)?.name || id;
}

/** The category shown: one menu, the same at every level and on the sidebar
 *  (a collection's: Rename and Delete, as on its menu row). */
function categoryMenu(scope, overview) {
  const c = collectionOf(scope, overview);
  if (c?.name) return { id: "collection-menu", title: "Collection: rename or delete", items: () => collectionItems(c) };
  const sv = savedSearchOf(scope, overview);
  if (sv?.name) return { id: "saved-search-menu", title: "Saved search: edit or delete", items: () => savedSearchItems(sv) };
  if (!scope.startsWith("schema:")) return null;
  const [id, ...values] = scope.slice(7).split("/").map(decodeURIComponent);
  return { id: "category-menu", title: `${values.length ? "Subcategory" : "Category"}: add, edit, rename or remove`, items: () => categoryItems(id, values) };
}

/** The category a scope shows, with its schema and its tree node (children, count). */
function placeOf(scope, overview) {
  if (!scope.startsWith("schema:")) return null;
  const [id, ...path] = scope.slice(7).split("/").map(decodeURIComponent);
  const schema = overview?.schemas?.find((s) => s.id === id);
  if (!schema) return null;
  let node = { children: schema.tree || [], count: schema.count };
  for (const v of path) {
    node = node.children.find((n) => n.value.toLowerCase() === v.toLowerCase());
    if (!node) return null;
  }
  return { schema, path, node };
}

const SHOW = [["subs", "Subcategories", "layers"], ["models", "Models", "box"]];

/** A subcategory as a tile: its icon, name, how many models it holds, and four of their covers (R3). */
function SubTile({ schema, path, node, rev }) {
  const [models, setModels] = useState(null);
  const scope = schemaScope(schema.id, path);
  useEffect(() => {
    let live = true;
    if (!node.count) { setModels([]); return; }
    api("models_query", { scope, q: "", sort: "added", limit: 4 }).then((r) => { if (live) setModels(r.items || []); }, () => { if (live) setModels([]); });
    return () => { live = false; };
  }, [scope, rev, node.count]);
  const icon = iconOf(schema, path);
  const subs = node.children.length;
  return html`<a class="subcat-tile" href=${routeHash(`browse:${scope}`)} data-sub=${node.value} title=${node.value}
    onContextMenu=${(e) => openMenu(e, categoryItems(schema.id, path))}
    onKeyDown=${(e) => { if ((e.shiftKey && e.key === "F10") || e.key === "ContextMenu") { e.preventDefault(); e.stopPropagation(); openMenu(e.currentTarget, categoryItems(schema.id, path)); } }}>
    <span class=${`subcat-pics subcat-pics-${Math.max(1, models?.length || 0)}`}>${models?.length
      ? models.map((m) => html`<${Cover} key=${m.id} model=${m} cls="subcat-pic" single=${true} />`)
      : html`<span class="subcat-empty">${icon ? html`<${CatIcon} icon=${icon} size=${34} />` : Icon.folder(34)}</span>`}</span>
    <span class="subcat-name">${icon ? html`<${CatIcon} icon=${icon} size=${15} />` : null}<span>${node.value}</span></span>
    <span class="subcat-meta">${node.count} ${node.count === 1 ? "model" : "models"}${subs ? ` · ${subs} ${subs === 1 ? "subcategory" : "subcategories"}` : ""}</span>
  </a>`;
}

const cardEl = (id) => document.querySelector(`.results [data-model="${CSS.escape(id)}"]`);

/** Click: select one. Ctrl or Cmd-click (or its tick box): add or remove it.
 *  Shift-click: the run from the one clicked before. */
function choose(e, m, items) {
  const s = ui.get();
  const ids = items.map((x) => x.id);
  const cur = s.picked.filter((id) => ids.includes(id));
  if (e.ctrlKey || e.metaKey) {
    ui.set({ picked: cur.includes(m.id) ? cur.filter((id) => id !== m.id) : [...cur, m.id], selection: m.id, anchor: m.id });
  } else if (e.shiftKey && s.anchor && ids.includes(s.anchor)) {
    const [a, b] = [ids.indexOf(s.anchor), ids.indexOf(m.id)].sort((x, y) => x - y);
    ui.set({ picked: ids.slice(a, b + 1), selection: m.id });
  } else {
    ui.set({ picked: [m.id], selection: m.id, anchor: m.id });
  }
}

function Tick({ m, on, items }) {
  return html`<input type="checkbox" class="pick-tick" tabindex="-1" checked=${on} aria-label=${`Select ${m.name}`}
    onClick=${(e) => { e.stopPropagation(); choose({ ctrlKey: true }, m, items); }} onDblClick=${(e) => e.stopPropagation()} />`;
}

/** Where a model is filed, category first ("Home and garden › Kitchen"). */
function fullPlace(m, overview) {
  if (!m.schema) return "Unsorted";
  return [overview?.schemas?.find((x) => x.id === m.schema)?.name, ...(m.path || [])].filter(Boolean).join(" › ");
}

/** Where a card's model is filed, leaving out what the page already says: inside
 *  a category, the category and the subcategories shown (`place`); none on
 *  Unsorted itself. Pages across categories (All models, Starred, a collection)
 *  give the whole place. */
function placeLine(m, place, unsorted, overview) {
  if (!m.schema) return unsorted ? null : "Unsorted";
  if (place && m.schema === place.schema.id) {
    const rest = (m.path || []).slice(place.path.length);
    return rest.length ? rest.join(" › ") : null;
  }
  return fullPlace(m, overview) || null;
}

function Card({ m, selected, fav, items, onMenu, where, full, onPress }) {
  return html`<div class="card" role="option" tabindex="0" data-model=${m.id} aria-selected=${selected ? "true" : "false"}
    onPointerDown=${(e) => onPress?.(e, m)} onClick=${(e) => choose(e, m, items)} onDblClick=${() => openModel(m)} onContextMenu=${(e) => onMenu(e, m)}>
    <${Cover} model=${m} />
    <${Tick} m=${m} on=${selected} items=${items} />
    ${fav ? html`<span class="card-fav" title="Starred">${Icon.star(14, true)}</span>` : null}
    <span class="card-name" title=${m.name}>${m.name}</span>
    <span class="card-meta">
      ${m.authors?.length ? html`<span class="card-author" title=${m.authors.join(", ")}>${m.authors.join(", ")}</span>` : null}
      ${where ? html`<span class="card-location" title=${full || where}>${where}</span>` : null}
    </span>
  </div>`;
}

function Row({ m, selected, fav, items, onMenu, where, full, sort, onPress }) {
  const [dateKey] = DATE_COL[sort] || ["added"];
  return html`<div class="row" role="option" tabindex="0" data-model=${m.id} aria-selected=${selected ? "true" : "false"}
    onPointerDown=${(e) => onPress?.(e, m)} onClick=${(e) => choose(e, m, items)} onDblClick=${() => openModel(m)} onContextMenu=${(e) => onMenu(e, m)}>
    <${Tick} m=${m} on=${selected} items=${items} />
    <${Cover} model=${m} cls="thumb-sm" />
    <span class="row-name">${m.name}${fav ? html` <span class="badge-star" title="Starred">${Icon.star(12, true)}</span>` : null}</span>
    <span class="row-author">${m.authors.join(", ")}</span>
    <span class="row-cat" title=${full || null}>${where || ""}</span>
    <span class="row-added">${String(m[dateKey] || "").slice(0, 10)}</span>
    <span class="row-size">${sort === "files" ? `${m.files?.count || 0} ${m.files?.count === 1 ? "file" : "files"}` : size(m.files?.bytes || 0)}</span>
  </div>`;
}

/** The filter chips, kept to one row: those that don't fit go in a "+N more" menu. */
function FilterChips({ chips, isOn, toggle }) {
  const box = useRef(null);
  const [fit, setFit] = useState(chips.length);
  const [width, setWidth] = useState(0);
  const sig = chips.map(([k, f]) => k + f.value).join("|");
  useLayoutEffect(() => {
    const ro = new ResizeObserver(([e]) => setWidth(Math.round(e.contentRect.width)));
    ro.observe(box.current);
    return () => ro.disconnect();
  }, []);
  // lay them all out on one line, then keep those that fit beside the More button
  useLayoutEffect(() => setFit(chips.length), [sig, width]);
  useLayoutEffect(() => {
    if (fit !== chips.length || !box.current) return;
    const el = box.current;
    const kids = [...el.querySelectorAll(":scope > .chip-btn")];
    const right = el.getBoundingClientRect().right - parseFloat(getComputedStyle(el).paddingRight || 0);
    if (!kids.length || kids[kids.length - 1].getBoundingClientRect().right <= right + 0.5) return;
    const more = 104; // room for "+NN more ▾"
    const n = kids.findIndex((c) => c.getBoundingClientRect().right > right - more);
    setFit(Math.max(1, n < 0 ? kids.length - 1 : n));
  }, [fit, sig, width]);
  const chip = ([k, f]) => {
    const on = isOn(k, f.value);
    return html`<button type="button" class=${`chip-btn${on ? " on" : ""}`} key=${k + f.value} aria-pressed=${on ? "true" : "false"} data-filter=${filterOf(k, f.value)}
      onClick=${() => toggle(k, f.value)} title=${on ? "Show them all again" : `Only ${k === "author" ? "by" : "tagged"} ${f.value}`}
      onContextMenu=${(e) => valueMenu(e, k === "tag" ? "tags" : "authors", f.value, { label: on ? "Show them all again" : `Only ${k === "author" ? "by" : "tagged #"}${k === "author" ? " " : ""}${f.value}`, run: () => toggle(k, f.value) })}>${k === "tag" ? "#" : ""}${f.value} ${on ? html`<span aria-hidden="true">×</span>` : html`<span class="muted">${f.count}</span>`}</button>`;
  };
  const rest = chips.slice(fit);
  return html`<div class="filters" aria-label="Narrow down" ref=${box}>${chips.slice(0, fit).map(chip)}
    ${rest.length ? html`<button type="button" class="chip-btn chip-more" id="filters-more" aria-haspopup="menu" title="More authors and tags to narrow down by"
      onClick=${(e) => openMenu(e.currentTarget, rest.map(([k, f]) => ({ id: `filter-${k}-${f.value}`, label: `${k === "tag" ? "#" : "by "}${f.value}${f.count != null ? ` (${f.count})` : ""}`,
        icon: isOn(k, f.value) ? "check" : null, run: () => toggle(k, f.value) })))}>+${rest.length} more ▾</button>` : null}</div>`;
}

/** The list's column headers: Name, Authors, the date and Size sort when clicked, as
 *  on Import. The date column is the one the sort uses (Added, Released, Changed);
 *  sorting by Most files makes the size column the count. */
function ListHead({ sort, setSort }) {
  const col = (k, label, cls) => html`<button type="button" class=${`linkish ${cls}`} data-sort=${k} aria-pressed=${sort === k ? "true" : "false"}
    title=${`Sort by ${label.toLowerCase()}`} onClick=${() => setSort(k)}>${label}${sort === k ? " ▾" : ""}</button>`;
  const [dateSort, dateLabel] = DATE_COL[sort] ? [sort, DATE_COL[sort][1]] : ["added", "Added"];
  return html`<div class="list-head" id="list-head" aria-hidden="true">
    <span class="row-tick-gap"></span><span class="thumb-gap"></span>
    ${col("name", "Name", "row-name")}${col("author", "Authors", "row-author")}<span class="row-cat">Category</span>
    ${col(dateSort, dateLabel, "row-added")}${sort === "files" ? col("files", "Files", "row-size") : col("size", "Size", "row-size")}
  </div>`;
}

/** Search filters in the search text (author:jo, tag:"big one"). */
const filterOf = (key, value) => `${key}:${/\s/.test(value) ? `"${value}"` : value}`;

/** The typed filters, each with an example from this library, for the ? at the end
 *  of the search box. `add(key, value)` puts one in the search. */
function filterItems(scope, overview, facets, add) {
  const place = placeOf(scope, overview);
  const schemas = overview?.schemas || [];
  const schema = place?.schema || (schemas.length === 1 ? schemas[0] : null);
  const sub = (place ? place.node.children : null)?.[0]?.value || schemas.find((x) => x.tree?.length)?.tree[0]?.value;
  const item = (key, example, what) => ({ id: `filter-${key}`, label: filterOf(key, example), sub: what, run: () => add(key, example) });
  const items = [
    { head: "Add a filter to the search" },
    item("author", facets.authors?.[0]?.value || "jo", "Made by this author"),
    item("tag", facets.tags?.[0]?.value || "presupported", "Has this tag"),
  ];
  const filed = scope !== "unsorted";
  if (filed && !place && schemas.length) items.push(item("category", schemas[0].name, "Filed in this category"));
  if (filed && sub) items.push(item("in", sub, "Filed in a subcategory with this name"));
  items.push(item("kind", "slicer", "Has files of this kind (model, slicer, image, doc, video, archive) or type (3mf, stl, zip…)"));
  items.push(item("missing", "author", "Has no author; also tags, source, licence, released or picture"));
  if (overview?.collections?.length) items.push(item("collection", overview.collections[0].name, "In this collection"));
  for (const f of schema?.fields || []) {
    const example = String(f.choices || "").split(",").map((x) => x.trim()).find(Boolean) || (f.type === "number" ? "1" : f.label.toLowerCase());
    items.push(item(f.key, example, `${schema.name}'s ${f.label}`));
  }
  return items;
}
function filtersIn(q) {
  const out = [];
  for (const m of q.matchAll(/(?:^|\s)(author|tag):("[^"]*"|\S+)/g)) out.push([m[1], m[2].replace(/^"|"$/g, "")]);
  return out;
}

export function Browser() {
  const s = useStore(ui, (st) => ({ route: st.route, q: st.q, sort: st.sort, collSort: st.collSort, layout: st.layout, selection: st.selection, picked: st.picked, rev: st.catalogRev, favs: st.favs, overview: st.overview, library: st.library, navOpen: st.navOpen }));
  const scope = s.route.slice(7);
  const coll = collectionOf(scope, s.overview);
  // a collection's page keeps its own sort, Your order to start with
  const inColl = scope.startsWith("coll:");
  const sort = inColl ? s.collSort || "order" : s.sort === "order" ? "name" : s.sort;
  const setSort = (v) => setPref(inColl ? { collSort: v } : { sort: v });
  const CTX = coll?.name ? { ...BROWSE, collection: coll } : BROWSE;
  const catShow = useStore(ui, (st) => st.catShow);
  const place = placeOf(scope, s.overview);
  const subs = place?.node.children || [];
  const show = subs.length ? (catShow[scope] || "subs") : "models";
  // Subcategories: their tiles, then only the models filed at this level (a search looks everywhere below)
  const direct = show === "subs" && !s.q;
  const [text, setText] = useState(s.q);
  const [result, setResult] = useState(null);
  const [limit, setLimit] = useState(PAGE);
  const seq = useRef(0);
  useLayoutEffect(() => { setText(s.q); }, [s.q]); // before paint, so nothing typed meanwhile is put back
  useEffect(() => { setLimit(PAGE); }, [scope, s.q, sort]);
  useEffect(() => {
    if (!s.library) return;
    const n = ++seq.current;
    const q = s.q;
    api("models_query", { scope, q, sort, limit, direct }).then((r) => { if (n === seq.current) setResult({ ...r, q, scope, direct }); },
      (e) => { if (n === seq.current) { setResult({ total: 0, items: [], facets: {}, q, scope, direct, error: e.message || String(e) }); } });
  }, [scope, s.q, sort, limit, s.rev, s.favs.length, s.library?.path, direct]);
  // what the summary beside the tiles describes: everything below, not only the models filed here
  const [whole, setWhole] = useState(null);
  useEffect(() => {
    if (!s.library || !direct) return;
    let live = true;
    api("models_query", { scope, q: "", sort, limit: 1 }).then((r) => { if (live) setWhole({ ...r, scope }); }, () => {});
    return () => { live = false; };
  }, [scope, direct, s.rev, s.library?.path]);
  // typing searches after a short pause
  useEffect(() => {
    if (text === s.q) return;
    const t = setTimeout(() => ui.set({ q: text }), 150);
    return () => clearTimeout(t);
  }, [text]);
  const items = result?.items || [];
  // only what's shown here can be selected (a model selected elsewhere isn't)
  const picked = items.filter((m) => s.picked.includes(m.id));
  const fileSrc = useStore(fileSel, (x) => x.src);
  const filesPicking = useFileSelection(scope === "unsorted" && picked.length === 1 && fileSrc?.id === picked[0].id ? fileSrc : null);
  const toggleFilter = (key, value) => {
    const f = filterOf(key, value);
    const on = filtersIn(s.q).some(([k, v]) => k === key && v === value);
    ui.set({ q: on ? s.q.replace(f, " ").replace(/\s+/g, " ").trim() : `${s.q} ${f}`.trim() });
  };
  // a filter from the ? menu: added to the search, its value selected so typing replaces it
  const addFilter = (key, value) => {
    const f = filterOf(key, value);
    const q = `${text.trim()} ${f}`.trim();
    setText(q);
    ui.set({ q });
    requestAnimationFrame(() => {
      const box = document.getElementById("search");
      if (!box) return;
      box.focus();
      const end = box.value.length;
      box.setSelectionRange(end - f.length + key.length + 1 + (f.endsWith('"') ? 1 : 0), end - (f.endsWith('"') ? 1 : 0));
    });
  };
  const onMenu = (e, m) => {
    let sel = picked;
    if (!picked.some((x) => x.id === m.id)) { ui.set({ picked: [m.id], selection: m.id, anchor: m.id }); sel = [m]; }
    openMenu(e, menuItems(MODEL_ACTIONS, sel, CTX));
  };
  // drag cards onto a list in the menu, or (Your order) among the others
  const reorderColl = inColl && sort === "order" && coll?.name ? coll : null;
  const onPress = (e, m) => pressModel(e, s.picked.includes(m.id) ? items.filter((x) => s.picked.includes(x.id)) : [m], reorderColl);
  usePageKeys((e) => {
    const st = ui.get();
    const ids = items.map((m) => m.id);
    const sel = items.filter((m) => st.picked.includes(m.id));
    // Alt+↑ and Alt+↓: the selected models one place earlier or later (Your order)
    if (e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown") && !e.ctrlKey && !e.metaKey) {
      if (!reorderColl || !sel.length || st.library?.read_only) return;
      e.preventDefault();
      const on = (id) => st.picked.includes(id);
      let before;
      if (e.key === "ArrowUp") {
        const prev = ids.slice(0, ids.findIndex(on)).filter((id) => !on(id)).pop();
        if (!prev) return;
        before = prev;
      } else {
        const last = ids.length - 1 - [...ids].reverse().findIndex(on);
        const next = ids.slice(last + 1).find((id) => !on(id));
        if (!next) return;
        before = ids[ids.indexOf(next) + 1] || null;
      }
      moveInCollection(reorderColl, sel.map((m) => m.id), before);
      return;
    }
    const l = letter(e);
    if (e.key === "Enter") { if (sel.length === 1) { e.preventDefault(); openModel(sel[0]); } return; }
    if (e.key === "F2") { if (sel.length === 1) { e.preventDefault(); const ok = writable(); if (ok === true) editDetails(sel, true); else toast(ok); } return; }
    if (l === "e" || l === "m" || l === "s" || l === "c") { if (sel.length) { e.preventDefault(); runKey(MODEL_ACTIONS, l.toUpperCase(), sel, CTX); } return; }
    if (selectAllKey(e)) { e.preventDefault(); ui.set({ picked: ids, anchor: ids[0] || null, selection: ids[ids.length - 1] || null }); return; }
    if (e.key === "Escape") { if (fileSel.get().picked.length && scope === "unsorted") clearFiles(); else if (sel.length) ui.set({ picked: [] }); return; }
    if (e.key === "ContextMenu") {
      const at = (ids.includes(st.selection) && cardEl(st.selection)) || (sel[0] && cardEl(sel[0].id));
      if (at && sel.length) openMenu(at, menuItems(MODEL_ACTIONS, sel, CTX));
      return;
    }
    if (!e.key.startsWith("Arrow") || !ids.length || e.ctrlKey || e.metaKey || e.altKey) return;
    let cols = 1;
    if (st.layout !== "list") {
      const els = [...document.querySelectorAll(".results > [data-model]")];
      cols = Math.max(1, els.filter((x) => x.offsetTop === els[0]?.offsetTop).length);
    }
    const d = { ArrowRight: st.layout === "list" ? 0 : 1, ArrowLeft: st.layout === "list" ? 0 : -1, ArrowDown: cols, ArrowUp: -cols }[e.key];
    if (!d) return;
    e.preventDefault();
    const cur = ids.indexOf(st.selection);
    const next = cur < 0 ? 0 : Math.max(0, Math.min(ids.length - 1, cur + d));
    const id = ids[next];
    if (e.shiftKey) {
      const anchor = ids.includes(st.anchor) ? st.anchor : ids[Math.max(cur, 0)];
      const [a, b] = [ids.indexOf(anchor), next].sort((x, y) => x - y);
      ui.set({ picked: ids.slice(a, b + 1), selection: id, anchor });
    } else {
      ui.set({ picked: [id], selection: id, anchor: id });
    }
    cardEl(id)?.focus();
  });
  const facets = result?.facets || {};
  const active = filtersIn(s.q);
  const isOn = (k, v) => active.some(([a, b]) => a === k && b === v);
  // the filters on first, then the authors and tags most used here
  const chips = [
    ...active.map(([k, v]) => [k, { value: v, count: null }]),
    ...[...(facets.authors || []).slice(0, 12).filter((f) => !isOn("author", f.value)).map((f) => ["author", f]),
      ...(facets.tags || []).slice(0, 16).filter((f) => !isOn("tag", f.value)).map((f) => ["tag", f])].sort((a, b) => b[1].count - a[1].count),
  ];
  const View = s.layout === "list" ? Row : Card;
  const name = placeName(scope, s.overview);
  const unsortedOne = scope === "unsorted" && picked.length === 1;
  // files picked in the Unsorted preview take the bar over from the model
  // the answer for this place and search (the last one stays up while the next comes)
  const fresh = result && result.q === s.q && result.scope === scope && result.direct === direct ? result : null;
  const total = direct ? place.node.count : result?.total;
  const count = html`<span class="page-count" id="browse-count" data-q=${result ? result.q : null} data-scope=${result ? result.scope : null}>${result ? `${total} ${total === 1 ? "model" : "models"}` : ""}</span>`;
  const sv = savedSearchOf(scope, s.overview);
  const svGone = sv && !sv.gone && placeGone(sv.scope, s.overview);
  const emptyNote = sv?.gone ? "That saved search isn't in the library any more."
    : svGone ? `It searches in ${scopeLabel(sv.scope, s.overview)}. Choose Edit saved search… in ⋯ to search all models instead.`
    : s.q ? `Nothing in ${name} matches that search.` : scope === "favs" ? "Nothing starred yet: star a model to keep it here."
    : sv ? "Nothing matches this saved search at the moment."
    : coll?.gone ? "That collection isn't in the library any more."
    : coll ? "Nothing in this collection yet. Select models anywhere and press C, or right-click them and choose Add to collection…"
    : scope === "unsorted" ? "No models here yet."
    : html`No models here yet. Drop model files or folders on this window to add them here, or use <a href=${routeHash("import")}>Import</a>.`;
  // Save… keeps the search under Collections (on a saved search's page, both together)
  const save = s.q && s.library && !s.library.read_only && !sv?.gone ? html`<button type="button" class="linkish" id="search-save" title="Keep this search under Collections in the menu"
    onClick=${() => ui.set({ dialog: { type: "saved-search", q: sv ? `${sv.q} ${s.q}` : s.q, scope: sv ? sv.scope : scope } })}>Save…</button>` : null;
  const wider = html`${s.q && scope !== "all" ? html`<a href=${routeHash("browse:all")} id="search-everywhere">Search all models</a>` : null}${save}`;
  const head = titleOf(scope, s.overview);
  const details = picked.length > 1 ? html`<${SelectedPanel} models=${picked} />`
    : html`<${ModelDetails} id=${picked[0]?.id || null} row=${false} empty=${html`<${PlaceSummary} name=${name} result=${direct ? (fresh && whole?.scope === scope ? whole : null) : fresh} here=${direct && fresh ? fresh.total : null}
        about=${sv?.name ? html`Finds <code class="saved-q">${sv.q}</code> in ${scopeLabel(sv.scope, s.overview)}.` : null}
        onTag=${(v) => toggleFilter("tag", v)} tagsOn=${active.filter(([k]) => k === "tag").map(([, v]) => v)}
        onAuthor=${(v) => toggleFilter("author", v)} authorsOn=${active.filter(([k]) => k === "author").map(([, v]) => v)} />`} />`;
  const selection = filesPicking || (picked.length ? { count: `${picked.length} ${picked.length === 1 ? "model" : "models"} selected`, onClear: () => ui.set({ picked: [] }),
    items: menuItems(MODEL_ACTIONS, picked, CTX), buttons: 3 } : null);
  return html`<${CommandBar} id="browse-head" up=${head.up} title=${head.title} count=${count} more=${categoryMenu(scope, s.overview)} selection=${selection}
      search=${html`<label class="search small toolbar-search with-help"><span class="visually-hidden">Search this place</span>
          <input id="search" data-search type="search" value=${text} placeholder="Search names, authors, tags…" autocomplete="off" spellcheck="false"
            title="Search (/). ? lists the filters, such as author:jo" onInput=${(e) => setText(e.target.value)} />
          <button type="button" class="ghost search-help" id="search-help" title="Search filters" aria-label="Search filters" aria-haspopup="menu"
            onClick=${(e) => openMenu(e.currentTarget, filterItems(scope, s.overview, (direct && whole?.scope === scope ? whole.facets : null) || facets, addFilter))}>?</button></label>`}
      tools=${html`${subs.length ? html`<${ViewSwitch} id="cat-show" label="Show" views=${SHOW} value=${s.q ? "models" : show} onChange=${(v) => setPref({ catShow: { ...catShow, [scope]: v } })} />` : null}
        <${ViewSwitch} id="layout-switch" views=${VIEWS} value=${s.layout === "list" ? "list" : "grid"} onChange=${(v) => setPref({ layout: v })} />
        <${SortMenu} id="sort" options=${inColl ? COLL_SORTS : SORTS} value=${sort} onChange=${setSort} />`} />
  <div class="browser">
    <div class="browse-main">
      ${chips.length ? html`<${FilterChips} chips=${chips} isOn=${isOn} toggle=${toggleFilter} />` : null}
      <div class="browse-scroll">
        <${SearchNote} q=${s.q} found=${fresh ? `${fresh.total} found in ${name}${subs.length ? " and its subcategories" : ""}` : ""} wider=${wider} onClear=${() => { setText(""); ui.set({ q: "" }); }} />
        ${result?.error ? html`<p class="warn-note" role="alert">${result.error}</p>` : null}
        ${scope === "unsorted" && items.length && !s.library?.read_only ? html`<p class="place-note" id="unsorted-note">These have no category yet: select them and choose Move to category… (M), or sort them on <a href=${routeHash("import")}>Import</a>.</p>` : null}
        ${direct ? html`<div class="subcat-tiles" id="subcat-tiles" aria-label="Subcategories">${subs.map((n) => html`<${SubTile} key=${n.value} schema=${place.schema} path=${[...place.path, n.value]} node=${n} rev=${s.rev} />`)}</div>
          ${items.length ? html`<h2 class="here-head" id="here-head">Here in ${name}</h2>` : null}
          ${result && !place.node.count ? html`<div class="empty" id="no-results">${emptyNote}</div>` : null}` : null}
        ${result && !items.length && !result.error && !direct ? html`<div class="empty" id="no-results">${emptyNote}</div>` : null}
        ${s.layout === "list" && items.length ? html`<${ListHead} sort=${sort} setSort=${setSort} />` : null}
        <div class=${`results ${s.layout === "list" ? "list-view" : "grid-view"}${picked.length > 1 ? " picking" : ""}`} role="listbox" aria-multiselectable="true" aria-label="Models">
          ${items.map((m) => html`<${View} key=${m.id} m=${m} items=${items} selected=${s.picked.includes(m.id)} fav=${s.favs.includes(m.id)} onMenu=${onMenu}
            where=${placeLine(m, place, scope === "unsorted", s.overview)} full=${fullPlace(m, s.overview)} sort=${sort} onPress=${onPress} />`)}
        </div>
        ${result && result.total > items.length ? html`<button type="button" class="ghost more-btn" onClick=${() => setLimit(limit + PAGE)}>Show more (${result.total - items.length} left)</button>` : null}
      </div>
    </div>
    <${Inspector} page=${scope === "unsorted" ? "unsorted" : "browse"} open=${!matchMedia("(max-width: 1250px)").matches} want=${unsortedOne ? "files" : null} tabs=${[
      { id: "files", label: "Files", icon: "folder", body: unsortedOne ? html`<${UnsortedPreview} key=${picked[0].id} id=${picked[0].id} />` : false },
      { id: "details", label: "Details", icon: "info", body: details },
    ]} />
  </div>`;
}

export { toast };
