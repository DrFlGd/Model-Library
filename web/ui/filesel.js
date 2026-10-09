// Shared model-file selection. Keys: d:<folder>, f:<file>, z:<zip>!<entry>.
import { createStore } from "../lib/store.js";
export const fileSel = createStore({ src: null, shown: "d:", picked: [], anchor: null });
export function reset(src, shown = "d:") {
  fileSel.set({ src, shown, picked: shown === "d:" ? [] : [shown], anchor: shown });
}
export function show(key) { fileSel.set({ shown: key }); }
export function pick(key, how = "one", order = []) {
  const s = fileSel.get();
  let picked;
  if (how === "toggle") picked = s.picked.includes(key) ? s.picked.filter((k) => k !== key) : [...s.picked, key];
  else if (how === "range" && order.includes(s.anchor) && order.includes(key)) {
    const a = order.indexOf(s.anchor), b = order.indexOf(key);
    picked = order.slice(Math.min(a, b), Math.max(a, b) + 1);
  } else picked = [key];
  fileSel.set({ picked, shown: key, anchor: how === "range" ? s.anchor || key : key });
}
export function clear() { fileSel.set({ picked: [], anchor: null }); }
export function keyParts(key) {
  if (key.startsWith("z:")) { const i = key.indexOf("!"); return { kind: "entry", file: key.slice(2, i), entry: key.slice(i + 1) }; }
  return { kind: key.startsWith("d:") ? "folder" : "file", file: key.slice(2) };
}
/** Normalise parent/child overlaps, including ZIP entries, before counting or planning.
 * Keeps visual selection intact while preventing duplicated work. */
export function normaliseKeys(keys) {
  const uniq = [...new Set(keys)];
  const selected = new Set(uniq);
  const ancestor = (path) => {
    if (selected.has("d:")) return true;
    const parts = path.split("/");
    for (let i = 1; i < parts.length; i++) {
      if (selected.has("d:" + parts.slice(0, i).join("/"))) return true;
    }
    return false;
  };
  return uniq.filter((key) => {
    const p = keyParts(key);
    if (p.kind === "folder") {
      if (!p.file) return true;
      return !ancestor(p.file);
    }
    if (p.kind === "file") return !ancestor(p.file);
    if (selected.has("f:" + p.file) || ancestor(p.file)) return false;
    const parts = p.entry.replace(/\/$/, "").split("/");
    for (let i = 1; i < parts.length; i++) {
      if (selected.has("z:" + p.file + "!" + parts.slice(0, i).join("/") + "/")) return false;
    }
    return true;
  });
}
/** Folders remain paths: the core expands their contents and removes overlaps. */
export function pickedFiles(keys = fileSel.get().picked) {
  const files = [], entries = [];
  for (const key of normaliseKeys(keys)) {
    const p = keyParts(key);
    if (p.kind === "entry") entries.push({ file: p.file, entry: p.entry });
    else files.push(p.file);
  }
  return { files: [...new Set(files)], entries };
}
export function pickEvent(key, e, order) { pick(key, e.shiftKey ? "range" : e.ctrlKey || e.metaKey ? "toggle" : "one", order); }
