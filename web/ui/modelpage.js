// A model's own page (docs/PLAN.md, "Phase 3 design"): a large viewer for its 3D
// files, pictures, documents and videos, and its files as a part tree, with a
// switch between variant folders (Presupported, Unsupported, Resin… as named in
// Settings). ZIPs unfold into their entries, which open straight from the archive.
import { html, useState, useEffect, useRef } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, resolvedTheme } from "./state.js";
import { routeHash, schemaScope } from "./context.js";
import { Icon } from "./icons.js";
import { api, apiBytes, isDesktop, libraryUrl, setStar, showModelFolder, openModelFile, toast, loadOverview } from "./library.js";
import { size } from "./details.js";

const MESH = /\.(stl|obj|3mf)$/i;
const MD = /\.(md|markdown|txt)$/i;
const VIDEO_OK = /\.(mp4|webm|m4v|mov)$/i;

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
function variantsIn(files, names) {
  const found = new Map();
  for (const f of files) for (const seg of f.rel.split("/").slice(0, -1)) if (isVariant(seg, names)) found.set(seg.toLowerCase(), seg);
  return [...found.values()].sort((a, b) => supported(b) - supported(a) || a.localeCompare(b));
}

const inVariant = (rel, variant, names) => {
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

const KIND_ICON = { model: "box", slicer: "stack", image: "image", doc: "file", video: "image", archive: "folder", other: "file" };

/** What a file opens as: { tab, file, entry }. */
function target(f, entry) {
  const name = entry || f.rel;
  if (MESH.test(name)) return { tab: "3d", file: f.rel, entry };
  if (f.kind === "image" || (entry && /\.(png|jpe?g|webp|gif|bmp|avif)$/i.test(entry))) return { tab: "pictures", file: f.rel, entry };
  if (!entry && f.kind === "doc") return { tab: "docs", file: f.rel };
  if (!entry && f.kind === "video") return { tab: "videos", file: f.rel };
  return null;
}

function ZipEntries({ model, f, open, current }) {
  const [entries, setEntries] = useState(null);
  const [error, setError] = useState("");
  useEffect(() => {
    api("model_zip", { id: model.id, file: f.rel }).then(setEntries, (e) => setError(e.message || String(e)));
  }, [model.id, f.rel]);
  if (error) return html`<li class="tree-note form-error">${error}</li>`;
  if (!entries) return html`<li class="tree-note muted">Reading the archive…</li>`;
  return entries.map((e) => {
    const t = target({ ...f, kind: e.kind }, e.name);
    const on = current?.file === f.rel && current?.entry === e.name;
    return html`<li key=${e.name} class="tree-file in-zip">
      <button type="button" class=${`tree-btn${on ? " on" : ""}`} disabled=${!t} data-entry=${e.name} onClick=${() => t && open(t)}>
        <span class="tree-icon">${(Icon[KIND_ICON[e.kind]] || Icon.file)(13)}</span><span class="tree-name">${e.name}</span><span class="muted tree-size">${size(e.size)}</span></button></li>`;
  });
}

function TreeNode({ node, model, open, current, depth, names }) {
  // Folders start open two levels down (a variant folder, then its parts).
  const [toggled, setToggled] = useState({});
  const [zipOpen, setZipOpen] = useState({});
  const isOpen = (path) => toggled[path] ?? depth < 2;
  const dirs = [...node.dirs.values()].sort((a, b) => a.name.localeCompare(b.name));
  return html`${dirs.map((d) => html`<li key=${d.path} class="tree-dir">
      <button type="button" class="tree-btn" aria-expanded=${isOpen(d.path) ? "true" : "false"} data-dir=${d.path}
        onClick=${() => setToggled({ ...toggled, [d.path]: !isOpen(d.path) })}>
        <span class="tree-icon">${Icon.folder(13)}</span><span class="tree-name">${d.name}</span>
        ${isVariant(d.name, names) ? html`<span class="badge">variant</span>` : null}</button>
      ${isOpen(d.path) ? html`<ul class="tree"><${TreeNode} node=${d} model=${model} open=${open} current=${current} depth=${depth + 1} names=${names} /></ul>` : null}
    </li>`)}
    ${node.files.map((f) => {
      const t = target(f);
      const zip = /\.zip$/i.test(f.rel);
      const on = current?.file === f.rel && !current?.entry;
      return html`<li key=${f.rel} class="tree-file">
        <button type="button" class=${`tree-btn${on ? " on" : ""}`} data-file=${f.rel} aria-expanded=${zip ? (zipOpen[f.rel] ? "true" : "false") : null}
          onClick=${() => (zip ? setZipOpen({ ...zipOpen, [f.rel]: !zipOpen[f.rel] }) : t ? open(t) : null)}>
          <span class="tree-icon">${(Icon[KIND_ICON[f.kind]] || Icon.file)(13)}</span><span class="tree-name">${f.name}</span><span class="muted tree-size">${size(f.size)}</span></button>
        ${zip && zipOpen[f.rel] ? html`<ul class="tree"><${ZipEntries} model=${model} f=${f} open=${open} current=${current} /></ul>` : null}
      </li>`;
    })}`;
}

function Stage3D({ model, current, onInfo }) {
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
    apiBytes("model_mesh", { id: model.id, file: current.file, entry: current.entry || null }).then((buf) => {
      if (!live) return;
      const info = viewer.current.show(buf);
      setState((s) => ({ ...s, busy: false, info }));
      onInfo?.(info);
    }, (e) => { if (live) { viewer.current?.clear(); setState((s) => ({ ...s, busy: false, info: null, error: e.message || String(e) })); } });
    return () => { live = false; };
  }, [state.ready, model.id, current?.file, current?.entry]);
  const cover = async () => {
    try {
      await api("model_cover", { id: model.id, snapshot: viewer.current.snapshot() });
      await loadOverview();
      toast("Saved this view as the model's cover.");
    } catch (e) { toast(`Couldn't save the cover: ${e.message || e}`, 6000); }
  };
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
        ${["iso", "top", "front"].map((v) => html`<button type="button" class="ghost" key=${v} onClick=${() => viewer.current?.view(v)}>${v === "iso" ? "3/4" : v[0].toUpperCase() + v.slice(1)}</button>`)}
        <button type="button" class="ghost" aria-pressed=${edges ? "true" : "false"} onClick=${() => { setEdges(!edges); viewer.current?.setEdges(!edges); }}>Edges</button>
        <button type="button" class="ghost" id="view-cover" disabled=${!info} onClick=${cover}>Use as cover</button>
      </span>
    </div>
  </div>`;
}

function Pictures({ model, pictures, current, setCurrent }) {
  const [url, setUrl] = useState(null);
  const pic = current || pictures[0];
  useEffect(() => {
    if (!pic) return;
    if (!pic.entry) { setUrl(libraryUrl(`${model.rel}/${pic.file}`)); return; }
    let live = true, made = null;
    apiBytes("model_entry", { id: model.id, file: pic.file, entry: pic.entry }).then((b) => {
      made = URL.createObjectURL(new Blob([b]));
      if (live) setUrl(made);
    }, () => {});
    return () => { live = false; if (made) URL.revokeObjectURL(made); };
  }, [model.id, pic?.file, pic?.entry]);
  const step = (d) => {
    const i = pictures.findIndex((p) => p.file === pic.file && p.entry === pic.entry);
    setCurrent(pictures[(i + d + pictures.length) % pictures.length]);
  };
  const cover = async () => {
    try {
      await api("model_cover", { id: model.id, file: pic.file });
      await loadOverview();
      toast("Saved as the model's cover.");
    } catch (e) { toast(`Couldn't set the cover: ${e.message || e}`, 6000); }
  };
  if (!pic) return html`<div class="stage-note muted">No pictures.</div>`;
  return html`<div class="stage-pictures">
    <div class="stage-picture">${url ? html`<img src=${url} alt=${pic.entry || pic.file} id="picture-big" />` : null}</div>
    <div class="stage-bar">
      <span class="stage-what">${pic.entry ? `${pic.file} › ${pic.entry}` : pic.file}</span>
      <span class="stage-tools">
        ${pictures.length > 1 ? html`<button type="button" class="ghost" onClick=${() => step(-1)} aria-label="Previous">‹</button><button type="button" class="ghost" onClick=${() => step(1)} aria-label="Next">›</button>` : null}
        <button type="button" class="ghost" id="picture-cover" disabled=${!!pic.entry || model.details?.cover === pic.file} onClick=${cover}>Use as cover</button>
      </span>
    </div>
    <div class="thumb-strip">${pictures.filter((p) => !p.entry).map((p) => html`<button type="button" key=${p.file} data-file=${p.file} class=${`strip-btn${p.file === pic.file && !pic.entry ? " on" : ""}`} onClick=${() => setCurrent(p)}>
      <img src=${libraryUrl(`${model.rel}/${p.file}`)} alt="" loading="lazy" /></button>`)}</div>
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

function Documents({ model, docs, current, setCurrent }) {
  const doc = current || docs[0];
  const [text, setText] = useState(null);
  useEffect(() => {
    setText(null);
    if (!doc || !MD.test(doc.file)) return;
    let live = true;
    api("model_doc", { id: model.id, file: doc.file }).then((r) => { if (live) setText(r.html); }, (e) => { if (live) setText(`<p>${String(e.message || e)}</p>`); });
    return () => { live = false; };
  }, [model.id, doc?.file]);
  if (!doc) return html`<div class="stage-note muted">No documents.</div>`;
  const lib = ui.get().library;
  return html`<div class="stage-docs">
    <div class="doc-list">${docs.map((d) => html`<button type="button" key=${d.file} class=${`ghost${d.file === doc.file ? " on" : ""}`} onClick=${() => setCurrent(d)}>${d.file}</button>`)}</div>
    <div class="doc-view">
      ${MD.test(doc.file) ? html`<div class="doc-text-view" id="doc-text" onClick=${followLink} dangerouslySetInnerHTML=${{ __html: text || "" }}></div>`
        : /\.pdf$/i.test(doc.file) ? html`<iframe class="doc-frame" title=${doc.file} src=${libraryUrl(`${model.rel}/${doc.file}`)}></iframe>`
        : html`<div class="stage-note muted">This kind of document opens in its own app.</div>`}
    </div>
    <div class="stage-bar"><span class="stage-what">${doc.file}</span>
      ${isDesktop() && lib ? html`<span class="stage-tools"><button type="button" class="ghost" onClick=${() => openModelFile(model, doc.file)}>Open in default app</button></span>` : null}</div>
  </div>`;
}

function Videos({ model, videos, current, setCurrent }) {
  const v = current || videos[0];
  if (!v) return html`<div class="stage-note muted">No videos.</div>`;
  return html`<div class="stage-videos">
    ${VIDEO_OK.test(v.file) ? html`<video class="stage-video" controls preload="metadata" src=${libraryUrl(`${model.rel}/${v.file}`)} key=${v.file}></video>`
      : html`<div class="stage-note muted">${v.file} plays in its own app.</div>`}
    <div class="doc-list">${videos.map((x) => html`<button type="button" key=${x.file} class=${`ghost${x.file === v.file ? " on" : ""}`} onClick=${() => setCurrent(x)}>${x.file}</button>`)}</div>
  </div>`;
}

export function ModelPage({ id }) {
  const s = useStore(ui, (st) => ({ rev: st.catalogRev, favs: st.favs, overview: st.overview, readOnly: !!st.library?.read_only, names: st.library?.variant_folders || [] }));
  const [m, setM] = useState(null);
  const [error, setError] = useState("");
  const [tab, setTab] = useState(null);
  const [variant, setVariant] = useState(undefined);
  const [cur, setCur] = useState({});
  const selection = useStore(ui, (st) => st.selection);
  useEffect(() => { ui.set({ selection: id, picked: [] }); }, [id]);
  useEffect(() => {
    // saving or moving a model can give it a new id: follow it
    if (selection && selection !== id && m) location.replace(routeHash(`model:${selection}`));
  }, [selection]);
  useEffect(() => {
    let live = true;
    api("model_get", { id }).then((v) => { if (live) { setM(v); setError(""); } }, (e) => { if (live) setError(e.message || String(e)); });
    return () => { live = false; };
  }, [id, s.rev]);
  if (error) return html`<div class="pages"><p class="warn-note" role="alert">${error}</p></div>`;
  if (!m) return html`<div class="pages"></div>`;
  const names = s.names;
  const variants = variantsIn(m.files_list, names);
  const chosen = variant === undefined || (variant && !variants.includes(variant)) ? variants[0] || null : variant;
  const files = m.files_list.filter((f) => inVariant(f.rel, chosen, names));
  const pictures = files.filter((f) => f.kind === "image").map((f) => ({ file: f.rel }));
  const docs = files.filter((f) => f.kind === "doc").map((f) => ({ file: f.rel }));
  const videos = files.filter((f) => f.kind === "video").map((f) => ({ file: f.rel }));
  const mainOk = m.main && inVariant(m.main.file, chosen, names);
  const first3d = mainOk ? m.main : (() => { const f = files.find((x) => MESH.test(x.rel)); return f ? { file: f.rel } : null; })();
  const tabs = [["3d", "3D", true], ["pictures", `Pictures (${pictures.length})`, pictures.length], ["docs", `Documents (${docs.length})`, docs.length], ["videos", `Videos (${videos.length})`, videos.length]].filter((t) => t[2]);
  const shown = tab || (first3d ? "3d" : tabs.find((t) => t[0] !== "3d")?.[0] || "3d");
  const open = (t) => { setCur({ ...cur, [t.tab]: t }); setTab(t.tab); };
  const schema = s.overview?.schemas?.find((x) => x.id === m.schema);
  const fav = s.favs.includes(m.id);
  const back = () => (history.length > 1 ? history.back() : (location.hash = routeHash("browse:all")));
  return html`<div class="model-page" id="model-page" data-model=${m.id}>
    <div class="mp-head">
      <button type="button" class="ghost" onClick=${back} aria-label="Back">‹ Back</button>
      <div class="mp-title"><h1 id="mp-name">${m.name}</h1>
        <p class="insp-sub">${m.authors.length ? `by ${m.authors.join(", ")} · ` : ""}${schema ? html`<a href=${routeHash(`browse:${schemaScope(schema.id)}`)}>${schema.name}</a>${m.path.map((v, i) => html` › <a href=${routeHash(`browse:${schemaScope(schema.id, m.path.slice(0, i + 1))}`)}>${v}</a>`)}` : html`<a href=${routeHash("browse:unsorted")}>Unsorted</a>`}</p></div>
      <div class="insp-actions">
        <button type="button" class="ghost" aria-pressed=${fav ? "true" : "false"} disabled=${s.readOnly} onClick=${() => setStar(m, !fav).catch((e) => toast(String(e.message || e)))}>${Icon.star(15, fav)} ${fav ? "Starred" : "Star"}</button>
        <button type="button" class="ghost" disabled=${s.readOnly} onClick=${() => ui.set({ dialog: { type: "edit-model", model: m, schema } })}>${Icon.edit(15)} Edit details…</button>
        <button type="button" class="ghost" disabled=${s.readOnly} onClick=${() => ui.set({ dialog: { type: "move-models", models: [m] } })}>${Icon.move(15)} Move…</button>
        ${isDesktop() ? html`<button type="button" class="ghost" onClick=${() => showModelFolder(m)}>${Icon.folder(15)} Show in folder</button>` : null}
      </div>
    </div>
    <div class="mp-body">
      <section class="mp-stage">
        <div class="seg mp-tabs" role="tablist">${tabs.map(([k, label]) => html`<button type="button" role="tab" key=${k} data-tab=${k} aria-pressed=${shown === k ? "true" : "false"} onClick=${() => setTab(k)}>${label}</button>`)}</div>
        ${shown === "3d" ? html`<${Stage3D} model=${m} current=${cur["3d"] && inVariant(cur["3d"].file, chosen, names) ? cur["3d"] : first3d} />` : null}
        ${shown === "pictures" ? html`<${Pictures} model=${m} pictures=${pictures} current=${cur.pictures} setCurrent=${(p) => setCur({ ...cur, pictures: p })} />` : null}
        ${shown === "docs" ? html`<${Documents} model=${m} docs=${docs} current=${cur.docs} setCurrent=${(d) => setCur({ ...cur, docs: d })} />` : null}
        ${shown === "videos" ? html`<${Videos} model=${m} videos=${videos} current=${cur.videos} setCurrent=${(v) => setCur({ ...cur, videos: v })} />` : null}
      </section>
      <aside class="mp-side">
        ${variants.length ? html`<div class="field-block"><span class="field-label">Variant</span>
          <div class="seg variant-seg" role="group" aria-label="Variant" id="variants">
            ${variants.map((v) => html`<button type="button" key=${v} aria-pressed=${chosen === v ? "true" : "false"} onClick=${() => setVariant(v)}>${v}</button>`)}
            <button type="button" aria-pressed=${chosen === null ? "true" : "false"} onClick=${() => setVariant(null)}>All</button>
          </div></div>` : null}
        <div class="field-block"><span class="field-label">Files <span class="muted">${files.length} · ${size(files.reduce((a, f) => a + f.size, 0))}</span></span>
          <ul class="tree" id="part-tree"><${TreeNode} node=${tree(files)} model=${m} open=${open} current=${cur[shown]} depth=${0} names=${names} key=${chosen || "all"} /></ul></div>
        ${m.details?.notes ? html`<div class="insp-section"><h3>Notes</h3><p class="insp-note">${m.details.notes}</p></div>` : null}
      </aside>
    </div>
  </div>`;
}
