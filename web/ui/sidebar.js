// The menu on the left: the library's places, each category (with its icon),
// and settings. The category being browsed shows its subcategory tree in a
// panel beside the menu, which folds to a strip (docs/PLAN.md, "Group 2 design",
// R5); in narrow windows the menu is a drawer and the tree unfolds in it instead.
// Right-click a category for the same Category menu as its page, which also adds
// a subcategory (no row for it in the tree: it was too easy to click by mistake).
import { html, useState, useRef, useEffect } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, setPref } from "./state.js";
import { routeHash, schemaScope } from "./context.js";
import { Icon } from "./icons.js";
import { CatIcon, iconOf } from "./caticons.js";
import { openMenu } from "./actions.js";
import { categoryItems } from "./categories.js";
import { NavHead, ThemeButton, useMedia, useNavFolded } from "./chrome.js";

const NARROW = "(max-width: 900px)";
const useNarrow = () => useMedia(NARROW);

/** The category a route shows: { id, path } or null. */
export function routeCategory(route) {
  if (!route?.startsWith("browse:schema:")) return null;
  const [id, ...path] = route.slice(14).split("/").map(decodeURIComponent);
  return id ? { id, path: path.filter(Boolean) } : null;
}

function NavLink({ route, label, icon, count, current, depth = 0, toggle = null, menu = null, on = null, id = null }) {
  const active = on ?? current === route;
  return html`<li class="nav-row" style=${depth ? `--depth: ${depth}` : null}>
    ${toggle || html`<span class="nav-toggle-space"></span>`}
    <a class=${`nav-link${active ? " active" : ""}`} id=${id} href=${routeHash(route)} title=${label} aria-label=${label} aria-current=${current === route ? "page" : null}
      onClick=${() => ui.set({ navOpen: false })} onContextMenu=${menu ? (e) => openMenu(e, menu()) : null}
      onKeyDown=${menu ? (e) => { if ((e.shiftKey && e.key === "F10") || e.key === "ContextMenu") { e.preventDefault(); e.stopPropagation(); openMenu(e.currentTarget, menu()); } } : null}>
      ${typeof icon === "string" ? html`<span class="nav-icon">${Icon[icon](15)}</span>` : icon ? html`<span class="nav-icon">${icon}</span>` : null}<span class="nav-label">${label}</span>
      ${count != null ? html`<span class="nav-count">${count}</span>` : null}</a></li>`;
}

/** A schema's category values, nested, each unfolding to the next level. */
function TreeRows({ schema, nodes, values, depth, current, open }) {
  return nodes.map((n) => {
    const path = [...values, n.value];
    const key = [schema.id, ...path].join("/");
    const unfolded = !!open[key];
    const toggle = n.children.length ? html`<button type="button" class="nav-toggle fold-btn" aria-expanded=${unfolded ? "true" : "false"} aria-label=${`${unfolded ? "Fold" : "Unfold"} ${n.value}`}
      onClick=${() => setPref({ open: { ...open, [key]: !unfolded } })}>${Icon.chevron(14)}</button>` : null;
    const route = `browse:${schemaScope(schema.id, path)}`;
    const icon = iconOf(schema, path);
    return html`<${NavLink} key=${key} route=${route} label=${n.value} count=${n.count} depth=${depth} current=${current} toggle=${toggle}
        icon=${icon || Object.keys(schema.icons || {}).some(Boolean) ? html`<${CatIcon} icon=${icon} fallback="blank" />` : null} menu=${() => categoryItems(schema.id, path)} />
      ${unfolded ? html`<${TreeRows} schema=${schema} nodes=${n.children} values=${path} depth=${depth + 1} current=${current} open=${open} />` : null}`;
  });
}

/** Unfold the way to the subcategory shown, so it's in sight in the tree. */
function useOpenTo(schemaId, path) {
  const key = path ? [schemaId, ...path].join("/") : null;
  useEffect(() => {
    if (!path?.length) return;
    const open = ui.get().open, more = {};
    for (let i = 1; i < path.length; i++) {
      const k = [schemaId, ...path.slice(0, i)].join("/");
      if (!open[k]) more[k] = true;
    }
    if (Object.keys(more).length) setPref({ open: { ...open, ...more } });
  }, [key]);
}

/** The category's tree beside the menu: resizable, folds to a strip with its name. */
function CategoryPanel({ schema, path, route, open }) {
  const prefs = useStore(ui, (s) => [s.catPanel, s.catPanelWidth]);
  const medium = useMedia("(max-width: 1250px)");
  const folded = prefs[0] === "folded" || (prefs[0] == null && medium);
  const width = Math.max(180, Math.min(420, prefs[1] || 230));
  const stopResize = useRef(null);
  useEffect(() => () => stopResize.current?.(), []);
  useOpenTo(schema.id, path);
  useEffect(() => {
    const key = (e) => {
      if (e.key !== "[" || e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey || ui.get().dialog || ui.get().menu) return;
      const t = e.target;
      if (t && (t.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(t.tagName))) return;
      e.preventDefault();
      setPref({ catPanel: document.querySelector("#category-panel.folded") ? "open" : "folded" });
    };
    addEventListener("keydown", key);
    return () => removeEventListener("keydown", key);
  }, []);
  const top = `browse:${schemaScope(schema.id)}`;
  const resize = (e) => {
    e.preventDefault();
    stopResize.current?.();
    const start = e.clientX, before = width;
    const move = (event) => setPref({ catPanelWidth: Math.max(180, Math.min(420, before + event.clientX - start)) });
    const stop = () => { removeEventListener("pointermove", move); removeEventListener("pointerup", stop); removeEventListener("pointercancel", stop); };
    stopResize.current = stop;
    addEventListener("pointermove", move); addEventListener("pointerup", stop); addEventListener("pointercancel", stop);
  };
  if (folded) {
    return html`<aside class="category-panel folded" id="category-panel" aria-label=${`${schema.name}'s subcategories`}>
      <button type="button" class="ghost panel-strip" id="category-panel-open" title="Show subcategories ([)" aria-label="Show subcategories ([)" onClick=${() => setPref({ catPanel: "open" })}>
        <span class="panel-strip-icon"><${CatIcon} icon=${iconOf(schema)} size=${16} fallback="layers" /></span><span class="panel-strip-name">${schema.name}</span>${Icon.chevronsRight(13)}</button>
    </aside>`;
  }
  return html`<aside class="category-panel" id="category-panel" style=${{ width: `${width}px` }} aria-label=${`${schema.name}'s subcategories`}>
    <header class="cat-panel-head">
      <a class=${`cat-panel-title${route === top ? " active" : ""}`} href=${routeHash(top)} title=${schema.name}
        onContextMenu=${(e) => openMenu(e, categoryItems(schema.id, []))}><${CatIcon} icon=${iconOf(schema)} size=${17} fallback="layers" /><span>${schema.name}</span>
        <span class="nav-count">${schema.count}</span></a>
      <button type="button" class="ghost panel-fold" id="category-panel-close" title="Hide subcategories ([)" aria-label="Hide subcategories ([)" onClick=${() => setPref({ catPanel: "folded" })}>${Icon.chevronsLeft(14)}</button>
    </header>
    <ul class="nav-list cat-panel-tree" id="category-tree">
      <${TreeRows} schema=${schema} nodes=${schema.tree} values=${[]} depth=${0} current=${route} open=${open} />
    </ul>
    ${!schema.tree.length ? html`<p class="muted nav-note">No subcategories.</p>` : null}
    <div class="cat-panel-resize" role="separator" aria-label="Subcategory panel width" aria-orientation="vertical" aria-valuemin="180" aria-valuemax="420" aria-valuenow=${width} tabIndex="0"
      onPointerDown=${resize} onKeyDown=${(e) => { if (["ArrowLeft", "ArrowRight"].includes(e.key)) { e.preventDefault(); setPref({ catPanelWidth: Math.max(180, Math.min(420, width + (e.key === "ArrowLeft" ? -20 : 20))) }); } }}></div>
  </aside>`;
}

export function Sidebar() {
  const s = useStore(ui, (st) => ({ route: st.route, navOpen: st.navOpen, overview: st.overview, open: st.open, library: st.library, navCollapsed: st.navCollapsed, modelPlace: st.modelPlace }));
  const narrow = useNarrow();
  const { folded } = useNavFolded();
  const ov = s.overview;
  // a category's page, or the page of a model filed in one
  const shown = routeCategory(s.route) || (s.route.startsWith("model:") && s.modelPlace ? { id: s.modelPlace.schema, path: s.modelPlace.path } : null);
  const panelSchema = !narrow && shown ? ov?.schemas?.find((sc) => sc.id === shown.id) : null;
  return html`${s.navOpen ? html`<div class="nav-backdrop" onClick=${() => ui.set({ navOpen: false })}></div>` : null}
    <nav class=${`sidebar${s.navOpen ? " open" : ""}${folded ? " collapsed" : ""}`} aria-label="Library">
      <${NavHead} />
      <ul class="nav-list">
        <${NavLink} route="home" label="Home" icon="home" current=${s.route} />
        <${NavLink} route="import" label="Import" icon="inbox" current=${s.route} />
        <${NavLink} route="browse:all" label="All models" icon="grid" count=${ov?.all} current=${s.route} />
        <${NavLink} route="browse:unsorted" label="Unsorted" icon="box" count=${ov?.unsorted} current=${s.route} />
        <${NavLink} route="browse:favs" label="Starred" icon="star" count=${ov?.starred} current=${s.route} />
      </ul>
      <p class="nav-head">Categories</p>
      ${ov?.schemas?.length ? html`<ul class="nav-list" id="schema-tree">${ov.schemas.map((sc) => {
        const key = sc.id;
        const route = `browse:${schemaScope(sc.id)}`;
        const icon = html`<${CatIcon} icon=${iconOf(sc)} fallback="layers" />`;
        if (!narrow) {
          // the tree is in the panel beside the menu; here the category is one row
          return html`<${NavLink} key=${key} route=${route} label=${sc.name} icon=${icon} count=${sc.count} current=${s.route}
            on=${shown?.id === sc.id} menu=${() => categoryItems(sc.id, [])} />`;
        }
        const unfolded = s.open[key] !== false; // schemas start unfolded
        const toggle = sc.tree.length ? html`<button type="button" class="nav-toggle fold-btn" aria-expanded=${unfolded ? "true" : "false"} aria-label=${`${unfolded ? "Fold" : "Unfold"} ${sc.name}`}
          onClick=${() => setPref({ open: { ...s.open, [key]: !unfolded } })}>${Icon.chevron(14)}</button>` : null;
        return html`<${NavLink} key=${key} route=${route} label=${sc.name} icon=${icon} count=${sc.count} current=${s.route} toggle=${toggle}
            menu=${() => categoryItems(sc.id, [])} />
          ${unfolded ? html`<${TreeRows} schema=${sc} nodes=${sc.tree} values=${[]} depth=${1} current=${s.route} open=${s.open} />` : null}`;
      })}</ul>` : html`<p class="muted nav-note">None yet.</p>`}
      ${s.library && !s.library.read_only ? html`<button type="button" class="ghost nav-add" id="new-schema" onClick=${() => ui.set({ dialog: { type: "new-schema" }, navOpen: false })}>${Icon.plus(14)} New category…</button>` : null}
      <ul class="nav-list nav-foot">
        <${NavLink} route="duplicates" label="Duplicates" icon="stack" current=${s.route} />
        <${NavLink} route="settings" label="Settings" icon="cog" current=${s.route} />
        <${NavLink} route="about" label="About" icon="info" current=${s.route} />
        <${ThemeButton} />
      </ul>
    </nav>
    ${panelSchema ? html`<${CategoryPanel} key=${panelSchema.id} schema=${panelSchema} path=${shown.path} route=${s.route} open=${s.open} />` : null}`;
}
