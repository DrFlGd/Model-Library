// Tidying the files inside a model (docs/PLAN.md, "A: rename, delete and new
// folder inside a model"): Rename… (F2), New folder…, Move to folder… and
// Delete…, for the files and folders picked in its file list. Files inside an
// archive aren't changed here. Each change is journalled: Undo in the message
// and on Home.
import { html, useState, useRef, useLayoutEffect } from "../lib/html.js";
import { ui } from "./state.js";
import { api, loadOverview, recorded } from "./library.js";
import { Dialog, close } from "./dialogs.js";
import { fileSel, keyParts, normaliseKeys, clear, show } from "./filesel.js";
import { size } from "./details.js";

const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;
const base = (rel) => rel.slice(rel.lastIndexOf("/") + 1);
const parentOf = (rel) => (rel.includes("/") ? rel.slice(0, rel.lastIndexOf("/")) : "");
const PROTECTED = /^(model\.json|_thumbs(\/.*)?|_media)$/;
const ext = (name) => (/\.([a-z0-9]{1,8})$/i.exec(name)?.[1] || "").toLowerCase();

/** The loose files and folders picked (not files inside archives, not the
 *  model's own top folder), each as { rel, folder }. */
export function pickedLoose(keys = fileSel.get().picked) {
  return normaliseKeys(keys).map(keyParts).filter((p) => p.kind !== "entry" && p.file && !PROTECTED.test(p.file))
    .map((p) => ({ rel: p.file, folder: p.kind === "folder" }));
}

/** Where New folder… makes it: beside what's picked, else in the folder shown. */
function placeFor(picked) {
  if (picked.length) return parentOf(picked[0].rel);
  const p = keyParts(fileSel.get().shown || "d:");
  return p.kind === "folder" ? p.file : parentOf(p.file);
}

/** The model's folders, from its file list ("" is the top). */
function foldersOf(model) {
  const out = new Set([""]);
  for (const f of model.files_list || []) {
    const parts = f.rel.split("/").slice(0, -1);
    for (let i = 1; i <= parts.length; i++) out.add(parts.slice(0, i).join("/"));
  }
  return [...out].filter((d) => !/^_thumbs(\/|$)/.test(d)).sort((a, b) => a.localeCompare(b));
}

/** The actions, for fileActions (the bar, and every right-click menu on the files). */
export function tidyActions(model) {
  const ro = ui.get().library?.read_only ? "The library is read-only." : false;
  const picked = pickedLoose();
  const one = picked.length === 1 ? picked[0] : null;
  const out = [];
  if (one) out.push({ id: "file-rename", label: "Rename…", icon: "edit", key: "F2", disabled: ro,
    run: () => ui.set({ dialog: { type: "file-rename", model, rel: one.rel, folder: one.folder } }) });
  out.push({ id: "file-new-folder", label: "New folder…", icon: "folder", disabled: ro,
    run: () => ui.set({ dialog: { type: "file-new-folder", model, parent: placeFor(picked), rels: picked.map((p) => p.rel) } }) });
  if (picked.length) {
    out.push({ id: "file-move", label: "Move to folder…", icon: "move", disabled: ro,
      run: () => ui.set({ dialog: { type: "file-move", model, rels: picked.map((p) => p.rel) } }) });
    out.push({ sep: true }, { id: "file-delete", label: "Delete…", icon: "trash", danger: true, disabled: ro,
      run: () => ui.set({ dialog: { type: "file-delete", model, items: picked } }) });
  }
  return out;
}

/** F2 in a file list: Rename… the one picked (or shown) loose file or folder. */
export function renameKey(model) {
  if (ui.get().library?.read_only) return false;
  let picked = pickedLoose();
  if (!picked.length) picked = pickedLoose([fileSel.get().shown].filter(Boolean));
  if (picked.length !== 1) return false;
  ui.set({ dialog: { type: "file-rename", model, rel: picked[0].rel, folder: picked[0].folder } });
  return true;
}

async function run(model, args, showAfter = "d:") {
  const r = await api("model_files_change", { id: model.id, ...args });
  clear();
  show(showAfter);
  await loadOverview();
  close();
  if (r.journal) recorded(`${r.label}.`, r.journal);
  return r;
}

/** A name box with the part before the extension selected, as Windows does. */
function NameBox({ id, value, onInput, keepExt = true }) {
  const ref = useRef(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.focus();
    const dot = keepExt ? value.lastIndexOf(".") : -1;
    el.setSelectionRange(0, dot > 0 ? dot : value.length);
  }, []);
  return html`<input type="text" id=${id} ref=${ref} required maxlength="200" spellcheck="false" autocomplete="off" value=${value} onInput=${(e) => onInput(e.target.value)} />`;
}

export function RenameFileDialog({ model, rel, folder }) {
  const was = base(rel);
  const [name, setName] = useState(was);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const changedType = !folder && ext(was) !== ext(name.trim()) && name.trim();
  const submit = async () => {
    setBusy(true);
    setError("");
    try {
      const to = [parentOf(rel), name.trim()].filter(Boolean).join("/");
      await run(model, { op: "rename", rel, name }, `${folder ? "d" : "f"}:${to}`);
    } catch (e) { setError(e.message || String(e)); } finally { setBusy(false); }
  };
  return html`<${Dialog} title=${`Rename ${was}`} id="file-rename-dialog" onSubmit=${submit} busy=${busy || !name.trim() || name.trim() === was} error=${error} submitLabel="Rename">
    <label class="field-block"><span>${folder ? "Folder name" : "File name"}</span><${NameBox} id="file-rename-name" value=${name} onInput=${setName} keepExt=${!folder} /></label>
    ${changedType ? html`<p class="warn-note" id="file-rename-type">${ext(was) ? `The type changes from .${ext(was)}` : "The type changes"}${ext(name.trim()) ? ` to .${ext(name.trim())}` : " to none"}: apps may not open it.</p>` : null}
  <//>`;
}

export function NewFolderDialog({ model, parent, rels }) {
  const [name, setName] = useState("New folder");
  const [move, setMove] = useState(rels.length > 0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const submit = async () => {
    setBusy(true);
    setError("");
    try {
      const at = [parent, name.trim()].filter(Boolean).join("/");
      await run(model, { op: "new_folder", parent, name, move: move ? rels : [] }, `d:${at}`);
    } catch (e) { setError(e.message || String(e)); } finally { setBusy(false); }
  };
  return html`<${Dialog} title="New folder" id="file-new-folder-dialog" onSubmit=${submit} busy=${busy || !name.trim()} error=${error} submitLabel="Make folder">
    <p class="muted">In ${parent || `${model.name}'s top folder`}.</p>
    <label class="field-block"><span>Name</span><${NameBox} id="file-new-folder-name" value=${name} onInput=${setName} keepExt=${false} /></label>
    ${rels.length ? html`<label class="check-row" id="file-new-folder-move"><input type="checkbox" checked=${move} onChange=${(e) => setMove(e.target.checked)} />
      <span>Move ${rels.length === 1 ? base(rels[0]) : `the ${rels.length} selected`} into it</span></label>` : null}
  <//>`;
}

export function MoveToFolderDialog({ model, rels }) {
  // not into what's moving, nor where they all are already
  const folders = foldersOf(model).filter((d) => !rels.some((r) => d === r || d.startsWith(`${r}/`)));
  const here = new Set(rels.map(parentOf));
  const [to, setTo] = useState(folders.find((d) => !(here.size === 1 && here.has(d))) ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const submit = async () => {
    setBusy(true);
    setError("");
    try { await run(model, { op: "move", rels, to }, `d:${to}`); } catch (e) { setError(e.message || String(e)); } finally { setBusy(false); }
  };
  const label = (d) => (d ? d.split("/").join(" › ") : `${model.name} (top folder)`);
  return html`<${Dialog} title=${`Move ${rels.length === 1 ? base(rels[0]) : plural(rels.length, "item", "items")} to a folder`} id="file-move-dialog" onSubmit=${submit}
      busy=${busy || (here.size === 1 && here.has(to))} error=${error} submitLabel="Move">
    <div class="field-block move-folder-list" role="radiogroup" aria-label="Folder" id="file-move-folders">
      ${folders.map((d) => html`<label class="check-row long" key=${d} style=${{ paddingLeft: `${d ? d.split("/").length * 14 : 0}px` }}>
        <input type="radio" name="file-move-to" checked=${to === d} onChange=${() => setTo(d)} /><span>${d ? base(d) : label(d)}${here.size === 1 && here.has(d) ? html` <span class="muted">(where ${rels.length === 1 ? "it is" : "they are"})</span>` : null}</span></label>`)}
    </div>
    <p class="muted">For a new folder, close this and choose New folder…: it can take the selection with it.</p>
  <//>`;
}

export function DeleteFilesDialog({ model, items }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const inside = (rel) => (model.files_list || []).filter((f) => f.rel === rel || f.rel.startsWith(`${rel}/`));
  const files = [...new Set(items.flatMap((it) => inside(it.rel)))];
  const bytes = files.reduce((n, f) => n + (f.size || 0), 0);
  const submit = async () => {
    setBusy(true);
    setError("");
    try { await run(model, { op: "delete", rels: items.map((it) => it.rel) }, `d:${parentOf(items[0].rel)}`); } catch (e) { setError(e.message || String(e)); } finally { setBusy(false); }
  };
  const what = items.length === 1 ? base(items[0].rel) : plural(items.length, "item", "items");
  return html`<${Dialog} title=${`Delete ${what}`} id="file-delete-dialog" onSubmit=${submit} busy=${busy} error=${error} submitLabel="Delete" danger=${true}>
    <p>${plural(files.length, "file", "files")} (${size(bytes)}) from ${model.name}${items.some((it) => it.folder) ? ", folders included" : ""}. ${files.length === 1 && items.length === 1 ? "It goes" : "They go"} to the library's recovery storage, not the bin: Undo, in the message or on Home, puts ${files.length === 1 && items.length === 1 ? "it" : "them"} back.</p>
    <ul class="delete-file-list" id="file-delete-list">${items.slice(0, 8).map((it) => html`<li key=${it.rel}>${it.rel}${it.folder ? "/" : ""}</li>`)}${items.length > 8 ? html`<li class="muted">and ${items.length - 8} more</li>` : null}</ul>
  <//>`;
}
