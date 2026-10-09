// The model workspace has one viewer, driven by the shared file selection.
// Compact Import keeps FilesView and its tabs in parts.js.
import { html, useEffect } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui } from "./state.js";
import { typing } from "./actions.js";
import { size } from "./details.js";
import { TypeTag, fileType } from "./filetypes.js";
import { isDesktop, openModelFile, openArchiveEntry } from "./library.js";
import { MESH, Stage3D, Pictures, Documents, Videos } from "./parts.js";
import { fileSel, keyParts, pick } from "./filesel.js";
import { Contents, Breadcrumb, parentKey, parentPath, isFolderKey, useArchive, childrenOf, sortChildren } from "./contents.js";

export function WorkspaceStage({ src, model, files, contextActions }) {
  const shown = useStore(fileSel, (s) => s.shown) || "d:";
  const by = useStore(ui, (s) => s.contentsSort || "name");
  const p = keyParts(shown);
  const archive = p.kind === "entry" || (p.kind === "file" && /\.zip$/i.test(p.file)) ? p.file : null;
  const { entries, error } = useArchive(src, archive);
  const container = isFolderKey(shown) || (p.kind === "file" && !!archive);
  const name = p.entry || p.file;
  const current = { file: p.file, entry: p.entry || undefined };
  const f = p.kind === "entry" ? entries?.find((e) => e.name === p.entry) : files.find((f) => f.rel === p.file);
  // Only siblings in the currently filtered variant participate in stepping.
  const siblings = sortChildren(childrenOf(p.kind === "entry" ? entries : files, parentPath(name), p.kind === "entry" ? archive : null), by).filter((r) => !r.folder);
  const step = (delta) => {
    const index = siblings.findIndex((r) => r.key === shown);
    if (index >= 0 && siblings.length > 1) pick(siblings[(index + delta + siblings.length) % siblings.length].key);
  };
  useEffect(() => {
    const key = (e) => {
      if (e.defaultPrevented || typing(e) || ui.get().dialog || ui.get().menu || e.ctrlKey || e.metaKey || e.altKey) return;
      if (e.target.closest?.(".file-panel, .filepanel, select, video")) return;
      if (e.key === "Backspace" && shown !== "d:") { e.preventDefault(); pick(parentKey(shown)); }
      else if (!container && ["ArrowLeft", "ArrowRight"].includes(e.key)) { e.preventDefault(); step(e.key === "ArrowLeft" ? -1 : 1); }
    };
    addEventListener("keydown", key);
    return () => removeEventListener("keydown", key);
  });
  const group = fileType(name).group;
  const mesh = MESH.test(name), picture = group === "image", document = /\.pdf$/i.test(name) || (!p.entry && /\.(md|markdown|txt)$/i.test(name)), video = !p.entry && /\.(mp4|webm|m4v|mov)$/i.test(name);
  return html`<section class="mp-stage workspace-stage" id="workspace-stage" aria-label="Selected file">
    <${Breadcrumb} model=${model} shown=${shown} />
    ${container ? html`<${Contents} key=${shown} src=${src} model=${model} files=${files} shown=${shown} archive=${archive} entries=${entries} error=${error} contextActions=${contextActions} />`
      : mesh ? html`<${Stage3D} src=${src} model=${model} current=${current} />`
      : picture ? html`<${Pictures} key=${shown} src=${src} model=${model} pictures=${[current]} current=${current} setCurrent=${() => {}} />`
      : document ? html`<${Documents} key=${shown} src=${src} model=${model} docs=${[current]} current=${current} setCurrent=${() => {}} />`
      : video ? html`<${Videos} key=${shown} src=${src} model=${model} videos=${[current]} current=${current} setCurrent=${() => {}} />`
      : html`<div class="workspace-no-view"><h2>${name.split("/").pop()}</h2><p><${TypeTag} name=${name} /> · ${size(f?.size || 0)}</p>${isDesktop() ? html`<button type="button" class="ghost" onClick=${() => p.entry ? openArchiveEntry(src, p.file, p.entry) : openModelFile(model, p.file)}>Open externally</button>` : html`<p class="muted">No in-app viewer for this file.</p>`}</div>`}
    ${!container && siblings.length > 1 ? html`<div class="workspace-step"><button type="button" class="ghost" aria-label="Previous file" title="Previous file (←)" onClick=${() => step(-1)}>‹ Previous</button><button type="button" class="ghost" aria-label="Next file" title="Next file (→)" onClick=${() => step(1)}>Next ›</button></div>` : null}
  </section>`;
}
