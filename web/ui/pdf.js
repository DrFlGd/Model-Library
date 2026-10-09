// PDF viewing in the file workspace. Archive entries are read on demand, without
// adding extracted files to the library. Blob URLs are bounded and reused.
import { html, useEffect, useState } from "../lib/html.js";
import { apiBytes, isDesktop, libraryUrl, openModelFile, openArchiveEntry } from "./library.js";

const CACHE_LIMIT = 4;
const MAX_CACHE_BYTES = 96 * 1024 * 1024;
const cache = new Map();
let cachedBytes = 0;

function cached(key) {
  const item = cache.get(key);
  if (!item) return null;
  cache.delete(key);
  cache.set(key, item);
  return item.url;
}

function remember(key, bytes) {
  const url = URL.createObjectURL(new Blob([bytes], { type: "application/pdf" }));
  if (bytes.byteLength <= MAX_CACHE_BYTES) {
    cache.set(key, { url, size: bytes.byteLength });
    cachedBytes += bytes.byteLength;
    while (cache.size > CACHE_LIMIT || cachedBytes > MAX_CACHE_BYTES) {
      const first = cache.keys().next().value;
      const old = cache.get(first);
      cache.delete(first);
      cachedBytes -= old.size;
      URL.revokeObjectURL(old.url);
    }
  }
  return { url, ephemeral: !cache.has(key) };
}

const errorText = (e) => e?.message || String(e);

/** A native PDF frame with accessible navigation. For ZIP entries only the
 * selected PDF is read; regular PDFs keep using the library's range protocol.
 * Page count is managed by the system PDF renderer, not guessed from raw bytes. */
export function PdfDocument({ src, doc, model }) {
  const [view, setView] = useState({ url: "", error: "", busy: true });
  const [page, setPage] = useState(1);
  const [zoom, setZoom] = useState("page-width");
  const stamp = model?.files_list?.find((f) => f.rel === doc.file);
  const key = [src.kind, src.id, doc.file, doc.entry || "", stamp?.size || "", stamp?.modified || ""].join("|");
  useEffect(() => { setPage(1); setZoom("page-width"); }, [key]);
  useEffect(() => {
    let live = true;
    let ephemeral = "";
    if (!doc.entry) {
      const rel = src.kind === "sort" ? "~sort/" + src.id + "/" + doc.file : src.rel + "/" + doc.file;
      setView({ url: libraryUrl(rel), error: "", busy: false });
    } else {
      const url = cached(key);
      if (url) {
        setView({ url, error: "", busy: false });
      } else {
        setView({ url: "", error: "", busy: true });
        apiBytes("model_pdf_entry", { [src.kind === "sort" ? "sort" : "id"]: src.id, file: doc.file, entry: doc.entry })
          .then((bytes) => {
            if (!live) return;
            const item = remember(key, bytes);
            ephemeral = item.ephemeral ? item.url : "";
            setView({ url: item.url, error: "", busy: false });
          }, (e) => { if (live) setView({ url: "", error: errorText(e), busy: false }); });
      }
    }
    return () => { live = false; if (ephemeral) URL.revokeObjectURL(ephemeral); };
  }, [key]);
  const move = (n) => setPage((p) => Math.max(1, Math.min(99999, p + n)));
  const fragment = zoom === "fit-page" ? "view=Fit" : zoom === "page-width" ? "view=FitH" : "zoom=" + zoom;
  const address = view.url ? view.url + "#toolbar=0&navpanes=0&page=" + page + "&" + fragment : "";
  return html`<div class="pdf-viewer" aria-label="PDF document">
    <div class="pdf-controls" role="toolbar" aria-label="PDF controls">
      <button type="button" class="ghost" disabled=${page <= 1 || view.busy} aria-label="Previous PDF page" onClick=${() => move(-1)}>‹</button>
      <label class="pdf-page-label">Page <input type="number" min="1" max="99999" aria-label="PDF page" value=${page} disabled=${view.busy} onChange=${(e) => setPage(Math.max(1, Math.min(99999, Number(e.target.value) || 1)))} /></label>
      <button type="button" class="ghost" disabled=${view.busy} aria-label="Next PDF page" onClick=${() => move(1)}>›</button>
      <label class="pdf-zoom-label">Zoom
        <select aria-label="PDF zoom" value=${zoom} disabled=${view.busy} onChange=${(e) => setZoom(e.target.value)}>
          <option value="page-width">Fit width</option><option value="fit-page">Fit page</option>
          <option value="75">75%</option><option value="100">100%</option><option value="125">125%</option><option value="150">150%</option><option value="200">200%</option>
        </select>
      </label>
    </div>
    ${view.busy ? html`<p role="status" class="stage-note muted">Opening the PDF…</p>` : null}
    ${view.error ? html`<p role="alert" class="form-error">This PDF could not be opened: ${view.error}. The archive has not been changed.</p>` : null}
    ${address ? html`<iframe class="doc-frame pdf-frame" key=${address} title=${doc.entry || doc.file} src=${address} onError=${() => setView((v) => ({ ...v, error: "The system PDF viewer could not display this file." }))}></iframe>` : null}
    ${doc.entry ? html`<p class="pdf-temporary-note muted">This PDF is read from the ZIP without changing the archive. Page display depends on the system PDF viewer. If the page is blank, use Open externally.</p>` : null}
    ${isDesktop() && (doc.entry || model) ? html`<button type="button" class="ghost pdf-external" onClick=${() => doc.entry ? openArchiveEntry(src, doc.file, doc.entry) : openModelFile(model, doc.file)}>Open externally</button>` : null}
  </div>`;
}
