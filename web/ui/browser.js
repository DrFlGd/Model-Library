// The browse view: one place of the library (all models, Unsorted, Starred,
// or a schema and some of its categories), searched in the core, shown as a
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
import { PageHead, ViewSwitch, SortMenu, SearchNote } from "./layout.js";

/** Open a model's own page. */
const openModel = (m) => { location.hash = routeHash(`model:${m.id}`); };

const PAGE = 300;
const SORTS = [["name", "Name"], ["added", "Recently added"], ["size", "Largest first"]];
const VIEWS = [["grid", "Grid", "grid"], ["list", "List", "list"]];
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

/** The place's name in words (the last of its crumbs). */
function placeName(scope, overview) {
  if (scope === "all") return "All models";
  if (scope === "unsorted") return "Unsorted";
  if (scope === "favs") return "Starred";
  const [id, ...values] = scope.slice(7).split("/").map(decodeURIComponent);
  return values.length ? values[values.length - 1] : overview?.schemas?.find((s) => s.id === id)?.name || id;
}

/** The category shown: one menu, the same at every level and on the sidebar. */
function categoryMenu(scope) {
  if (!scope.startsWith("schema:")) return null;
  const [id, ...values] = scope.slice(7).split("/").map(decodeURIComponent);
  return { id: "category-menu", label: values.length ? "Subcategory" : "Category", icon: "layers", title: "Add, edit, rename or delete", items: () => categoryItems(id, values) };
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
    <span class="row-added">${(m.added || "").slice(0, 10)}</span>
    <span class="row-size">${size(m.files?.bytes || 0)}</span>
  </div>`;
}

/** The list's column headers: Name, Added and Size sort when clicked, as on Import. */
function ListHead({ sort }) {
  const col = (k, label, cls) => html`<button type="button" class=${`linkish ${cls}`} data-sort=${k} aria-pressed=${sort === k ? "true" : "false"}
    title=${`Sort by ${label.toLowerCase()}`} onClick=${() => setPref({ sort: k })}>${label}${sort === k ? " ▾" : ""}</button>`;
  return html`<div class="list-head" id="list-head" aria-hidden="true">
    <span class="row-tick-gap"></span><span class="thumb-gap"></span>
    ${col("name", "Name", "row-name")}<span class="row-author">Authors</span><span class="row-cat">Category</span>
    ${col("added", "Added", "row-added")}${col("size", "Size", "row-size")}
  </div>`;
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
  useLayoutEffect(() => { setText(s.q); }, [s.q]); // before paint, so nothing typed meanwhile is put back
  useEffect(() => { setLimit(PAGE); }, [scope, s.q, s.sort]);
  useEffect(() => {
    if (!s.library) return;
    const n = ++seq.current;
    const q = s.q;
    api("models_query", { scope, q, sort: s.sort, limit }).then((r) => { if (n === seq.current) setResult({ ...r, q, scope }); },
      (e) => { if (n === seq.current) { setResult({ total: 0, items: [], facets: {}, q, scope, error: e.message || String(e) }); } });
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
  const name = placeName(scope, s.overview);
  // the answer for this place and search (the last one stays up while the next comes)
  const fresh = result && result.q === s.q && result.scope === scope ? result : null;
  const count = html`<span class="page-count" id="browse-count" data-q=${result ? result.q : null} data-scope=${result ? result.scope : null}>${result ? `${result.total} ${result.total === 1 ? "model" : "models"}` : ""}</span>`;
  const wider = s.q && scope !== "all" ? html`<a href=${routeHash("browse:all")} id="search-everywhere">Search all models</a>` : null;
  return html`<div class="browser">
    <div class="browse-main">
      <${PageHead} title=${html`<${Title} scope=${scope} overview=${s.overview} />`} count=${count} menu=${categoryMenu(scope)} />
      <div class="toolbar">
        <label class="search small toolbar-search"><span class="visually-hidden">Search this place</span>
          <input id="search" data-search type="search" value=${text} placeholder="Search names, authors, tags… or author:jo tag:presupported" autocomplete="off" spellcheck="false"
            title="Search (/)" onInput=${(e) => setText(e.target.value)} /></label>
        <${SortMenu} id="sort" options=${SORTS} value=${s.sort} onChange=${(v) => setPref({ sort: v })} />
        <${ViewSwitch} views=${VIEWS} value=${s.layout === "list" ? "list" : "grid"} onChange=${(v) => setPref({ layout: v })} />
      </div>
      ${chips.length ? html`<div class="filters" aria-label="Narrow down">${chips.map(([k, f]) => {
        const on = isOn(k, f.value);
        return html`<button type="button" class=${`chip-btn${on ? " on" : ""}`} key=${k + f.value} aria-pressed=${on ? "true" : "false"} data-filter=${filterOf(k, f.value)}
          onClick=${() => toggleFilter(k, f.value)} title=${on ? "Show them all again" : `Only ${k === "author" ? "by" : "tagged"} ${f.value}`}>${k === "tag" ? "#" : ""}${f.value} ${on ? html`<span aria-hidden="true">×</span>` : html`<span class="muted">${f.count}</span>`}</button>`;
      })}</div>` : null}
      <div class="browse-scroll">
        <${SearchNote} q=${s.q} found=${fresh ? `${fresh.total} found in ${name}` : ""} wider=${wider} onClear=${() => { setText(""); ui.set({ q: "" }); }} />
        ${result?.error ? html`<p class="warn-note" role="alert">${result.error}</p>` : null}
        ${result && !items.length && !result.error ? html`<div class="empty" id="no-results">${s.q ? `Nothing in ${name} matches that search.` : scope === "favs" ? "Nothing starred yet: star a model to keep it here." : "No models here yet."}</div>` : null}
        ${s.layout === "list" && items.length ? html`<${ListHead} sort=${s.sort} />` : null}
        <div class=${`results ${s.layout === "list" ? "list-view" : "grid-view"}${picked.length > 1 ? " picking" : ""}`} role="listbox" aria-multiselectable="true" aria-label="Models">
          ${items.map((m) => html`<${View} key=${m.id} m=${m} items=${items} selected=${s.picked.includes(m.id)} fav=${s.favs.includes(m.id)} onMenu=${onMenu} />`)}
        </div>
        ${result && result.total > items.length ? html`<button type="button" class="ghost more-btn" onClick=${() => setLimit(limit + PAGE)}>Show more (${result.total - items.length} left)</button>` : null}
      </div>
    </div>
    ${picked.length > 1 ? html`<${SelectedPanel} models=${picked} />`
      : html`<${ModelDetails} id=${picked[0]?.id || null} empty=${html`<${PlaceSummary} name=${name} result=${fresh} />`} />`}
  </div>`;
}

export { toast };
