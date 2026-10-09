// Collection/inspector cover composition (Agent F). The index provides only
// paths and sizes; heavy mesh previews are requested after a tile is in view.
// Explicit model.json covers always win over this automatic composition.
import { html, useState, useEffect, useRef } from "../lib/html.js";
import { api, libraryUrl } from "./library.js";
import { Icon } from "./icons.js";
import { fileType } from "./filetypes.js";

const KIND_ICON = { model: "box", slicer: "stack", image: "image", doc: "file", video: "image", archive: "archive", other: "file" };
const KIND_LABEL = { model: "3D", slicer: "Slicer", image: "Image", doc: "Document", video: "Video", archive: "Archive", other: "File" };
const MAX_JOBS = 3;
let running = 0;
const waiting = [];
const requests = new Map();

/** Queue at most three render requests for *visible* tiles, not for all cards. */
function enqueue(task) {
  return new Promise((resolve, reject) => {
    const go = () => {
      running++;
      Promise.resolve().then(task).then(resolve, reject).finally(() => {
        running--;
        waiting.shift()?.();
      });
    };
    if (running < MAX_JOBS) go(); else waiting.push(go);
  });
}

function meshUrl(model, item) {
  const key = [model.rel, item.file, item.size, item.modified].join(":");
  if (!requests.has(key)) {
    requests.set(key, enqueue(() => api("file_preview", { id: model.id, file: item.file }))
      .then((result) => result?.url || null, () => null));
    // The index will generate a different key if content changes; keep the
    // promise cache bounded for long browsing sessions.
    if (requests.size > 1200) requests.delete(requests.keys().next().value);
  }
  return requests.get(key);
}

function observe(ref, onVisible) {
  const element = ref.current;
  if (!element) return undefined;
  if (!("IntersectionObserver" in window)) { onVisible(); return undefined; }
  const io = new IntersectionObserver((events) => {
    if (events.some((event) => event.isIntersecting)) {
      onVisible();
      io.disconnect();
    }
  }, { rootMargin: "120px" });
  io.observe(element);
  return () => io.disconnect();
}

function KindTile({ kind, count, label }) {
  const k = KIND_ICON[kind] || "file";
  return html`<span class="cover-kind" title=${label || `${count} ${KIND_LABEL[kind] || "File"} files`}>
    ${Icon[k]?.(25) || Icon.file(25)}
    <span>${label || KIND_LABEL[kind] || "File"}${count > 1 ? ` ×${count}` : ""}</span>
  </span>`;
}

/** A single file tile; failure shows its file type rather than an empty image. */
function FileTile({ model, item }) {
  const ref = useRef(null);
  const [seen, setSeen] = useState(false);
  const [rendered, setRendered] = useState(null);
  const [failed, setFailed] = useState(false);
  const version = [model.rel, item.file, item.size, item.modified].join(":");
  useEffect(() => { setSeen(false); setRendered(null); setFailed(false); }, [version]);
  useEffect(() => observe(ref, () => setSeen(true)), [version]);
  useEffect(() => {
    if (!seen || item.kind !== "model") return;
    let live = true;
    meshUrl(model, item).then((url) => { if (live) setRendered(url); });
    return () => { live = false; };
  }, [seen, version]);
  const source = item.kind === "image"
    ? libraryUrl(`${model.rel}/${item.file}`) + `?v=${item.modified || 0}-${item.size || 0}`
    : rendered ? libraryUrl(rendered) : null;
  return html`<span class="cover-file" ref=${ref} title=${item.file}>
    ${source && !failed
      ? html`<img src=${source} alt="" loading="lazy" onError=${() => setFailed(true)} />`
      : html`<${KindTile} kind=${item.kind} count=${1} label=${fileType(item.file).label} />`}
  </span>`;
}

function automaticTiles(model) {
  const files = model.files || {};
  const previews = (files.previews || []).filter((it) => it?.file).slice(0, 4);
  const previewable = files.previewable || previews.length;
  const total = files.count || 0;
  const tiles = previews.map((item) => ({ type: "preview", item }));
  if (previewable <= 1 && total > previewable) {
    // With just one drawable file, do not pretend the whole model is that one file.
    const otherKinds = Object.entries(files.kinds || {})
      .filter(([kind]) => !previews.some((p) => p.kind === kind))
      .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
    const kind = otherKinds[0]?.[0] || "other";
    tiles.push({ type: "kind", kind, count: total - previewable });
  }
  if (!tiles.length) {
    for (const [kind, count] of Object.entries(files.kinds || {}).sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).slice(0, 4)) {
      tiles.push({ type: "kind", kind, count });
    }
  }
  if (!tiles.length) tiles.push({ type: "kind", kind: "other", count: 0, label: "No files" });
  return { tiles: tiles.slice(0, 4), more: Math.max(0, previewable - previews.length) };
}

/** Shared by grid cards, list rows, Home and model details. */
export function Cover({ model, cls = "" }) {
  const files = model.files || {};
  const explicit = files.explicit_cover;
  const missing = !!files.cover_missing;
  const [broken, setBroken] = useState(false);
  const explicitVersion = `${model.rel}/${explicit || ""}:${files.cover_modified || 0}`;
  useEffect(() => setBroken(false), [explicitVersion]);
  const title = explicit
    ? missing || broken ? `Chosen cover unavailable: ${explicit}. Showing automatic preview. Choose another cover or Use automatic preview.`
      : `Chosen cover: ${explicit}`
    : `Automatic preview of ${files.count || 0} files`;
  if (explicit && !missing && !broken) {
    return html`<span class=${`thumb ${cls}`} title=${title} role="img" aria-label=${title}>
      <img src=${libraryUrl(`${model.rel}/${explicit}`) + `?v=${files.cover_modified || 0}`}
        alt="" loading="lazy" onError=${() => setBroken(true)} />
    </span>`;
  }
  const { tiles, more } = automaticTiles(model);
  return html`<span class=${`thumb cover-composed ${cls}`} title=${title} role="img" aria-label=${title}>
    <span class=${`cover-tiles cover-tiles-${tiles.length}`}>
      ${tiles.map((tile, i) => tile.type === "preview"
        ? html`<${FileTile} key=${tile.item.file} model=${model} item=${tile.item} />`
        : html`<${KindTile} key=${`${tile.kind}:${i}`} kind=${tile.kind} count=${tile.count} label=${tile.label} />`)}
    </span>
    ${more ? html`<span class="cover-count" title=${`${more} additional previewable files`}>+${more}</span>` : null}
    ${missing || broken ? html`<span class="cover-warning">Cover missing</span>` : null}
  </span>`;
}
