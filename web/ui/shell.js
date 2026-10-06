// Mounts the interface islands into the page: top bar buttons, sidebar, the
// main view for the current route, and the status bar.
import { html, render } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, initState } from "./state.js";
import { setContext, ctx } from "./context.js";
import { Sidebar } from "./sidebar.js";
import { TopButtons, StatusBar } from "./chrome.js";
import { Home } from "./home.js";
import { Browser } from "./browser.js";
import { Dialogs } from "./dialogs.js";
import { Settings } from "./settings.js";
import { ImportPage, onDropped } from "./import.js";
import { loadOverview } from "./library.js";

function Main() {
  const route = useStore(ui, (s) => s.route);
  const blocked = useStore(ui, (s) => s.firstRun && !s.library);
  if (route === "settings" && !blocked) return html`<${Settings} />`;
  if (route === "import" && !blocked) return html`<${ImportPage} />`;
  if (route.startsWith("browse:") && !blocked) return html`<${Browser} key=${route} />`;
  return html`<${Home} />`;
}

export function mountShell(context) {
  setContext(context);
  window.__modlib = ctx; // for tests
  initState(context.platform.store);
  const info = context.platform.info || {};
  ui.set({ library: info.library || null, libraryError: info.library_error || null, recent: info.recent_libraries || [], firstRun: !!info.first_run });
  const mount = (id, C) => { const el = document.getElementById(id); if (el) render(html`<${C} />`, el); };
  mount("topbuttons", TopButtons);
  mount("sidebar-root", Sidebar);
  mount("main-root", Main);
  mount("statusbar", StatusBar);
  mount("dialog-root", Dialogs);
  loadOverview();
  context.platform.onDrop?.(onDropped);
}

export { ui };
