// Direct children of a model folder or ZIP, sharing selection with the file panel.
import { html, useState, useEffect, useLayoutEffect, useRef } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, setPref } from "./state.js";
import { extractionAction } from "./extract.js";
import { ctx } from "./context.js";
import { Icon } from "./icons.js";
import { api, apiBytes, isDesktop, openModelFile } from "./library.js";
import { size } from "./details.js";
import { TypeTag, fileType } from "./filetypes.js";
import { SortMenu, ViewSwitch } from "./layout.js";
import { openMenu, typing } from "./actions.js";
import { MESH, LazyPreview, srcArgs, useAsCover, useSeen, queued } from "./parts.js";
import { fileSel, keyParts, pick, pickEvent, show, clear } from "./filesel.js";

export const parentPath = (path) => path.replace(/\/$/, "").split("/").slice(0, -1).join("/");
export function parentKey(key) {
  const p = keyParts(key);
  if (p.kind === "entry") {
    const parent = parentPath(p.entry);
    return parent ? `z:${p.file}!${parent}/` : `f:${p.file}`;
  }
  return `d:${parentPath(p.file)}`;
}
export const isFolderKey = (key) => key.startsWith("d:") || (key.startsWith("z:") && key.endsWith("/"));
export const viewable = (name) => /\.(stl|obj|3mf|png|jpe?g|webp|gif|bmp|avif|pdf|md|markdown|txt|mp4|webm|m4v|mov|zip)$/i.test(name);

/** Keep archive state tied to its source; late replies cannot replace another ZIP. */
export function useArchive(src, file) {
  const [state, setState] = useState({ entries: null, error: "" });
  useLayoutEffect(() => { setState({ entries: null, error: "" }); }, [src.id, file]);
  useEffect(() => {
    if (!file) return;
    let live = true;
    api("model_zip", { ...srcArgs(src), file }).then((entries) => { if (live) setState({ entries, error: "" }); }, (e) => { if (live) setState({ entries: [], error: e.message || String(e) }); });
    return () => { live = false; };
  }, [src.id, file]);
  return state;
}

/** Pure direct-child projection, also used by sibling navigation and checks. */
export function childrenOf(files, folder = "", archive = null) {
  const prefix = folder ? folder.replace(/\/$/, "") + "/" : "";
  const dirs = new Map(), rows = [];
  for (const f of files || []) {
    const rel = f.rel ?? f.name;
    if (!rel.startsWith(prefix) || rel === prefix) continue;
    const tail = rel.slice(prefix.length), slash = tail.indexOf("/");
    if (slash >= 0) {
      const name = tail.slice(0, slash), path = prefix + name;
      if (!name) continue;
      let d = dirs.get(path);
      if (!d) { d = { key: archive ? `z:${archive}!${path}/` : `d:${path}`, name, rel: path, file: archive || path, entry: archive ? path + "/" : null, folder: true, size: 0, count: 0, kind: "folder", modified: 0 }; dirs.set(path, d); }
      d.size += f.size || 0; if (!rel.endsWith("/")) d.count++;
      d.modified = Math.max(d.modified, Number(f.modified || f.mtime || 0));
    } else rows.push({ ...f, key: archive ? `z:${archive}!${rel}` : `f:${rel}`, name: tail, rel, file: archive || rel, entry: archive ? rel : null, folder: false });
  }
  return [...dirs.values(), ...rows];
}
export function sortChildren(rows, by) {
  const cmp = by === "size" ? (a, b) => (b.size || 0) - (a.size || 0)
    : by === "type" ? (a, b) => fileType(a.name).label.localeCompare(fileType(b.name).label)
    : by === "newest" ? (a, b) => Number(b.modified || b.mtime || 0) - Number(a.modified || a.mtime || 0) : () => 0;
  return [...rows].sort((a, b) => Number(b.folder) - Number(a.folder) || cmp(a, b) || a.name.localeCompare(b.name, undefined, { numeric: true }));
}
export function Breadcrumb({ model, shown }) {
  const p = keyParts(shown), crumbs = [{ key: "d:", name: model.name }];
  const folder = p.kind === "folder" ? p.file : parentPath(p.file);
  let path = "";
  for (const name of folder.split("/").filter(Boolean)) { path = path ? `${path}/${name}` : name; crumbs.push({ key: `d:${path}`, name }); }
  if (p.kind === "entry" || /\.zip$/i.test(p.file)) {
    crumbs.push({ key: `f:${p.file}`, name: p.file.split("/").pop() });
    path = "";
    for (const name of (p.entry || "").replace(/\/$/, "").split("/").filter(Boolean).slice(0, p.entry?.endsWith("/") ? undefined : -1)) {
      path = path ? `${path}/${name}` : name; crumbs.push({ key: `z:${p.file}!${path}/`, name });
    }
  }
  return html`<nav class="contents-breadcrumb" aria-label="File path">${crumbs.map((c, i) => html`<span key=${c.key}>${i ? html`<span class="muted"> › </span>` : null}<button type="button" class="ghost" data-crumb=${c.key} onClick=${() => pick(c.key)}>${c.name}</button></span>`)}</nav>`;
}
function ArchivePicturePreview({ src, row, fallback }) {
  const ref = useRef(null), seen = useSeen(ref);
  const [url, setUrl] = useState(null);
  useEffect(() => {
    if (!seen) return;
    let live = true, made = null;
    queued(() => apiBytes("model_entry", { ...srcArgs(src), file: row.file, entry: row.entry })).then((bytes) => {
      if (!live) return;
      made = URL.createObjectURL(new Blob([bytes])); setUrl(made);
    }, () => {});
    return () => { live = false; if (made) URL.revokeObjectURL(made); };
  }, [seen, src.id, row.file, row.entry]);
  return html`<span class="lazy-preview" ref=${ref}>${url ? html`<img src=${url} alt="" />` : fallback}</span>`;
}

export function Contents({ src, model, files, shown, archive, entries, error, contextActions }) {
  const picked = useStore(fileSel, (s) => s.picked);
  const prefs = useStore(ui, (s) => [s.contentsSort || "name", s.contentsView || "grid"]);
  const [limit, setLimit] = useState(500);
  useLayoutEffect(() => { setLimit(500); }, [shown, src.id]);
  const p = keyParts(shown), folder = archive ? p.entry || "" : p.file;
  const rows = sortChildren(childrenOf(archive ? entries : files, folder, archive), prefs[0]);
  const order = rows.map((r) => r.key);
  const open = (row) => {
    pick(row.key);
    if (!row.folder && !row.entry && !viewable(row.name) && isDesktop()) openModelFile(model, row.file);
  };
  const select = (row, e) => { pickEvent(row.key, e, order); show(shown); };
  const menu = (e, row) => {
    if (!picked.includes(row.key)) { pick(row.key); show(shown); }
    const lib = ui.get().library;
    openMenu(e, [
      { id: "view", label: "Show here", icon: "eye", run: () => open(row) },
      ...(!row.entry && isDesktop() ? [
        ...(!row.folder ? [{ id: "open-own", label: "Open in its own app", icon: "external", run: () => openModelFile(model, row.file) }] : []),
        { id: "folder", label: "Show in folder", icon: "folder", run: () => ctx.platform.library.openPath([lib.path, src.rel, row.folder ? row.rel : parentPath(row.rel)].filter(Boolean).join("/")) },
      ] : []),
      ...(!row.entry && fileType(row.name).group === "image" ? [{ id: "cover", label: "Use as cover", icon: "image", disabled: lib?.read_only ? "The library is read-only." : false, run: () => useAsCover(src.id, row.file) }] : []),
      extractionAction(src, model),
      ...(contextActions?.(row) || []),
    ]);
  };
  const key = (e) => {
    if (typing(e) || ui.get().dialog || ui.get().menu) return;
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "a") { e.preventDefault(); e.stopPropagation(); fileSel.set({ picked: order, anchor: order[0] || null }); }
    else if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); clear(); }
    else if (e.key === "Enter") { const row = rows.find((r) => r.key === e.target.closest?.("[data-key]")?.dataset.key) || rows.find((r) => r.key === picked[picked.length - 1]); if (row) { e.preventDefault(); open(row); } }
  };
  const preview = (row) => {
    const fallback = html`<span class=${`tile-icon${row.folder ? " folder-icon" : ""}`}>${row.folder ? Icon.folder(36) : (Icon[fileType(row.name).group === "image" ? "image" : "file"])(30)}</span>`;
    if (row.folder) return fallback;
    const image = fileType(row.name).group === "image";
    if (!image && !MESH.test(row.name)) return fallback;
    if (row.entry && image) return html`<${ArchivePicturePreview} key=${row.key} src=${src} row=${row} fallback=${fallback} />`;
    return html`<${LazyPreview} key=${row.key} cacheKey=${`${src.id}:${row.key}:${row.size}`} ask=${() => image ? Promise.resolve({ url: `${src.rel}/${row.file}` }) : api("file_preview", { ...srcArgs(src), file: row.file, entry: row.entry || null })} fallback=${fallback} />`;
  };
  return html`<div class="contents-view" id="contents-view" tabIndex="0" onKeyDown=${key}>
    <div class="contents-tools"><${SortMenu} id="contents-sort" options=${[["name", "Name"], ["type", "Type"], ["size", "Largest first"], ["newest", "Newest"]]} value=${prefs[0]} onChange=${(contentsSort) => setPref({ contentsSort })} /><${ViewSwitch} id="contents-views" views=${[["grid", "Grid", "grid"], ["list", "List", "list"]]} value=${prefs[1]} onChange=${(contentsView) => setPref({ contentsView })} /></div>
    ${error ? html`<p class="form-error" role="alert">${error}</p>` : archive && !entries ? html`<p class="muted">Reading the archive…</p>` : !rows.length ? html`<p class="muted">Nothing in this folder.</p>` : null}
    ${prefs[1] === "list" ? html`<div class="contents-columns">${[["name", "Name"], ["type", "Type"], ["size", "Size"]].map(([k, label]) => html`<button type="button" class="ghost" data-sort=${k} onClick=${() => setPref({ contentsSort: k })}>${label}${prefs[0] === k ? " ↓" : ""}</button>`)}</div>` : null}
    <div class=${prefs[1] === "list" ? "contents-list" : "file-grid contents-grid"} role="group" aria-label="Folder contents">${rows.slice(0, limit).map((row) => html`<button type="button" key=${row.key} class=${`${prefs[1] === "list" ? "contents-row" : "file-tile"}${picked.includes(row.key) ? " on" : ""}`} data-key=${row.key} aria-pressed=${picked.includes(row.key) ? "true" : "false"} title=${row.name} onClick=${(e) => select(row, e)} onDblClick=${() => open(row)} onContextMenu=${(e) => menu(e, row)}>
      ${prefs[1] !== "list" ? html`<span class="tile-pic">${preview(row)}${!row.folder ? html`<span class="tile-type"><${TypeTag} name=${row.name} /></span>` : null}</span>` : null}
      <span class="tile-name">${row.folder && prefs[1] === "list" ? html`<span class="folder-icon">${Icon.folder(15)}</span>` : null}${row.name}</span>
      ${prefs[1] === "list" ? html`<span>${row.folder ? "Folder" : html`<${TypeTag} name=${row.name} />`}</span>` : null}
      <span class="muted contents-size">${row.folder ? `${row.count} files` : size(row.size || 0)}</span></button>`)}</div>
    ${rows.length > limit ? html`<button type="button" class="ghost" onClick=${() => setLimit(limit + 500)}>Show more (${rows.length - limit} left)</button>` : null}
  </div>`;
}
