// The browse view: one place of the library (all models, Unsorted, Starred,
// or a schema and some of its categories), searched in the core, shown as a
// grid or a list, with the selected model's details beside it. Selecting works
// as everywhere (docs/PLAN.md, "UI pass design"): click, Ctrl or Cmd-click,
// Shift-click, tick boxes, Ctrl+A, Esc and the arrow keys; right-click gives the
// same actions as the details panel.
import { html, useState, useEffect, useRef } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, setPref } from "./state.js";
import { routeHash, schemaScope } from "./context.js";
import { Icon } from "./icons.js";
import { api, libraryUrl, toast } from "./library.js";
import { ModelDetails } from "./details.js";
import { ActionRow, MODEL_ACTIONS, menuItems, openMenu, usePageKeys, runKey, letter, selectAllKey, editDetails, writable } from "./actions.js";
import { categoryItems } from "./categories.js";

/** Open a model's own page. */
const openModel = (m) => { location.hash = routeHash(`model:${m.id}`); };

const PAGE = 300;
const SORTS = [["name", "Name"], ["added", "Recently added"], ["size", "Largest first"]];
const KIND_ICON = { model: "box", slicer: "stack", image: "image", doc: "file", video: "image", archive: "folder", other: "file" };
const CTX = { page: "browse" };

/** The place's title, as links back up its categories. */
function Title({ scope, overview }) {
  if (scope === "all") return html`<h1>All models</h1>`;
  if (scope === "unsorted") return html`<h1>Unsorted</h1>`;
  if (scope === "favs") return html`<h1>Starred</h1>`;
  const [id, ...values] = scope.slice(7).split("/").map(decodeURIComponent);
  const schema = overview?.schemas?.find((s) => s.id === id);
  const crumbs = [[schema?.name || id, []], ...values.map((v, i) => [v, values.slice(0, i + 1)])];
  return html`<h1 class="crumbs">${crumbs.map(([label, path], i) => html`${i ? html`<span class="crumb-sep"> › </span>` : null}${
    i === crumbs.length - 1 ? label : html`<a href=${routeHash(`browse:${schemaScope(id, path)}`)}>${label}</a>`}`)}</h1>`;
}

/** The category shown: one menu, the same at every level and on the sidebar. */
function CategoryButton({ scope }) {
  if (!scope.startsWith("schema:")) return null;
  const [id, ...values] = scope.slice(7).split("/").map(decodeURIComponent);
  return html`<button type="button" class="ghost" id="category-menu" aria-haspopup="menu" title="Add, edit, rename or delete"
    onClick=${(e) => openMenu(e.currentTarget, categoryItems(id, values))}>${Icon.layers(14)} ${values.length ? "Subcategory" : "Category"} ▾</button>`;
}

/** A model's cover picture, or an icon for the kind of files it has. */
export function Cover({ model, cls = "" }) {
  const [failed, setFailed] = useState(false);
  const cover = model.files?.cover;
  if (cover && !failed) {
    return html`<span class=${`thumb ${cls}`}><img src=${libraryUrl(`${model.rel}/${cover}`)} alt="" loading="lazy" onError=${() => setFailed(true)} /></span>`;
  }
  const kinds = Object.keys(model.files?.kinds || {});
  const kind = ["model", "slicer", "archive", "image", "doc", "video"].find((k) => kinds.includes(k)) || "other";
  return html`<span class=${`thumb thumb-none ${cls}`}>${(Icon[KIND_ICON[kind]] || Icon.file)(40)}</span>`;
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

function Card({ m, selected, fav, items, onMenu }) {
  return html`<div class="card" role="option" tabindex="0" data-model=${m.id} aria-selected=${selected ? "true" : "false"}
    onClick=${(e) => choose(e, m, items)} onDblClick=${() => openModel(m)} onContextMenu=${(e) => onMenu(e, m)}>
    <${Cover} model=${m} />
    <${Tick} m=${m} on=${selected} items=${items} />
    ${fav ? html`<span class="card-fav" title="Starred">${Icon.star(14, true)}</span>` : null}
    <span class="card-name">${m.name}</span>
    <span class="card-sub">${m.authors.join(", ") || " "}</span>
    <span class="card-sub">${m.path.join(" › ") || (m.schema ? "" : "Unsorted")}</span>
  </div>`;
}

function Row({ m, selected, fav, items, onMenu }) {
  return html`<div class="row" role="option" tabindex="0" data-model=${m.id} aria-selected=${selected ? "true" : "false"}
    onClick=${(e) => choose(e, m, items)} onDblClick=${() => openModel(m)} onContextMenu=${(e) => onMenu(e, m)}>
    <${Tick} m=${m} on=${selected} items=${items} />
    <${Cover} model=${m} cls="thumb-sm" />
    <span class="row-name">${m.name}${fav ? html` <span class="badge-star" title="Starred">${Icon.star(12, true)}</span>` : null}</span>
    <span class="row-author">${m.authors.join(", ")}</span>
    <span class="row-cat">${m.path.join(" › ") || (m.schema ? "" : "Unsorted")}</span>
  </div>`;
}

/** Several models selected: what can be done to all of them. */
function Selected({ models }) {
  return html`<aside class="inspector" aria-label="Selected models" id="picked-panel">
    <h2 class="insp-title">${models.length} models selected</h2>
    <p class="insp-sub">Ctrl or Cmd-click adds or removes one, Shift-click selects a run, Ctrl+A selects everything shown and Esc clears.</p>
    <${ActionRow} targets=${models} ctx=${CTX} idPrefix="picked" />
    <div><button type="button" class="ghost" id="picked-clear" title="Clear selection (Esc)" onClick=${() => ui.set({ picked: [] })}>Clear selection</button></div>
    <ul class="insp-files">${models.map((m) => html`<li key=${m.id}><span>${m.name}</span><span class="muted">${m.path.join(" › ") || "Unsorted"}</span></li>`)}</ul>
  </aside>`;
}

/** Search filters in the search text (author:jo, tag:"big one"). */
const filterOf = (key, value) => `${key}:${/\s/.test(value) ? `"${value}"` : value}`;
function filtersIn(q) {
  const out = [];
  for (const m of q.matchAll(/(?:^|\s)(author|tag):("[^"]*"|\S+)/g)) out.push([m[1], m[2].replace(/^"|"$/g, "")]);
  return out;
}

export function Browser() {
  const s = useStore(ui, (st) => ({ route: st.route, q: st.q, sort: st.sort, layout: st.layout, selection: st.selection, picked: st.picked, rev: st.catalogRev, favs: st.favs, overview: st.overview, library: st.library }));
  const scope = s.route.slice(7);
  const [text, setText] = useState(s.q);
  const [result, setResult] = useState(null);
  const [limit, setLimit] = useState(PAGE);
  const seq = useRef(0);
  useEffect(() => { setText(s.q); }, [s.q]);
  useEffect(() => { setLimit(PAGE); }, [scope, s.q, s.sort]);
  useEffect(() => {
    if (!s.library) return;
    const n = ++seq.current;
    const q = s.q;
    api("models_query", { scope, q, sort: s.sort, limit }).then((r) => { if (n === seq.current) setResult({ ...r, q }); },
      (e) => { if (n === seq.current) { setResult({ total: 0, items: [], facets: {}, q, error: e.message || String(e) }); } });
  }, [scope, s.q, s.sort, limit, s.rev, s.favs.length, s.library?.path]);
  // typing searches after a short pause
  useEffect(() => {
    if (text === s.q) return;
    const t = setTimeout(() => ui.set({ q: text }), 150);
    return () => clearTimeout(t);
  }, [text]);
  const items = result?.items || [];
  // only what's shown here can be selected (a model selected elsewhere isn't)
  const picked = items.filter((m) => s.picked.includes(m.id));
  const toggleFilter = (key, value) => {
    const f = filterOf(key, value);
    const on = filtersIn(s.q).some(([k, v]) => k === key && v === value);
    ui.set({ q: on ? s.q.replace(f, " ").replace(/\s+/g, " ").trim() : `${s.q} ${f}`.trim() });
  };
  const onMenu = (e, m) => {
    let sel = picked;
    if (!picked.some((x) => x.id === m.id)) { ui.set({ picked: [m.id], selection: m.id, anchor: m.id }); sel = [m]; }
    openMenu(e, menuItems(MODEL_ACTIONS, sel, CTX));
  };
  usePageKeys((e) => {
    const st = ui.get();
    const ids = items.map((m) => m.id);
    const sel = items.filter((m) => st.picked.includes(m.id));
    const l = letter(e);
    if (e.key === "Enter") { if (sel.length === 1) { e.preventDefault(); openModel(sel[0]); } return; }
    if (e.key === "F2") { if (sel.length === 1) { e.preventDefault(); const ok = writable(); if (ok === true) editDetails(sel, true); else toast(ok); } return; }
    if (l === "e" || l === "m" || l === "s") { if (sel.length) { e.preventDefault(); runKey(MODEL_ACTIONS, l.toUpperCase(), sel, CTX); } return; }
    if (selectAllKey(e)) { e.preventDefault(); ui.set({ picked: ids, anchor: ids[0] || null, selection: ids[ids.length - 1] || null }); return; }
    if (e.key === "Escape") { if (sel.length) ui.set({ picked: [] }); return; }
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
  const chips = [
    ...active.map(([k, v]) => [k, { value: v, count: null }]),
    ...(facets.authors || []).slice(0, 6).filter((f) => !isOn("author", f.value)).map((f) => ["author", f]),
    ...(facets.tags || []).slice(0, 8).filter((f) => !isOn("tag", f.value)).map((f) => ["tag", f]),
  ];
  const View = s.layout === "list" ? Row : Card;
  return html`<div class="browser">
    <div class="browse-main">
      <div class="browse-head">
        <div class="browse-title"><${Title} scope=${scope} overview=${s.overview} />
          <span class="browse-count" id="browse-count" data-q=${result ? result.q : null}>${result ? `${result.total} ${result.total === 1 ? "model" : "models"}` : ""}</span>
          <${CategoryButton} scope=${scope} /></div>
        <label class="search small browse-search"><span class="visually-hidden">Search this place</span>
          <input id="search" data-search type="search" value=${text} placeholder="Search names, authors, tags… or author:jo tag:presupported" autocomplete="off" spellcheck="false"
            title="Search (/)" onInput=${(e) => setText(e.target.value)} /></label>
        <div class="browse-tools">
          <label class="tool-select"><span class="visually-hidden">Sort</span>
            <select id="sort" title="Sort" value=${s.sort} onChange=${(e) => setPref({ sort: e.target.value })}>${SORTS.map(([v, l]) => html`<option value=${v} key=${v}>${l}</option>`)}</select></label>
          <div class="seg" role="group" aria-label="View">
            <button type="button" aria-pressed=${s.layout === "grid" ? "true" : "false"} title="Grid" aria-label="Grid" onClick=${() => setPref({ layout: "grid" })}>${Icon.grid(15)}</button>
            <button type="button" aria-pressed=${s.layout === "list" ? "true" : "false"} title="List" aria-label="List" onClick=${() => setPref({ layout: "list" })} data-layout="list">${Icon.list(15)}</button>
          </div>
        </div>
      </div>
      ${chips.length ? html`<div class="filters" aria-label="Narrow down">${chips.map(([k, f]) => {
        const on = isOn(k, f.value);
        return html`<button type="button" class=${`chip-btn${on ? " on" : ""}`} key=${k + f.value} aria-pressed=${on ? "true" : "false"} data-filter=${filterOf(k, f.value)}
          onClick=${() => toggleFilter(k, f.value)} title=${on ? "Show them all again" : `Only ${k === "author" ? "by" : "tagged"} ${f.value}`}>${k === "tag" ? "#" : ""}${f.value} ${on ? html`<span aria-hidden="true">×</span>` : html`<span class="muted">${f.count}</span>`}</button>`;
      })}</div>` : null}
      <div class="browse-scroll">
        ${result?.error ? html`<p class="warn-note" role="alert">${result.error}</p>` : null}
        ${result && !items.length && !result.error ? html`<div class="empty" id="no-results">${s.q ? "Nothing matches that search here." : scope === "favs" ? "Nothing starred yet: star a model to keep it here." : "No models here yet."}</div>` : null}
        <div class=${`results ${s.layout === "list" ? "list-view" : "grid-view"}${picked.length > 1 ? " picking" : ""}`} role="listbox" aria-multiselectable="true" aria-label="Models">
          ${items.map((m) => html`<${View} key=${m.id} m=${m} items=${items} selected=${s.picked.includes(m.id)} fav=${s.favs.includes(m.id)} onMenu=${onMenu} />`)}
        </div>
        ${result && result.total > items.length ? html`<button type="button" class="ghost more-btn" onClick=${() => setLimit(limit + PAGE)}>Show more (${result.total - items.length} left)</button>` : null}
      </div>
    </div>
    ${picked.length > 1 ? html`<${Selected} models=${picked} />` : html`<${ModelDetails} id=${picked[0]?.id || null} />`}
  </div>`;
}

export { toast };
