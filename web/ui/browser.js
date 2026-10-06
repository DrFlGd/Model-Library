// The browse view: one place of the library (all models, Unsorted, Favourites,
// or a schema and some of its categories), searched in the core, shown as a
// grid or a list, with the selected model's details beside it.
import { html, useState, useEffect, useRef } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, setPref } from "./state.js";
import { routeHash, schemaScope } from "./context.js";
import { Icon } from "./icons.js";
import { api, libraryUrl, toast, loadOverview } from "./library.js";

/** Open a model's own page. */
const openModel = (m) => { location.hash = routeHash(`model:${m.id}`); };
import { ModelDetails } from "./details.js";

const PAGE = 300;
const SORTS = [["name", "Name"], ["added", "Recently added"], ["size", "Largest"]];
const KIND_ICON = { model: "box", slicer: "stack", image: "image", doc: "file", video: "image", archive: "folder", other: "file" };

/** The place's title, as links back up its categories. */
function Title({ scope, overview }) {
  if (scope === "all") return html`<h1>All models</h1>`;
  if (scope === "unsorted") return html`<h1>Unsorted</h1>`;
  if (scope === "favs") return html`<h1>Favourites</h1>`;
  const [id, ...values] = scope.slice(7).split("/").map(decodeURIComponent);
  const schema = overview?.schemas?.find((s) => s.id === id);
  const crumbs = [[schema?.name || id, []], ...values.map((v, i) => [v, values.slice(0, i + 1)])];
  return html`<h1 class="crumbs">${crumbs.map(([label, path], i) => html`${i ? html`<span class="crumb-sep"> › </span>` : null}${
    i === crumbs.length - 1 ? label : html`<a href=${routeHash(`browse:${schemaScope(id, path)}`)}>${label}</a>`}`)}</h1>`;
}

/** Changing the category shown: edit it (its own page), or rename or move a value. */
function PlaceTools({ scope, readOnly, overview, empty }) {
  if (readOnly || !scope.startsWith("schema:")) return null;
  const [id, ...values] = scope.slice(7).split("/").map(decodeURIComponent);
  const sc = overview?.schemas?.find((s) => s.id === id);
  const add = sc && values.length < sc.levels.length
    ? html`<button type="button" class="ghost" id="add-subcategory" title=${`Add a ${sc.levels[values.length].label.toLowerCase()} here`} onClick=${() => ui.set({ dialog: { type: "add-subcategory", schemaId: id, path: values } })}>${Icon.plus(14)} Add ${sc.levels[values.length].label.toLowerCase()}…</button>` : null;
  const remove = values.length && empty
    ? html`<button type="button" class="ghost" id="remove-subcategory" title="Remove this empty subcategory and its folder" onClick=${async () => {
      try { await api("subcategory_remove", { schema: id, path: values }); await loadOverview(); location.hash = routeHash(`browse:${schemaScope(id, values.slice(0, -1))}`); toast(`Removed ${values[values.length - 1]}.`); }
      catch (e) { toast(e.message || String(e), 6000); } }}>Remove</button>` : null;
  return values.length
    ? html`${add}<button type="button" class="ghost" id="rename-node" title="Rename, merge or move this category, with its folders" onClick=${() => ui.set({ dialog: { type: "rename-node", schemaId: id, path: values } })}>${Icon.edit(14)} Rename or move…</button>${remove}`
    : html`${add}<button type="button" class="ghost" id="edit-schema" title="Edit this category: its levels, folders and fields" onClick=${() => ui.set({ dialog: { type: "edit-schema", schemaId: id } })}>${Icon.edit(14)} Edit category…</button>`;
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

/** Click: select one. Ctrl or Cmd click: add or remove it. Shift click: the run from the selected one. */
function choose(e, m, items) {
  const s = ui.get();
  if (e.ctrlKey || e.metaKey) {
    const picked = s.picked.length ? s.picked : s.selection ? [s.selection] : [];
    const next = picked.includes(m.id) ? picked.filter((id) => id !== m.id) : [...picked, m.id];
    ui.set({ picked: next, selection: next.length === 1 ? next[0] : next.includes(s.selection) ? s.selection : next[next.length - 1] || null });
  } else if (e.shiftKey && s.selection) {
    const ids = items.map((x) => x.id);
    const [a, b] = [ids.indexOf(s.selection), ids.indexOf(m.id)].sort((x, y) => x - y);
    if (a < 0) ui.set({ selection: m.id, picked: [] });
    else ui.set({ picked: ids.slice(a, b + 1) });
  } else {
    ui.set({ selection: m.id, picked: [] });
  }
}

function Card({ m, selected, fav, items }) {
  return html`<div class="card" role="option" tabindex="0" data-model=${m.id} aria-selected=${selected ? "true" : "false"}
    onClick=${(e) => choose(e, m, items)} onDblClick=${() => openModel(m)} onKeyDown=${(e) => { if (e.key === "Enter") openModel(m); }}>
    <${Cover} model=${m} />
    ${fav ? html`<span class="card-fav" title="Favourite">${Icon.star(14, true)}</span>` : null}
    <span class="card-name">${m.name}</span>
    <span class="card-sub">${m.authors.join(", ") || " "}</span>
    <span class="card-sub">${m.path.join(" › ") || (m.schema ? "" : "Unsorted")}</span>
  </div>`;
}

function Row({ m, selected, fav, items }) {
  return html`<div class="row" role="option" tabindex="0" data-model=${m.id} aria-selected=${selected ? "true" : "false"}
    onClick=${(e) => choose(e, m, items)} onDblClick=${() => openModel(m)} onKeyDown=${(e) => { if (e.key === "Enter") openModel(m); }}>
    <${Cover} model=${m} cls="thumb-sm" />
    <span class="row-name">${m.name}${fav ? html` <span class="badge-star">${Icon.star(12, true)}</span>` : null}</span>
    <span class="row-author">${m.authors.join(", ")}</span>
    <span class="row-cat">${m.path.join(" › ") || (m.schema ? "" : "Unsorted")}</span>
  </div>`;
}

/** Several models picked: what can be done to all of them. */
function Picked({ models }) {
  const readOnly = useStore(ui, (st) => !!st.library?.read_only);
  return html`<aside class="inspector" aria-label="Picked models" id="picked-panel">
    <h2 class="insp-title">${models.length} models picked</h2>
    <p class="insp-sub">Ctrl or Cmd click adds or removes one; Shift click picks a run.</p>
    <div class="insp-actions">
      <button type="button" class="ghost" id="edit-picked" disabled=${readOnly || !models.length} onClick=${() => ui.set({ dialog: { type: "edit-picked", models } })}>${Icon.edit(15)} Edit details…</button>
      <button type="button" class="ghost" id="move-picked" disabled=${readOnly || !models.length} onClick=${() => ui.set({ dialog: { type: "move-models", models } })}>${Icon.move(15)} Move to category…</button>
      <button type="button" class="ghost" onClick=${() => ui.set({ picked: [] })}>Clear</button>
    </div>
    <ul class="insp-files">${models.map((m) => html`<li key=${m.id}><span>${m.name}</span><span class="muted">${m.path.join(" › ") || "Unsorted"}</span></li>`)}</ul>
  </aside>`;
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
  const addFilter = (key, value) => {
    const f = `${key}:${/\s/.test(value) ? `"${value}"` : value}`;
    if (!s.q.includes(f)) ui.set({ q: `${s.q} ${f}`.trim() });
  };
  const items = result?.items || [];
  const facets = result?.facets || {};
  const chips = [...(facets.authors || []).slice(0, 6).map((f) => ["author", f]), ...(facets.tags || []).slice(0, 8).map((f) => ["tag", f])];
  const View = s.layout === "list" ? Row : Card;
  return html`<div class="browser">
    <div class="browse-main">
      <div class="browse-head">
        <div class="browse-title"><${Title} scope=${scope} overview=${s.overview} />
          <span class="browse-count" id="browse-count" data-q=${result ? result.q : null}>${result ? `${result.total} ${result.total === 1 ? "model" : "models"}` : ""}</span>
          <${PlaceTools} scope=${scope} readOnly=${!!s.library?.read_only} overview=${s.overview} empty=${!!result && !s.q && result.total === 0} /></div>
        <label class="search small browse-search"><span class="visually-hidden">Search this place</span>
          <input id="search" type="search" value=${text} placeholder="Search names, authors, tags… or author:jo tag:presupported" autocomplete="off" spellcheck="false"
            onInput=${(e) => setText(e.target.value)} /></label>
        <div class="browse-tools">
          <label class="tool-select"><span class="visually-hidden">Sort</span>
            <select id="sort" value=${s.sort} onChange=${(e) => setPref({ sort: e.target.value })}>${SORTS.map(([v, l]) => html`<option value=${v} key=${v}>${l}</option>`)}</select></label>
          <div class="seg" role="group" aria-label="View">
            <button type="button" aria-pressed=${s.layout === "grid" ? "true" : "false"} title="Grid" onClick=${() => setPref({ layout: "grid" })}>${Icon.grid(15)}</button>
            <button type="button" aria-pressed=${s.layout === "list" ? "true" : "false"} title="List" onClick=${() => setPref({ layout: "list" })} data-layout="list">${Icon.list(15)}</button>
          </div>
        </div>
      </div>
      ${chips.length ? html`<div class="filters" aria-label="Narrow down">${chips.map(([k, f]) => html`<button type="button" class="chip-btn" key=${k + f.value}
        onClick=${() => addFilter(k, f.value)} title=${`Only ${k === "author" ? "by" : "tagged"} ${f.value}`}>${k === "tag" ? "#" : ""}${f.value} <span class="muted">${f.count}</span></button>`)}</div>` : null}
      <div class="browse-scroll">
        ${result?.error ? html`<p class="warn-note" role="alert">${result.error}</p>` : null}
        ${result && !items.length && !result.error ? html`<div class="empty" id="no-results">${s.q ? "Nothing matches that search here." : scope === "favs" ? "No favourites yet: star a model to keep it here." : "No models here yet."}</div>` : null}
        <div class=${`results ${s.layout === "list" ? "list-view" : "grid-view"}`} role="listbox" aria-label="Models">
          ${items.map((m) => html`<${View} key=${m.id} m=${m} items=${items} selected=${s.picked.length > 1 ? s.picked.includes(m.id) : s.selection === m.id} fav=${s.favs.includes(m.id)} />`)}
        </div>
        ${result && result.total > items.length ? html`<button type="button" class="ghost more-btn" onClick=${() => setLimit(limit + PAGE)}>Show more (${result.total - items.length} left)</button>` : null}
      </div>
    </div>
    ${s.picked.length > 1 ? html`<${Picked} models=${items.filter((m) => s.picked.includes(m.id))} />` : html`<${ModelDetails} id=${s.selection} />`}
  </div>`;
}

export { toast };
