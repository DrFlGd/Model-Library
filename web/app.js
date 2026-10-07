// Model Library: start-up and routing. The interface itself is in ui/
// (Preact + htm islands, no build step); the backend is reached through
// platform.js.
import { createPlatform } from "./platform.js";
import { mountShell, ui } from "./ui/shell.js";
import { routeFromHash } from "./ui/context.js";
import { watchLibrary } from "./ui/library.js";

// errors the tests read back (tests/desktop_page.py)
window.__errors = [];
window.addEventListener("error", (e) => window.__errors.push(String(e.message)));
window.addEventListener("unhandledrejection", (e) => window.__errors.push(String(e.reason?.message || e.reason)));

function route() {
  // the model last selected stays selected if the new place shows it
  ui.set((s) => ({ route: routeFromHash(location.hash), navOpen: false, menu: null, picked: s.selection ? [s.selection] : [], anchor: s.selection }));
}

async function start() {
  const platform = await createPlatform();
  mountShell({ platform });
  route();
  window.addEventListener("hashchange", route);
  ui.set({ ready: true });
  watchLibrary();
}

start();
