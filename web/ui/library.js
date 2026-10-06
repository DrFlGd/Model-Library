// The open library: opening another folder, renaming, rescanning, its overview
// (schemas, category trees, counts), and changes to models (details, stars).
import { ui, addJob } from "./state.js";
import { ctx } from "./context.js";

export const isDesktop = () => ctx.platform?.kind === "desktop";
export const api = (cmd, args) => ctx.platform.api(cmd, args);

/** Show a short message at the bottom of the window. */
export function toast(text, ms = 3500) {
  const el = document.getElementById("toast");
  if (!el) return;
  el.textContent = text;
  el.hidden = false;
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => { el.hidden = true; }, ms);
}

/** Read the app's info again and update what the interface shows. */
export async function refreshLibrary() {
  const info = await ctx.platform.refreshInfo();
  ui.set({ library: info.library || null, libraryError: info.library_error || null, recent: info.recent_libraries || [], firstRun: !!info.first_run });
  return info;
}

/** The schemas, their category trees and the counts (reads the library on first use). */
export async function loadOverview() {
  if (!ui.get().library) { ui.set({ overview: null }); return null; }
  const done = ui.get().overview ? () => {} : addJob("Reading the library");
  try {
    const overview = await api("library_overview");
    ui.set((s) => ({ overview, catalogRev: s.catalogRev + 1 }));
    return overview;
  } catch (e) {
    toast(`Couldn't read the library: ${e.message || e}`, 6000);
    return null;
  } finally {
    done();
  }
}

/** Read every model folder again (`full`: ignore what's cached on this computer). */
export async function rescan(full = true) {
  const done = addJob("Reading the library again");
  try {
    const r = await api("library_scan", { full });
    await loadOverview();
    toast(`${r.models} models, read in ${(r.ms / 1000).toFixed(1)} s.`);
  } finally {
    done();
  }
}

/** Open (or create) a library folder: `path`, or one picked in a dialog. */
export async function openLibrary(path) {
  const p = ctx.platform;
  const target = path || (await p.library.pickFolder("Open or create a library folder"));
  if (!target) return null;
  const done = addJob("Opening library");
  try {
    const info = await api("library_open", { path: target });
    await refreshLibrary();
    const prefs = await api("prefs_get").catch(() => ({}));
    ui.set({ favs: prefs["ml-favs"] || [], selection: null, overview: null });
    await loadOverview();
    toast(`Opened ${info.name}.`);
    return info;
  } catch (e) {
    toast(`Couldn't open that folder: ${e.message || e}`, 6000);
    return null;
  } finally {
    done();
  }
}

export async function renameLibrary(name) {
  await api("library_rename", { name });
  await refreshLibrary();
}

export function showLibraryFolder() {
  const path = ui.get().library?.path;
  if (path) ctx.platform.library.openPath(path);
}

/** A file in the library as the page can load it (cover pictures). */
export const libraryUrl = (rel) => ctx.platform.library?.url(rel);

/** Show a model's folder in the file manager. */
export function showModelFolder(model) {
  const lib = ui.get().library;
  if (lib) ctx.platform.library.openPath(`${lib.path}/${model.rel}`);
}

/** Star or unstar a model (starring gives it a model.json, so the star lasts). */
export async function setStar(model, on) {
  const r = await api("model_star", { id: model.id, on });
  ui.set((s) => ({ favs: r.favourites, selection: s.selection === model.id ? r.id : s.selection, catalogRev: s.catalogRev + 1 }));
  return r;
}

/** Save a model's details. Returns the model as the index has it now. */
export async function saveDetails(model, patch) {
  const v = await api("model_update", { id: model.id, patch });
  ui.set((s) => ({ selection: s.selection === model.id ? v.id : s.selection, favs: s.favs.map((f) => (f === model.id ? v.id : f)) }));
  await loadOverview();
  return v;
}
