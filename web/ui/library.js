// The open library: opening another folder, renaming, rescanning, its overview
// (schemas, category trees, counts), and changes to models (details, stars).
import { ui, addJob } from "./state.js";
import { ctx } from "./context.js";

export const isDesktop = () => ctx.platform?.kind === "desktop";
export const api = (cmd, args) => ctx.platform.api(cmd, args);
export const apiBytes = (cmd, args) => ctx.platform.apiBytes(cmd, args);

/** Show a short message at the bottom of the window. `opts`: how long (ms), or
 *  { ms, action: { label, run } } for a button in the message (Undo). It stays
 *  while the pointer is on it. */
export function toast(text, opts = {}) {
  const { ms = 3500, action = null, actions = [] } = typeof opts === "number" ? { ms: opts } : opts;
  const el = document.getElementById("toast");
  if (!el) return;
  const span = document.createElement("span");
  span.textContent = text;
  el.replaceChildren(span);
  for (const item of [...(action ? [action] : []), ...actions]) {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "toast-action";
    b.textContent = item.label;
    b.onclick = () => { el.hidden = true; item.run(); };
    el.append(b);
  }
  el.hidden = false;
  const hide = (after) => { clearTimeout(toast.timer); toast.timer = setTimeout(() => { el.hidden = true; }, after); };
  el.onmouseenter = () => clearTimeout(toast.timer);
  el.onmouseleave = () => hide(2500);
  hide(ms);
}

/** Changes made in this window that can be undone, newest last: { text, undo }. */
const undos = [];

/** Say a change was made, with Undo in the message; Ctrl+Z undoes it too.
 *  `undo()` puts it back, and may return the message to show then. Returns a
 *  function that undoes it (for an Undo button on the page). */
export function undoable(text, undo, ms = 8000, actions = []) {
  const entry = { text, undo };
  undos.push(entry);
  if (undos.length > 20) undos.shift();
  toast(text, { ms, action: { label: "Undo", run: () => runUndo(entry) }, actions });
  return () => runUndo(entry);
}

/** "Moved Benchy to Office. More…" → "moved Benchy to Office": the first clause, for "Undone: …". */
const undoneText = (text) => {
  const first = text.split(/[.:](?:\s|$)/)[0];
  return /^[A-Z][a-z]/.test(first) ? first[0].toLowerCase() + first.slice(1) : first;
};

async function runUndo(entry) {
  const i = undos.indexOf(entry);
  if (i < 0) return toast("That's been undone already.");
  undos.splice(i, 1);
  try {
    const said = await entry.undo();
    toast(typeof said === "string" ? said : `Undone: ${undoneText(entry.text)}.`, 5000);
  } catch (e) {
    toast(`Couldn't undo it: ${e.message || e}`, 8000);
  }
}

/** Ctrl+Z: undo the last change made in this window. */
export function undoLast() {
  const entry = undos[undos.length - 1];
  if (!entry) return toast("Nothing to undo.");
  return runUndo(entry);
}

/** Models that got a new id (saving gives a model.json; moving changes a path id):
 *  the selection and stars follow them. `map`: { old id: new id }. */
export function followIds(map) {
  const to = (id) => map[id] || id;
  ui.set((s) => ({ selection: s.selection && to(s.selection), anchor: s.anchor && to(s.anchor), picked: s.picked.map(to), favs: s.favs.map(to) }));
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

/** Changes made outside the app: the core watches the library folder and reads
 *  what changed; the page asks every few seconds whether it found anything, and
 *  asks it to look itself when the window comes back to the front (network
 *  shares don't always report changes) and every few minutes. */
export function watchLibrary() {
  let rev = null;
  let looked = Date.now();
  const seen = async (r) => {
    const changed = rev !== null && r.rev !== rev;
    rev = r.rev;
    if (changed) await loadOverview();
  };
  const ask = (check) => {
    if (!ui.get().library || ui.get().firstRun) return;
    if (check) looked = Date.now();
    api("library_changes", check ? { check: true } : {}).then(seen, () => {});
  };
  setInterval(() => { if (!document.hidden) ask(false); }, 4000);
  setInterval(() => ask(true), 5 * 60 * 1000);
  const back = () => { if (!document.hidden && Date.now() - looked > 30000) ask(true); };
  window.addEventListener("focus", back);
  document.addEventListener("visibilitychange", back);
}

/** Read every model folder again (`full`: ignore what's cached on this computer). */
export async function rescan(full = true) {
  if (full) {
    // a job with Stop in the status bar; stopped, the library stays as read before
    try {
      const { job } = await api("library_scan", { full, job: true });
      const done = await followJob(job, "Reading the library again");
      if (done.error) throw new Error(done.error);
      const r = done.result || {};
      await loadOverview();
      toast(r.stopped ? "Stopped. The library shows what was read before." : `${r.models} models, read in ${(r.ms / 1000).toFixed(1)} s.`);
    } catch (e) {
      toast(`Couldn't read the library: ${e.message || e}`, 8000);
    }
    return;
  }
  const done = addJob("Reading the library again");
  try {
    const r = await api("library_scan", { full });
    await loadOverview();
    toast(`${r.models} models, read in ${(r.ms / 1000).toFixed(1)} s.`);
  } catch (e) {
    toast(`Couldn't read the library: ${e.message || e}`, 8000);
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
    ui.set({ favs: prefs["ml-favs"] || [], selection: null, anchor: null, picked: [], overview: null });
    undos.length = 0;
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

/** Set the folder names that mark variants (null: back to the defaults). */
export async function setVariantFolders(names) {
  await api("library_variants", { names });
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

/** Open one of a model's files in its default app. */
export function openModelFile(model, rel) {
  const lib = ui.get().library;
  if (lib) ctx.platform.library.openPath(`${lib.path}/${model.rel}/${rel}`);
}

/** Star or unstar a model (starring gives it a model.json, so the star lasts). */
export async function setStar(model, on) {
  const r = await api("model_star", { id: model.id, on });
  followIds({ [model.id]: r.id });
  ui.set({ favs: r.favourites });
  await loadOverview();
  return r;
}

/** Save a model's details, recorded so the message can undo it. Returns the
 *  model as the index has it now (`journal`: the change, for Undo). */
export async function saveDetails(model, patch) {
  const v = await api("model_update", { id: model.id, patch, journal: true });
  followIds({ [model.id]: v.id });
  await loadOverview();
  return v;
}

/** Say a recorded change was made (a move, an edit, an import), with Undo in the
 *  message; `after()` runs once it's undone (the page reads things again). */
export function recorded(text, journal, after, actions = []) {
  if (!journal) return toast(text, { actions });
  undoable(text, async () => {
    const r = await undoChange(journal);
    await after?.(r);
  }, 8000, actions);
}

/** Wait for a background job (importing), showing it in the status bar.
 *  `onProgress(job)` sees each poll. Resolves with the finished job. */
export async function followJob(id, label, onProgress) {
  const done = addJob(label, () => api("job_cancel", { id }));
  try {
    for (;;) {
      const job = await api("job", { id });
      onProgress?.(job);
      const p = job.progress || {};
      if (p.items) done.update(`${label}: ${Math.min(p.item + 1, p.items)} of ${p.items}`);
      if (job.done) return job;
      await new Promise((r) => setTimeout(r, 300));
    }
  } finally {
    done();
  }
}

/** What a category change would move: { label, models, moving, clashes, sample }. */
export const planChange = (change) => api("relayout_plan", { change });

async function changeJob(start, label) {
  const { job } = await start;
  const done = await followJob(job, label);
  await loadOverview();
  if (done.error) throw new Error(done.error);
  const r = done.result || {};
  if (r.failed?.length) throw new Error(`${r.failed.length} ${r.failed.length === 1 ? "model" : "models"} couldn't be moved (${r.failed[0].name}: ${r.failed[0].error}). Home has the change: finish it or put things back.`);
  return r;
}

/** Make a category change (rename, merge, edit, delete), moving folders to match. */
export const runChange = (change, label = "Moving folders") => changeJob(api("relayout_apply", { change }), label);
/** Undo a recorded change, or finish one that was interrupted. */
export const undoChange = (id) => changeJob(api("journal_undo", { id }), "Undoing");
export const finishChange = (id) => changeJob(api("journal_finish", { id }), "Finishing");
