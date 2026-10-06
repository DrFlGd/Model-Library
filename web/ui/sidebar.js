// The menu on the left: the library's places, each schema with its category
// tree, and settings. On narrow windows it's a drawer.
import { html } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, setPref } from "./state.js";
import { routeHash, schemaScope } from "./context.js";
import { Icon } from "./icons.js";

function NavLink({ route, label, icon, count, current, depth = 0, toggle = null }) {
  const on = current === route;
  return html`<li class="nav-row" style=${depth ? `--depth: ${depth}` : null}>
    ${toggle || html`<span class="nav-toggle-space"></span>`}
    <a class=${`nav-link${on ? " active" : ""}`} href=${routeHash(route)} aria-current=${on ? "page" : null}
      onClick=${() => ui.set({ navOpen: false })}>
      ${icon ? html`<span class="nav-icon">${Icon[icon](15)}</span>` : null}<span class="nav-label">${label}</span>
      ${count != null ? html`<span class="nav-count">${count}</span>` : null}</a></li>`;
}

/** A schema's category values, nested, each unfolding to the next level. */
function TreeRows({ schema, nodes, values, depth, current, open }) {
  return nodes.map((n) => {
    const path = [...values, n.value];
    const key = [schema.id, ...path].join("/");
    const unfolded = !!open[key];
    const toggle = n.children.length ? html`<button type="button" class="nav-toggle" aria-expanded=${unfolded ? "true" : "false"} aria-label=${`${unfolded ? "Fold" : "Unfold"} ${n.value}`}
      onClick=${() => setPref({ open: { ...open, [key]: !unfolded } })}>${Icon.chevron(14)}</button>` : null;
    return html`<${NavLink} key=${key} route=${`browse:${schemaScope(schema.id, path)}`} label=${n.value} count=${n.count} depth=${depth} current=${current} toggle=${toggle} />
      ${unfolded ? html`<${TreeRows} schema=${schema} nodes=${n.children} values=${path} depth=${depth + 1} current=${current} open=${open} />` : null}`;
  });
}

export function Sidebar() {
  const s = useStore(ui, (st) => ({ route: st.route, navOpen: st.navOpen, overview: st.overview, open: st.open, library: st.library }));
  const ov = s.overview;
  return html`${s.navOpen ? html`<div class="nav-backdrop" onClick=${() => ui.set({ navOpen: false })}></div>` : null}
    <nav class=${`sidebar${s.navOpen ? " open" : ""}`} aria-label="Library">
      <ul class="nav-list">
        <${NavLink} route="home" label="Home" icon="home" current=${s.route} />
        <${NavLink} route="browse:all" label="All models" icon="grid" count=${ov?.all} current=${s.route} />
        <${NavLink} route="browse:unsorted" label="Unsorted" icon="box" count=${ov?.unsorted} current=${s.route} />
        <${NavLink} route="browse:favs" label="Favourites" icon="star" current=${s.route} />
      </ul>
      <p class="nav-head">Categories</p>
      ${ov?.schemas?.length ? html`<ul class="nav-list" id="schema-tree">${ov.schemas.map((sc) => {
        const key = sc.id;
        const unfolded = s.open[key] !== false; // schemas start unfolded
        const toggle = sc.tree.length ? html`<button type="button" class="nav-toggle" aria-expanded=${unfolded ? "true" : "false"} aria-label=${`${unfolded ? "Fold" : "Unfold"} ${sc.name}`}
          onClick=${() => setPref({ open: { ...s.open, [key]: !unfolded } })}>${Icon.chevron(14)}</button>` : null;
        return html`<${NavLink} key=${key} route=${`browse:${schemaScope(sc.id)}`} label=${sc.name} icon="layers" count=${sc.count} current=${s.route} toggle=${toggle} />
          ${unfolded ? html`<${TreeRows} schema=${sc} nodes=${sc.tree} values=${[]} depth=${1} current=${s.route} open=${s.open} />` : null}`;
      })}</ul>` : html`<p class="muted nav-note">None yet.</p>`}
      ${s.library && !s.library.read_only ? html`<button type="button" class="ghost nav-add" id="new-schema" onClick=${() => ui.set({ dialog: { type: "new-schema" }, navOpen: false })}>${Icon.plus(14)} New category…</button>` : null}
      <ul class="nav-list nav-foot"><${NavLink} route="settings" label="Settings" icon="cog" current=${s.route} /></ul>
    </nav>`;
}
