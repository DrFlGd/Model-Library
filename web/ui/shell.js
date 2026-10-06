// Mounts the interface islands into the page: top bar buttons, sidebar, the
// main view for the current route, and the status bar.
import { html, render } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, initState } from "./state.js";
import { setContext, ctx } from "./context.js";
import { Sidebar } from "./sidebar.js";
import { TopButtons, StatusBar } from "./chrome.js";
import { Home, BrowseEmpty } from "./home.js";
import { Settings } from "./settings.js";

function Main() {
  const route = useStore(ui, (s) => s.route);
  if (route === "settings") return html`<${Settings} />`;
  if (route.startsWith("browse:")) return html`<${BrowseEmpty} />`;
  return html`<${Home} />`;
}

export function mountShell(context) {
  setContext(context);
  window.__modlib = ctx; // for tests
  initState(context.platform.store);
  const info = context.platform.info || {};
  ui.set({ library: info.library || null, libraryError: info.library_error || null, recent: info.recent_libraries || [] });
  const mount = (id, C) => { const el = document.getElementById(id); if (el) render(html`<${C} />`, el); };
  mount("topbuttons", TopButtons);
  mount("sidebar-root", Sidebar);
  mount("main-root", Main);
  mount("statusbar", StatusBar);
}

export { ui };
