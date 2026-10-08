// Looking at a model's files (docs/PLAN.md, "Phase 3 design" and "Phase 5
// design"), on the model page and in the sorting workspace's details pane: a
// large viewer for its 3D files, pictures, documents and videos, and its files in
// four views: Folders (the part tree, with the variant switch), List (every file),
// By type, and Grid (a preview of each 3D file and picture). ZIPs unfold into
// their entries, which open straight from the archive. Right-click a library
// model's file to open it in its own app, show it in its folder or use it as the
// cover.
//
// `src` says whose files they are: { kind: "model", id, rel } for a model in the
// library, { kind: "sort", id } for one in the sorting workspace.
import { html, useState, useEffect, useLayoutEffect, useRef } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, resolvedTheme, setPref } from "./state.js";
import { ctx } from "./context.js";
import { Icon } from "./icons.js";
import { api, apiBytes, isDesktop, libraryUrl, openModelFile, toast, loadOverview, recorded, followIds } from "./library.js";
import { ViewSwitch, SortMenu } from "./layout.js";
import { size } from "./details.js";
import { openMenu, typing } from "./actions.js";
import { fileSel, pick, pickEvent, show, clear } from "./filesel.js";
import { TypeTag } from "./filetypes.js";

export const MESH = /\.(stl|obj|3mf)$/i;
const MD = /\.(md|markdown|txt)$/i;
const VIDEO_OK = /\.(mp4|webm|m4v|mov)$/i;
const PICTURE = /\.(png|jpe?g|webp|gif|bmp|avif)$/i;

/** The arguments that name a file's model to the core. */
export const srcArgs = (src) => (src.kind === "sort" ? { sort: src.id } : { id: src.id });
/** The address of one of a model's files. */
export const fileUrl = (src, rel) => libraryUrl(src.kind === "sort" ? `~sort/${src.id}/${rel}` : `${src.rel}/${rel}`);

const words = (s) => s.toLowerCase().split(/[^\p{L}\p{N}]+/u).filter(Boolean);

/** Whether a folder name is a variant: it is one of the library's variant
 *  names (Settings), or holds one as whole words ("Resin 32mm" holds "Resin"),
 *  with case, spaces and dashes ignored ("Pre-supported" is "Presupported"). */
export function isVariant(segment, names) {
  const fw = words(segment);
  return names.some((n) => {
    const tw = words(n);
    if (!tw.length) return false;
    if (fw.join("") === tw.join("")) return true;
    for (let i = 0; i + tw.length <= fw.length; i++) if (tw.every((w, k) => fw[i + k] === w)) return true;
    return false;
  });
}

/** Folders with supports come first (as the core's thumb::variant_of picks previews). */
const supported = (seg) => ["presupported", "supported", "withsupports", "supports"].includes(seg.toLowerCase().replace(/[^a-z0-9]/g, ""));

/** The variant folder names in a model (e.g. ["Presupported", "Unsupported"]). */
export function variantsIn(files, names) {
  const found = new Map();
  for (const f of files) for (const seg of f.rel.split("/").slice(0, -1)) if (isVariant(seg, names)) found.set(seg.toLowerCase(), seg);
  return [...found.values()].sort((a, b) => supported(b) - supported(a) || a.localeCompare(b));
}

export const inVariant = (rel, variant, names) => {
  if (!variant) return true;
  const segs = rel.split("/").slice(0, -1).filter((s) => isVariant(s, names));
  return !segs.length || segs.some((s) => s.toLowerCase() === variant.toLowerCase());
};

/** Files as a tree: { name, path, dirs: Map, files: [] }. */
function tree(files) {
  const root = { name: "", path: "", dirs: new Map(), files: [] };
  for (const f of files) {
    const parts = f.rel.split("/");
    let node = root;
    for (const p of parts.slice(0, -1)) {
      if (!node.dirs.has(p)) node.dirs.set(p, { name: p, path: node.path ? `${node.path}/${p}` : p, dirs: new Map(), files: [] });
      node = node.dirs.get(p);
    }
    node.files.push({ ...f, name: parts[parts.length - 1] });
  }
  return root;
}

export const KIND_ICON = { model: "box", slicer: "stack", image: "image", doc: "file", video: "image", archive: "archive", other: "file" };
const kindIcon = (kind, s = 13) => (Icon[KIND_ICON[kind]] || Icon.file)(s);
const TYPES = [["model", "3D models"], ["slicer", "Slicer projects"], ["image", "Pictures"], ["doc", "Documents"], ["video", "Videos"], ["archive", "Archives"], ["other", "Other files"]];

/** What a file opens as: { tab, file, entry }. */
function target(f, entry) {
  const name = entry || f.rel;
  if (MESH.test(name)) return { tab: "3d", file: f.rel, entry };
  if (f.kind === "image" || (entry && PICTURE.test(entry))) return { tab: "pictures", file: f.rel, entry };
  if (!entry && f.kind === "doc") return { tab: "docs", file: f.rel };
  if (!entry && f.kind === "video") return { tab: "videos", file: f.rel };
  return null;
}

const splitRel = (rel) => {
  const i = rel.lastIndexOf("/");
  return i < 0 ? ["", rel] : [rel.slice(0, i), rel.slice(i + 1)];
};

function ZipEntries({ src, f, open, current }) {
  const [entries, setEntries] = useState(null);
  const [error, setError] = useState("");
  useEffect(() => {
    api("model_zip", { ...srcArgs(src), file: f.rel }).then(setEntries, (e) => setError(e.message || String(e)));
  }, [src.id, f.rel]);
  if (error) return html`<li class="tree-note form-error">${error}</li>`;
  if (!entries) return html`<li class="tree-note muted">Reading the archive…</li>`;
  return entries.map((e) => {
    const t = target({ ...f, kind: e.kind }, e.name);
    const on = current?.file === f.rel && current?.entry === e.name;
    return html`<li key=${e.name} class="tree-file in-zip">
      <button type="button" class=${`tree-btn${on ? " on" : ""}`} disabled=${!t} data-entry=${e.name} onClick=${() => t && open(t)}>
        <span class="tree-icon tree-type"><${TypeTag} name=${e.name} /></span><span class="tree-name">${e.name}</span><span class="muted tree-size">${size(e.size)}</span></button></li>`;
  });
}

function TreeNode({ node, src, open, current, depth, names }) {
  // Folders start open two levels down (a variant folder, then its parts).
  const [toggled, setToggled] = useState({});
  const isOpen = (path) => toggled[path] ?? depth < 2;
  const dirs = [...node.dirs.values()].sort((a, b) => a.name.localeCompare(b.name));
  return html`${dirs.map((d) => html`<li key=${d.path} class="tree-dir">
      <button type="button" class="tree-btn" aria-expanded=${isOpen(d.path) ? "true" : "false"} data-dir=${d.path}
        onClick=${() => setToggled({ ...toggled, [d.path]: !isOpen(d.path) })}>
        <span class="tree-icon">${Icon.folder(13)}</span><span class="tree-name">${d.name}</span>
        ${isVariant(d.name, names) ? html`<span class="badge">variant</span>` : null}</button>
      ${isOpen(d.path) ? html`<ul class="tree"><${TreeNode} node=${d} src=${src} open=${open} current=${current} depth=${depth + 1} names=${names} /></ul>` : null}
    </li>`)}
    ${node.files.map((f) => html`<${FileRow} key=${f.rel} f=${f} name=${f.name} src=${src} open=${open} current=${current} />`)}`;
}

/** Whether a file can be opened in its own app from here (a library model's, on the desktop). */
const ownApp = (src) => src.kind === "model" && isDesktop();
const NO_VIEW = "This app has no viewer for this kind of file";

/** A file's right-click menu: open it in its own app, show it in its folder, use it as the cover. */
function fileMenu(e, src, f, open) {
  const t = target(f);
  const lib = ui.get().library;
  const dir = f.rel.includes("/") ? f.rel.slice(0, f.rel.lastIndexOf("/")) : "";
  const items = [
    ...(t ? [{ id: "view", label: "Show here", icon: "eye", run: () => open(t) }] : []),
    ...(ownApp(src) ? [
      { id: "open-own", label: "Open in its own app", icon: "external", run: () => openModelFile({ rel: src.rel }, f.rel) },
      { id: "folder", label: "Show in folder", icon: "folder", run: () => ctx.platform.library.openPath([lib.path, src.rel, dir].filter(Boolean).join("/")) },
    ] : []),
    ...(src.kind === "model" && f.kind === "image" ? [{ id: "cover", label: "Use as cover", icon: "image", disabled: lib?.read_only ? "The library is read-only." : false, run: () => useAsCover(src.id, f.rel) }] : []),
  ];
  openMenu(e, items);
}

/** Set a model's cover (a picture of its own, or `snapshot`: the 3D view), with Undo. */
async function setCover(id, args, said) {
  try {
    const v = await api("model_cover", { id, ...args });
    followIds({ [id]: v.id });
    await loadOverview();
    recorded(said, v.journal);
  } catch (e) { toast(`Couldn't set the cover: ${e.message || e}`, 6000); }
}

export const useAsCover = (id, file) => setCover(id, { file }, "Saved as the model's cover.");

/** One file in a list: opens it (a ZIP unfolds into its entries). A file with no
 *  viewer here is greyed; double-click opens it in its own app. */
function FileRow({ f, name, folder, src, open, current }) {
  const [zipOpen, setZipOpen] = useState(false);
  const t = target(f);
  const zip = /\.zip$/i.test(f.rel);
  const on = current?.file === f.rel && !current?.entry;
  const none = !t && !zip;
  return html`<li class="tree-file">
    <button type="button" class=${`tree-btn${on ? " on" : ""}${none ? " no-view" : ""}`} data-file=${f.rel} aria-expanded=${zip ? (zipOpen ? "true" : "false") : null}
      title=${none ? `${NO_VIEW}${ownApp(src) ? ": double-click to open it in its own app" : ""}` : null}
      onClick=${() => (zip ? setZipOpen(!zipOpen) : t ? open(t) : null)} onDblClick=${none && ownApp(src) ? () => openModelFile({ rel: src.rel }, f.rel) : null}
      onContextMenu=${(e) => fileMenu(e, src, f, open)}>
      <span class="tree-icon tree-type"><${TypeTag} name=${f.rel} /></span>
      <span class="tree-name">${name}${folder ? html`<span class="tree-folder muted">${folder}</span>` : null}</span>
      <span class="muted tree-size">${size(f.size)}</span></button>
    ${zip && zipOpen ? html`<ul class="tree"><${ZipEntries} src=${src} f=${f} open=${open} current=${current} /></ul>` : null}
  </li>`;
}

/** Every file in one list, sorted by folder, name, size or kind, with a filter. */
function AllFiles({ files, src, open, current }) {
  const [q, setQ] = useState("");
  const [by, setBy] = useState("folder");
  const words = q.toLowerCase().split(/\s+/).filter(Boolean);
  const rows = files
    .filter((f) => words.every((w) => f.rel.toLowerCase().includes(w)))
    .map((f) => { const [folder, name] = splitRel(f.rel); return { f, folder, name }; });
  const cmp = {
    folder: (a, b) => a.folder.localeCompare(b.folder) || a.name.localeCompare(b.name),
    name: (a, b) => a.name.localeCompare(b.name) || a.folder.localeCompare(b.folder),
    size: (a, b) => b.f.size - a.f.size,
    kind: (a, b) => TYPES.findIndex((t) => t[0] === a.f.kind) - TYPES.findIndex((t) => t[0] === b.f.kind) || a.name.localeCompare(b.name),
  }[by];
  rows.sort(cmp);
  return html`<div class="all-files" id="all-files">
    <div class="parts-tools">
      <input type="search" class="parts-filter" placeholder="Search files" aria-label="Search files" value=${q} onInput=${(e) => setQ(e.target.value)} />
      <${SortMenu} id="files-sort" options=${FILE_SORTS} value=${by} onChange=${setBy} />
    </div>
    <ul class="tree flat">${rows.map(({ f, folder, name }) => html`<${FileRow} key=${f.rel} f=${f} name=${name} folder=${folder} src=${src} open=${open} current=${current} />`)}</ul>
    ${!rows.length ? html`<p class="tree-note muted">No files match.</p>` : null}
  </div>`;
}

/** Files grouped by kind: 3D models, slicer projects, pictures, documents… */
function ByType({ files, src, open, current }) {
  return html`<div class="by-type" id="by-type">${TYPES.map(([kind, label]) => {
    const list = files.filter((f) => f.kind === kind);
    if (!list.length) return null;
    return html`<section key=${kind} class="type-group" data-kind=${kind}>
      <h4>${label} <span class="muted">${list.length} · ${size(list.reduce((a, f) => a + f.size, 0))}</span></h4>
      <ul class="tree flat">${list.map((f) => { const [folder, name] = splitRel(f.rel); return html`<${FileRow} key=${f.rel} f=${f} name=${name} folder=${folder} src=${src} open=${open} current=${current} />`; })}</ul>
    </section>`;
  })}</div>`;
}

// Previews are drawn a few at a time, as they come into view.
let running = 0;
const waiting = [];
export function queued(fn) {
  return new Promise((resolve, reject) => {
    const go = () => {
      running++;
      fn().then(resolve, reject).finally(() => { running--; waiting.shift()?.(); });
    };
    if (running < 3) go(); else waiting.push(go);
  });
}
const previews = new Map();

/** Whether an element has come into view (once it has, it stays true). */
export function useSeen(ref) {
  const [seen, setSeen] = useState(false);
  useEffect(() => {
    const el = ref.current;
    if (!el || seen) return;
    if (!("IntersectionObserver" in window)) { setSeen(true); return; }
    const io = new IntersectionObserver((es) => { if (es.some((e) => e.isIntersecting)) { setSeen(true); io.disconnect(); } }, { rootMargin: "300px" });
    io.observe(el);
    return () => io.disconnect();
  }, [seen]);
  return seen;
}

/** A preview drawn by the core, once it's in view: `ask` gives { url }. */
export function LazyPreview({ cacheKey, ask, fallback, alt = "" }) {
  const ref = useRef(null);
  const seen = useSeen(ref);
  const [url, setUrl] = useState(previews.get(cacheKey));
  useEffect(() => {
    if (!seen || url !== undefined) return;
    let live = true;
    queued(ask).then((r) => { previews.set(cacheKey, r?.url || null); if (live) setUrl(r?.url || null); }, () => { previews.set(cacheKey, null); if (live) setUrl(null); });
    return () => { live = false; };
  }, [seen, cacheKey]);
  const broken = () => { previews.set(cacheKey, null); setUrl(null); };
  return html`<span class="lazy-preview" ref=${ref}>${url ? html`<img src=${libraryUrl(url)} alt=${alt} onError=${broken} />` : url === null ? fallback : html`<span class="preview-wait"></span>`}</span>`;
}

/** A tile for each 3D file and picture (and the rest as icons). */
function FileGrid({ files, src, open, current }) {
  const [limit, setLimit] = useState(120);
  const order = (f) => (MESH.test(f.rel) || f.kind === "image" ? 0 : 1);
  const list = [...files].sort((a, b) => order(a) - order(b) || a.rel.localeCompare(b.rel));
  const shown = list.slice(0, limit);
  return html`<div class="file-grid-wrap" id="file-grid"><div class="file-grid">${shown.map((f) => {
    const [, name] = splitRel(f.rel);
    const t = target(f);
    const on = current?.file === f.rel && !current?.entry;
    const pic = f.kind === "image"
      ? html`<img src=${fileUrl(src, f.rel)} alt="" loading="lazy" />`
      : MESH.test(f.rel)
        ? html`<${LazyPreview} cacheKey=${`${src.kind}:${src.id}:${f.rel}:${f.size}`} ask=${() => api("file_preview", { ...srcArgs(src), file: f.rel })} fallback=${html`<span class="tile-icon">${kindIcon(f.kind, 28)}</span>`} />`
        : html`<span class="tile-icon">${kindIcon(f.kind, 28)}</span>`;
    return html`<button type="button" key=${f.rel} class=${`file-tile${on ? " on" : ""}${t ? "" : " no-view"}`} data-file=${f.rel} title=${t ? f.rel : `${f.rel}: ${NO_VIEW.toLowerCase()}`}
      onClick=${() => t && open(t)} onDblClick=${!t && ownApp(src) ? () => openModelFile({ rel: src.rel }, f.rel) : null} onContextMenu=${(e) => fileMenu(e, src, f, open)}>
      <span class="tile-pic">${pic}<span class="tile-type"><${TypeTag} name=${f.rel} /></span></span><span class="tile-name">${name}</span></button>`;
  })}</div>
  ${list.length > shown.length ? html`<button type="button" class="ghost" onClick=${() => setLimit(limit + 240)}>Show more (${list.length - shown.length} left)</button>` : null}</div>`;
}

const VIEWS = [["folders", "Folders", "folder"], ["all", "List", "list"], ["type", "By type", "grouped"], ["grid", "Grid", "grid"]];
const FILE_SORTS = [["folder", "Folder"], ["name", "Name"], ["size", "Largest first"], ["kind", "Kind"]];

/** A model's files in the view chosen last (Folders, All files, By type, Grid). */
export function PartsViews({ src, files, names, open, current, treeKey }) {
  const view = useStore(ui, (s) => s.partsView || "folders");
  return html`<div class="parts">
    <${ViewSwitch} id="parts-views" label="Show the files as" views=${VIEWS} value=${view} onChange=${(k) => setPref({ partsView: k })} />
    ${view === "folders" ? html`<ul class="tree" id="part-tree"><${TreeNode} node=${tree(files)} src=${src} open=${open} current=${current} depth=${0} names=${names} key=${treeKey} /></ul>` : null}
    ${view === "all" ? html`<${AllFiles} files=${files} src=${src} open=${open} current=${current} />` : null}
    ${view === "type" ? html`<${ByType} files=${files} src=${src} open=${open} current=${current} />` : null}
    ${view === "grid" ? html`<${FileGrid} files=${files} src=${src} open=${open} current=${current} />` : null}
  </div>`;
}

export function Stage3D({ src, model, current, onInfo }) {
  const canvas = useRef(null);
  const viewer = useRef(null);
  const [state, setState] = useState({ busy: false, error: "", info: null });
  const [edges, setEdges] = useState(false);
  const theme = useStore(ui, (s) => s.theme);
  useEffect(() => {
    let live = true;
    import("../viewer.js").then(({ Viewer }) => {
      if (!live || !canvas.current) return;
      viewer.current = new Viewer(canvas.current);
      viewer.current.setTheme(resolvedTheme() !== "light");
      setState((s) => ({ ...s, ready: true }));
    }, (e) => setState({ error: `The 3D viewer couldn't start: ${e.message || e}` }));
    return () => { live = false; viewer.current?.dispose(); viewer.current = null; };
  }, []);
  useEffect(() => { viewer.current?.setTheme(resolvedTheme() !== "light"); }, [theme]);
  useEffect(() => {
    if (!state.ready || !current) return;
    let live = true;
    setState((s) => ({ ...s, busy: true, error: "" }));
    apiBytes("model_mesh", { ...srcArgs(src), file: current.file, entry: current.entry || null }).then((buf) => {
      if (!live) return;
      const info = viewer.current.show(buf);
      setState((s) => ({ ...s, busy: false, info }));
      onInfo?.(info);
    }, (e) => { if (live) { viewer.current?.clear(); setState((s) => ({ ...s, busy: false, info: null, error: e.message || String(e) })); } });
    return () => { live = false; };
  }, [state.ready, src.id, current?.file, current?.entry]);
  const cover = () => setCover(model.id, { snapshot: viewer.current.snapshot() }, "Saved this view as the model's cover.");
  const info = state.info;
  return html`<div class="stage-3d">
    <div class="stage-canvas"><canvas ref=${canvas} id="viewer-canvas"></canvas>
      ${state.busy ? html`<div class="stage-note">Loading…</div>` : null}
      ${state.error ? html`<div class="stage-note form-error" role="alert">${state.error}</div>` : null}
      ${!current ? html`<div class="stage-note muted">No 3D file this app can show (STL, OBJ or 3MF).</div>` : null}</div>
    <div class="stage-bar">
      <span class="stage-what" id="viewer-file">${current ? (current.entry ? `${current.file} › ${current.entry}` : current.file) : ""}</span>
      ${info ? html`<span class="muted" id="viewer-size">${info.x.toFixed(1)} × ${info.y.toFixed(1)} × ${info.z.toFixed(1)} mm · ${info.triangles.toLocaleString()} triangles</span>` : null}
      <span class="stage-tools">
        ${["iso", "top", "front"].map((v) => html`<button type="button" class="ghost" key=${v} title=${{ iso: "Look from above at an angle", top: "Look from the top", front: "Look from the front" }[v]} onClick=${() => viewer.current?.view(v)}>${v === "iso" ? "3/4" : v[0].toUpperCase() + v.slice(1)}</button>`)}
        <button type="button" class="ghost" aria-pressed=${edges ? "true" : "false"} title="Show the edges of the triangles" onClick=${() => { setEdges(!edges); viewer.current?.setEdges(!edges); }}>Edges</button>
        ${model ? html`<button type="button" class="ghost" id="view-cover" disabled=${!info || !!ui.get().library?.read_only} title="Save this view as the model's cover picture" onClick=${cover}>Use as cover</button>` : null}
      </span>
    </div>
  </div>`;
}

export function Pictures({ src, model, pictures, current, setCurrent }) {
  const [url, setUrl] = useState(null);
  const pic = current || pictures[0];
  useEffect(() => {
    if (!pic) return;
    if (!pic.entry) { setUrl(fileUrl(src, pic.file)); return; }
    let live = true, made = null;
    apiBytes("model_entry", { ...srcArgs(src), file: pic.file, entry: pic.entry }).then((b) => {
      made = URL.createObjectURL(new Blob([b]));
      if (live) setUrl(made);
    }, () => {});
    return () => { live = false; if (made) URL.revokeObjectURL(made); };
  }, [src.id, pic?.file, pic?.entry]);
  const step = (d) => {
    const i = pictures.findIndex((p) => p.file === pic.file && p.entry === pic.entry);
    setCurrent(pictures[(i + d + pictures.length) % pictures.length]);
  };
  // ← and → go through the pictures (when not typing, and nothing else has the key)
  useEffect(() => {
    if (pictures.length < 2) return;
    const key = (e) => {
      if (e.defaultPrevented || typing(e) || ui.get().dialog || ui.get().menu || e.ctrlKey || e.metaKey || e.altKey) return;
      if (e.target.closest?.(".tree, .parts-views, select")) return;
      if (e.key === "ArrowLeft" || e.key === "ArrowRight") { e.preventDefault(); step(e.key === "ArrowLeft" ? -1 : 1); }
    };
    addEventListener("keydown", key);
    return () => removeEventListener("keydown", key);
  });
  const cover = () => useAsCover(model.id, pic.file);
  if (!pic) return html`<div class="stage-note muted">No pictures.</div>`;
  return html`<div class="stage-pictures">
    <div class="stage-picture">${url ? html`<img src=${url} alt=${pic.entry || pic.file} id="picture-big" />` : null}</div>
    <div class="stage-bar">
      <span class="stage-what">${pic.entry ? `${pic.file} › ${pic.entry}` : pic.file}</span>
      <span class="stage-tools">
        ${pictures.length > 1 ? html`<button type="button" class="ghost" onClick=${() => step(-1)} aria-label="Previous picture" title="Previous picture (←)">‹</button><button type="button" class="ghost" onClick=${() => step(1)} aria-label="Next picture" title="Next picture (→)">›</button>` : null}
        ${model ? html`<button type="button" class="ghost" id="picture-cover" disabled=${!!pic.entry || model.details?.cover === pic.file || !!ui.get().library?.read_only}
          title=${pic.entry ? "A picture inside an archive can't be the cover" : model.details?.cover === pic.file ? "This is the cover already" : "Use this picture as the model's cover"} onClick=${cover}>Use as cover</button>` : null}
      </span>
    </div>
    <div class="thumb-strip">${pictures.filter((p) => !p.entry).slice(0, 60).map((p) => html`<button type="button" key=${p.file} data-file=${p.file} class=${`strip-btn${p.file === pic.file && !pic.entry ? " on" : ""}`} onClick=${() => setCurrent(p)}>
      <img src=${fileUrl(src, p.file)} alt="" loading="lazy" /></button>`)}</div>
  </div>`;
}

/** Links in a readme open in the web browser, never in the app's window. */
function followLink(e) {
  const a = e.target.closest?.("a");
  if (!a) return;
  e.preventDefault();
  const href = a.getAttribute("href") || "";
  if (/^https?:\/\//i.test(href)) window.open(href, "_blank", "noopener");
}

export function Documents({ src, model, docs, current, setCurrent }) {
  const doc = current || docs[0];
  const [text, setText] = useState(null);
  useEffect(() => {
    setText(null);
    if (!doc || !MD.test(doc.file)) return;
    let live = true;
    api("model_doc", { ...srcArgs(src), file: doc.file }).then((r) => { if (live) setText(r.html); }, (e) => { if (live) setText(`<p>${String(e.message || e).replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" }[c]))}</p>`); });
    return () => { live = false; };
  }, [src.id, doc?.file]);
  if (!doc) return html`<div class="stage-note muted">No documents.</div>`;
  const lib = ui.get().library;
  return html`<div class="stage-docs">
    <div class="doc-list">${docs.map((d) => html`<button type="button" key=${d.file} class=${`ghost${d.file === doc.file ? " on" : ""}`} onClick=${() => setCurrent(d)}>${d.file}</button>`)}</div>
    <div class="doc-view">
      ${MD.test(doc.file) ? html`<div class="doc-text-view" id="doc-text" onClick=${followLink} dangerouslySetInnerHTML=${{ __html: text || "" }}></div>`
        : /\.pdf$/i.test(doc.file) ? html`<iframe class="doc-frame" title=${doc.file} src=${fileUrl(src, doc.file)}></iframe>`
        : html`<div class="stage-note muted">This kind of document opens in its own app.</div>`}
    </div>
    <div class="stage-bar"><span class="stage-what">${doc.file}</span>
      ${isDesktop() && lib && model ? html`<span class="stage-tools"><button type="button" class="ghost" onClick=${() => openModelFile(model, doc.file)}>${Icon.external(14)} Open in its own app</button></span>` : null}</div>
  </div>`;
}

export function Videos({ src, model, videos, current, setCurrent }) {
  const v = current || videos[0];
  if (!v) return html`<div class="stage-note muted">No videos.</div>`;
  return html`<div class="stage-videos">
    ${VIDEO_OK.test(v.file) ? html`<video class="stage-video" controls preload="metadata" src=${fileUrl(src, v.file)} key=${v.file}></video>`
      : html`<div class="stage-note muted">${v.file} plays in its own app.
        ${model && ownApp(src) ? html` <button type="button" class="ghost" id="video-open" onClick=${() => openModelFile(model, v.file)}>${Icon.external(14)} Open in its own app</button>` : null}</div>`}
    <div class="doc-list">${videos.map((x) => html`<button type="button" key=${x.file} class=${`ghost${x.file === v.file ? " on" : ""}`} onClick=${() => setCurrent(x)}>${x.file}</button>`)}</div>
  </div>`;
}

/** A model's files to look at: the stage (3D, pictures, documents, videos) and,
 *  beside it, the variant switch and the parts views (then `side`). `files`:
 *  [{ rel, size, kind }]; `main`: the 3D file shown first ({ file, entry }).
 *  `model` (a library model) adds its cover buttons. `compact` stacks them. */
export function FilesView({ src, files: allFiles, main, model, names, compact, side }) {
  const [tab, setTab] = useState(null);
  const [variant, setVariant] = useState(undefined);
  const [cur, setCur] = useState({});
  // before paint, so a file picked as the view appears isn't put back
  useLayoutEffect(() => { setTab(null); setVariant(undefined); setCur({}); }, [src.kind, src.id]);
  const variants = variantsIn(allFiles, names);
  const chosen = variant === undefined || (variant && !variants.includes(variant)) ? variants[0] || null : variant;
  const files = allFiles.filter((f) => inVariant(f.rel, chosen, names));
  const pictures = files.filter((f) => f.kind === "image").map((f) => ({ file: f.rel }));
  const docs = files.filter((f) => f.kind === "doc").map((f) => ({ file: f.rel }));
  const videos = files.filter((f) => f.kind === "video").map((f) => ({ file: f.rel }));
  const mainOk = main && inVariant(main.file, chosen, names);
  const first3d = mainOk ? main : (() => { const f = files.find((x) => MESH.test(x.rel)); return f ? { file: f.rel } : null; })();
  const tabs = [["3d", "3D", true], ["pictures", `Pictures (${pictures.length})`, pictures.length], ["docs", `Documents (${docs.length})`, docs.length], ["videos", `Videos (${videos.length})`, videos.length]].filter((t) => t[2]);
  const shown = tab || (first3d ? "3d" : tabs.find((t) => t[0] !== "3d")?.[0] || "3d");
  const open = (t) => { setCur({ ...cur, [t.tab]: t }); setTab(t.tab); };
  return html`<div class=${`mp-body${compact ? " compact" : ""}`}>
    <section class="mp-stage">
      <div class="seg mp-tabs" role="tablist">${tabs.map(([k, label]) => html`<button type="button" role="tab" key=${k} data-tab=${k} aria-pressed=${shown === k ? "true" : "false"} onClick=${() => setTab(k)}>${label}</button>`)}</div>
      ${shown === "3d" ? html`<${Stage3D} src=${src} model=${model} current=${cur["3d"] && inVariant(cur["3d"].file, chosen, names) ? cur["3d"] : first3d} />` : null}
      ${shown === "pictures" ? html`<${Pictures} src=${src} model=${model} pictures=${pictures} current=${cur.pictures} setCurrent=${(p) => setCur({ ...cur, pictures: p })} />` : null}
      ${shown === "docs" ? html`<${Documents} src=${src} model=${model} docs=${docs} current=${cur.docs} setCurrent=${(d) => setCur({ ...cur, docs: d })} />` : null}
      ${shown === "videos" ? html`<${Videos} src=${src} model=${model} videos=${videos} current=${cur.videos} setCurrent=${(v) => setCur({ ...cur, videos: v })} />` : null}
    </section>
    <aside class="mp-side">
      ${variants.length ? html`<div class="field-block"><span class="field-label">Variant</span>
        <div class="seg variant-seg" role="group" aria-label="Variant" id="variants">
          ${variants.map((v) => html`<button type="button" key=${v} aria-pressed=${chosen === v ? "true" : "false"} onClick=${() => setVariant(v)}>${v}</button>`)}
          <button type="button" aria-pressed=${chosen === null ? "true" : "false"} onClick=${() => setVariant(null)}>All</button>
        </div></div>` : null}
      <div class="field-block"><span class="field-label">Files <span class="muted">${files.length} · ${size(files.reduce((a, f) => a + f.size, 0))}</span></span>
        <${PartsViews} src=${src} files=${files} names=${names} open=${open} current=${cur[shown]} treeKey=${`${src.id}:${chosen || "all"}`} /></div>
      ${side || null}
    </aside>
  </div>`;
}

/** The shared file tree/list with the model workspace's multi-selection. */
export function SelectionRows({ src, files, name, view = "folders", contextActions }) {
  const sel = useStore(fileSel);
  const [folded, setFolded] = useState({});
  const [archives, setArchives] = useState({});
  const [sort, setSort] = useState("name");
  const root = tree(files);
  const rows = [];
  const source = useRef(src.id);
  useLayoutEffect(() => {
    if (source.current !== src.id) { source.current = src.id; setFolded({}); setArchives({}); }
  }, [src.id]);
  const addTree = (node, level) => {
    const key = `d:${node.path}`;
    rows.push({ key, name: node.path ? node.name : name, rel: node.path, folder: true, level });
    if (folded[key]) return;
    [...node.dirs.values()].sort((a, b) => a.name.localeCompare(b.name)).forEach((d) => addTree(d, level + 1));
    [...node.files].sort((a, b) => a.name.localeCompare(b.name)).forEach((f) => addFile(f, level + 1));
  };
  const addFile = (f, level) => {
    const key = `f:${f.rel}`;
    rows.push({ ...f, key, file: f.rel, name: splitRel(f.rel)[1], level });
    if (folded[key] !== false || !archives[f.rel]?.entries) return;
    const dirs = new Set();
    for (const e of archives[f.rel].entries) {
      const pieces = e.name.split("/");
      for (let i = 1; i < pieces.length; i++) dirs.add(pieces.slice(0, i).join("/") + "/");
    }
    const all = [...[...dirs].map((entry) => ({ entry, folder: true, size: 0 })), ...archives[f.rel].entries.filter((e) => !e.name.endsWith("/")).map((e) => ({ ...e, entry: e.name }))];
    all.sort((a, b) => a.entry.localeCompare(b.entry));
    for (const e of all) {
      const parents = e.entry.replace(/\/$/, "").split("/").slice(0, -1);
      if (parents.some((_, i) => folded[`z:${f.rel}!${parents.slice(0, i + 1).join("/")}/`])) continue;
      rows.push({ ...e, key: `z:${f.rel}!${e.entry}`, file: f.rel, name: e.entry.replace(/\/$/, "").split("/").pop(), rel: e.entry, level: level + 1 + parents.length });
    }
  };
  if (view === "folders") addTree(root, 0);
  else {
    const ordered = [...files].sort((a, b) => sort === "size" ? b.size - a.size : sort === "kind" ? a.kind.localeCompare(b.kind) || a.rel.localeCompare(b.rel) : a.rel.localeCompare(b.rel));
    if (view === "type") for (const [kind, label] of TYPES) {
      const list = ordered.filter((f) => f.kind === kind);
      if (list.length) { rows.push({ heading: `${label} · ${list.length}`, kind, key: `heading:${kind}` }); list.forEach((f) => addFile(f, 0)); }
    } else ordered.forEach((f) => addFile(f, 0));
  }
  const order = rows.filter((r) => !r.heading).map((r) => r.key);
  const unfold = async (row) => {
    const isZip = !row.entry && /\.zip$/i.test(row.rel);
    const now = folded[row.key] ?? isZip;
    setFolded((s) => ({ ...s, [row.key]: !now }));
    if (isZip && !archives[row.rel]) {
      setArchives((s) => ({ ...s, [row.rel]: { loading: true } }));
      const id = src.id;
      try { const entries = await api("model_zip", { ...srcArgs(src), file: row.rel }); if (source.current === id) setArchives((s) => ({ ...s, [row.rel]: { entries } })); }
      catch (e) { if (source.current === id) setArchives((s) => ({ ...s, [row.rel]: { error: e.message || String(e) } })); }
    }
  };
  // Opening a folder from tiles reveals its ancestors in the tree.
  useEffect(() => {
    const key = sel.shown;
    const path = key.slice(2).split("!")[0];
    setFolded((s) => {
      const next = { ...s, "d:": false };
      const bits = path.split("/");
      for (let i = 1; i < bits.length; i++) next[`d:${bits.slice(0, i).join("/")}`] = false;
      return next;
    });
    if (key.startsWith("z:")) {
      const file = key.slice(2).split("!")[0];
      if (!archives[file]) unfold({ key: `f:${file}`, rel: file });
      else setFolded((s) => ({ ...s, [`f:${file}`]: false }));
      const entry = key.slice(key.indexOf("!") + 1).replace(/\/$/, "").split("/");
      setFolded((s) => {
        const next = { ...s };
        for (let i = 1; i < entry.length; i++) next[`z:${file}!${entry.slice(0, i).join("/")}/`] = false;
        return next;
      });
    }
  }, [sel.shown]);
  const menu = (e, row) => {
    if (!sel.picked.includes(row.key)) pick(row.key);
    const extra = contextActions?.(row) || [];
    const lib = ui.get().library;
    openMenu(e, [
      { id: "view", label: "Show here", icon: "eye", run: () => show(row.key) },
      ...(ownApp(src) && !row.entry ? [
        { id: "open-own", label: "Open in its own app", icon: "external", run: () => openModelFile({ rel: src.rel }, row.rel) },
        { id: "folder", label: "Show in folder", icon: "folder", run: () => ctx.platform.library.openPath([lib.path, src.rel, row.folder ? row.rel : splitRel(row.rel)[0]].filter(Boolean).join("/")) },
      ] : []),
      ...(row.kind === "image" && !row.entry ? [{ id: "cover", label: "Use as cover", icon: "image", disabled: lib?.read_only ? "The library is read-only." : false, run: () => useAsCover(src.id, row.rel) }] : []), ...extra,
    ]);
  };
  const keys = (e) => {
    if (typing(e)) return;
    if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) {
      const row = rows.find((r) => r.key === (e.target.closest?.("[data-key]")?.dataset.key || sel.shown));
      if (row) { e.preventDefault(); menu(e, row); }
    }
    else if (e.key === "Escape") { clear(); e.preventDefault(); }
    else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "a") { fileSel.set({ picked: order, anchor: order[0] }); e.preventDefault(); }
    else if (["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)) {
      const i = order.indexOf(e.target.closest?.("[data-key]")?.dataset.key || sel.shown);
      const j = e.key === "Home" ? 0 : e.key === "End" ? order.length - 1 : Math.max(0, Math.min(order.length - 1, i + (e.key === "ArrowDown" ? 1 : -1)));
      if (order[j]) { pick(order[j], e.shiftKey ? "range" : "one", order); [...e.currentTarget.querySelectorAll("[data-key]")].find((el) => el.dataset.key === order[j])?.focus(); }
      e.preventDefault();
    }
  };
  return html`<div class="selection-rows" onKeyDown=${keys}>
    ${view === "all" ? html`<${SortMenu} id="files-sort" options=${[["name", "Name"], ["kind", "Type"], ["size", "Largest first"]]} value=${sort} onChange=${setSort} />` : null}
    <ul class="tree" id=${view === "folders" ? "part-tree" : view === "all" ? "all-files" : "by-type"}>${rows.map((r) => r.heading ? html`<li key=${r.key} class="file-type-heading" data-kind=${r.kind}>${r.heading}</li>` : html`<li key=${r.key} class=${r.folder ? "tree-dir" : "tree-file"}>
      <div class="file-selection-row" style=${{ paddingLeft: `${r.level * 14}px` }}>
        ${r.folder || (!r.entry && /\.zip$/i.test(r.rel)) ? html`<button class="ghost file-fold" type="button" aria-label=${`${(folded[r.key] ?? !r.folder) ? "Unfold" : "Fold"} ${r.name}`} aria-expanded=${!(folded[r.key] ?? !r.folder)} onClick=${() => unfold(r)}>${(folded[r.key] ?? !r.folder) ? "›" : "⌄"}</button>` : html`<span class="file-fold"></span>`}
        <button type="button" class=${`tree-btn${sel.picked.includes(r.key) ? " on" : ""}`} data-key=${r.key} data-file=${!r.folder && !r.entry ? r.rel : null} data-dir=${r.folder ? r.rel : null} data-entry=${r.entry || null} aria-pressed=${sel.picked.includes(r.key)}
          onClick=${(e) => pickEvent(r.key, e, order)} onContextMenu=${(e) => menu(e, r)} title=${r.rel || name}>
          <span class="tree-icon tree-type">${r.folder ? Icon.folder(13) : html`<${TypeTag} name=${r.entry || r.rel} />`}</span><span class="tree-name">${r.name}</span>${!r.folder ? html`<span class="muted tree-size">${size(r.size || 0)}</span>` : null}
        </button>
      </div>
      ${archives[r.rel]?.loading ? html`<p class="tree-note muted">Reading the archive…</p>` : archives[r.rel]?.error ? html`<p class="tree-note form-error">${archives[r.rel].error}</p>` : null}
    </li>`)}</ul>
  </div>`;
}
