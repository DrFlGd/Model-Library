// Interface state shared by the islands (sidebar, main view, status bar).
// Preferences persist through the platform's store (the app keeps them on this
// computer; favourites go with the library).
import { createStore } from "../lib/store.js";

export const ui = createStore({
  ready: false,
  route: "home",           // home | browse:<all|unsorted|favs> | settings
  library: null,           // the open library's info (path, name, format, read_only)
  libraryError: null,
  recent: [],              // libraries opened before (paths), newest first
  favs: [],
  jobs: [],                // [{ id, label, started, cancel }]
  theme: "system",
  navOpen: false,          // sidebar drawer on small screens
});

let prefs = null;
const LAYOUT_KEY = "ml-ui";

/** Load saved preferences (call once the platform store exists). */
export function initState(store) {
  prefs = store.prefs;
  const saved = prefs.get(LAYOUT_KEY, {}) || {};
  ui.set({ favs: prefs.get("ml-favs", []) || [], theme: saved.theme || "system" });
  applyTheme();
  matchMedia("(prefers-color-scheme: dark)").addEventListener?.("change", applyTheme);
}

function savePrefs() {
  const s = ui.get();
  prefs?.set(LAYOUT_KEY, { theme: s.theme });
}

export function setPref(patch) {
  ui.set(patch);
  savePrefs();
}

export const THEMES = [["system", "Follow the system"], ["light", "Light"], ["dark", "Dark"], ["night", "Night (dim, warm, for a dark workshop)"]];

/** The theme in use: light, dark or night ("system" resolves to light or dark). */
export function resolvedTheme() {
  const t = ui.get().theme;
  if (t === "system") return matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  return t;
}
export function applyTheme() {
  document.documentElement.dataset.theme = resolvedTheme();
  window.dispatchEvent(new CustomEvent("ml-theme"));
}
/** Top-bar button: light -> dark -> night -> light. */
export function cycleTheme() {
  const next = { light: "dark", dark: "night", night: "light" }[resolvedTheme()];
  setTheme(next);
}
export function setTheme(theme) {
  setPref({ theme });
  applyTheme();
}

let jobSeq = 0;
/** Background work shown in the status bar. Returns a function that removes it;
 *  its .update(label) changes the text shown. */
export function addJob(label, cancel) {
  const job = { id: ++jobSeq, label, started: Date.now(), cancel };
  ui.set((s) => ({ jobs: [...s.jobs, job] }));
  const done = () => ui.set((s) => ({ jobs: s.jobs.filter((j) => j.id !== job.id) }));
  done.update = (text) => ui.set((s) => ({ jobs: s.jobs.map((j) => (j.id === job.id ? { ...j, label: text } : j)) }));
  return done;
}
