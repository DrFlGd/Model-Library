// The open library: opening another folder, renaming, showing it in the file
// manager (desktop).
import { ui, addJob } from "./state.js";
import { ctx } from "./context.js";

export const isDesktop = () => ctx.platform?.kind === "desktop";

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
  ui.set({ library: info.library || null, libraryError: info.library_error || null, recent: info.recent_libraries || [] });
  return info;
}

/** Open (or create) a library folder: `path`, or one picked in a dialog. */
export async function openLibrary(path) {
  const p = ctx.platform;
  const target = path || (await p.library.pickFolder("Open or create a library folder"));
  if (!target) return null;
  const done = addJob("Opening library");
  try {
    const info = await p.api("library_open", { path: target });
    await refreshLibrary();
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
  await ctx.platform.api("library_rename", { name });
  await refreshLibrary();
}

export function showLibraryFolder() {
  const path = ui.get().library?.path;
  if (path) ctx.platform.library.openPath(path);
}
