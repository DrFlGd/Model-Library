// The model workspace's collapsible, resizable file panel.
import { html, useState, useEffect, useRef } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, setPref } from "./state.js";
import { SelectionRows } from "./parts.js";
import { ViewSwitch } from "./layout.js";
import { size } from "./details.js";

export function FilePanel({ src, model, files, variants, variant, setVariant, open, toggle, narrow, contextActions }) {
  const prefs = useStore(ui);
  const [q, setQ] = useState("");
  const stopResize = useRef(null);
  useEffect(() => () => stopResize.current?.(), []);
  const width = Math.max(220, Math.min(480, prefs.filePanelWidth || 300));
  const view = prefs.filePanelView || "folders";
  const words = q.toLowerCase().split(/\s+/).filter(Boolean);
  const filtered = files.filter((f) => words.every((word) => f.rel.toLowerCase().includes(word)));
  const resize = (e) => {
    e.preventDefault();
    stopResize.current?.();
    const start = e.clientX, before = width;
    const move = (event) => setPref({ filePanelWidth: Math.max(220, Math.min(480, before + event.clientX - start)) });
    const stop = () => { removeEventListener("pointermove", move); removeEventListener("pointerup", stop); removeEventListener("pointercancel", stop); };
    stopResize.current = stop;
    addEventListener("pointermove", move); addEventListener("pointerup", stop); addEventListener("pointercancel", stop);
  };
  return html`<aside id="file-panel" class=${`file-panel${open ? "" : " collapsed"}${narrow ? " drawer" : ""}`} style=${{ width: open ? `${width}px` : "36px" }} aria-label="Model files">
    ${!open ? html`<button type="button" class="ghost panel-open" id="file-panel-open" title="Open files ([)" aria-label="Open files ([)" onClick=${toggle}>›</button>` : html`
      <header class="file-panel-head"><div><strong>Files</strong><span class="muted">${files.length} · ${size(files.reduce((sum, f) => sum + (f.size || 0), 0))}</span></div><button type="button" class="ghost" id="file-panel-close" title="Collapse files ([)" aria-label="Collapse files ([)" onClick=${toggle}>‹</button></header>
      ${variants.length ? html`<div class="seg variant-seg" role="group" aria-label="Variant" id="variants">${variants.map((v) => html`<button type="button" key=${v} aria-pressed=${variant === v} onClick=${() => setVariant(v)}>${v}</button>`)}<button type="button" aria-pressed=${variant === null} onClick=${() => setVariant(null)}>All</button></div>` : null}
      <input type="search" class="parts-filter" placeholder="Search files" aria-label="Search files" value=${q} onInput=${(e) => setQ(e.target.value)} />
      <${ViewSwitch} id="parts-views" label="Show the files as" views=${[["folders", "Folders", "folder"], ["all", "List", "list"], ["type", "By type", "grouped"]]} value=${view} onChange=${(value) => setPref({ filePanelView: value })} />
      <div class="file-panel-scroll"><${SelectionRows} src=${src} files=${filtered} name=${model.name} view=${view} contextActions=${contextActions} />${!filtered.length && q ? html`<p class="muted">No files match.</p>` : null}</div>
      ${!narrow ? html`<div class="file-panel-resize" role="separator" aria-label="File panel width" aria-orientation="vertical" aria-valuemin="220" aria-valuemax="480" aria-valuenow=${width} tabIndex="0" onPointerDown=${resize} onKeyDown=${(e) => { if (["ArrowLeft", "ArrowRight"].includes(e.key)) { e.preventDefault(); setPref({ filePanelWidth: Math.max(220, Math.min(480, width + (e.key === "ArrowLeft" ? -20 : 20))) }); } }}></div>` : null}
    `}
  </aside>`;
}
