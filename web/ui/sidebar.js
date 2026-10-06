// The menu on the left: the library's places, its schemas (Phase 1) and
// settings. On narrow windows it's a drawer.
import { html } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui } from "./state.js";
import { routeHash } from "./context.js";
import { Icon } from "./icons.js";

const PLACES = [
  ["home", "Home", "home"],
  ["browse:all", "All models", "grid"],
  ["browse:unsorted", "Unsorted", "box"],
  ["browse:favs", "Favourites", "star"],
];

function NavLink({ route, label, icon, current }) {
  const on = current === route;
  return html`<li class="nav-row"><a class=${`nav-link${on ? " active" : ""}`} href=${routeHash(route)} aria-current=${on ? "page" : null}
    onClick=${() => ui.set({ navOpen: false })}>
    <span class="nav-icon">${Icon[icon](15)}</span><span class="nav-label">${label}</span></a></li>`;
}

export function Sidebar() {
  const s = useStore(ui, (st) => ({ route: st.route, navOpen: st.navOpen }));
  return html`${s.navOpen ? html`<div class="nav-backdrop" onClick=${() => ui.set({ navOpen: false })}></div>` : null}
    <nav class=${`sidebar${s.navOpen ? " open" : ""}`} aria-label="Library">
      <ul class="nav-list">${PLACES.map(([route, label, icon]) => html`<${NavLink} key=${route} route=${route} label=${label} icon=${icon} current=${s.route} />`)}</ul>
      <p class="nav-head">Schemas</p>
      <p class="muted nav-note">None yet. Schemas arrive in the next phase.</p>
      <ul class="nav-list nav-foot"><${NavLink} route="settings" label="Settings" icon="cog" current=${s.route} /></ul>
    </nav>`;
}
