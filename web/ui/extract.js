// Make selected model files a separate model, previewed and journalled by the core.
import { html, useState, useLayoutEffect } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui } from "./state.js";
import { routeHash } from "./context.js";
import { api, toast, followJob, loadOverview, recorded } from "./library.js";
import { writable } from "./actions.js";
import { Dialog, close } from "./dialogs.js";
import { CategoryPicker } from "./category.js";
import { fileSel, pickedFiles, keyParts, clear, show } from "./filesel.js";
import { sendFilesAction } from "./fileops.js";

export function extractionReason(src, model = src?.model, keys = fileSel.get().picked) {
  if (writable() !== true) return writable();
  if (!src || src.kind !== "model") return "Open a library model first.";
  if (!keys.length) return "Select files or folders first.";
  return false;
}

/** A selected folder's name, or the shared filename stem, with separators trimmed. */
export function extractionName(keys) {
  const names = keys.map((k) => { const p = keyParts(k); const n = (p.entry || p.file).replace(/\/$/, "").split("/").pop(); return p.kind === "folder" ? n : n.replace(/\.[^.]+$/, ""); });
  if (!names.length) return "New model";
  let stem = names[0];
  for (const n of names.slice(1)) { let i = 0; while (i < stem.length && i < n.length && stem[i].toLowerCase() === n[i].toLowerCase()) i++; stem = stem.slice(0, i); }
  return stem.replace(/[\s_.-]+$/, "") || "New model";
}

export function extractionAction(src, model = src?.model) {
  return { id: "extract", label: "Make a new model…", key: "N", icon: "plus", disabled: extractionReason(src, model), run: async () => {
    const keys = [...fileSel.get().picked];
    const reason = extractionReason(src, model, keys);
    if (reason) return toast(reason);
    try {
      const fresh = await api("model_get", { id: src.id });
      const latest = extractionReason(src, fresh, keys);
      if (latest) return toast(latest);
      ui.set({ dialog: { type: "extract-model", model: fresh, keys } });
    } catch (e) { toast(e.message || String(e)); }
  } };
}

export function installExtractKeys() {
  const key = (e) => {
    if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey || e.key.toLowerCase() !== "n" || ui.get().dialog || ui.get().menu || e.target.closest?.("input, textarea, select, [contenteditable=true]")) return;
    const src = fileSel.get().src;
    if (!src || !ui.get().route.startsWith("model:")) return;
    e.preventDefault(); extractionAction(src).run();
  };
  addEventListener("keydown", key);
  return () => removeEventListener("keydown", key);
}

export function ExtractionBar({ src, model = src?.model }) {
  const selection = useStore(fileSel);
  useStore(ui, (s) => s.library?.read_only);
  if (!selection.picked.length) return null;
  const action = extractionAction(src, model);
  const p = pickedFiles(selection.picked);
  const count = new Set((model?.files_list || []).filter((f) => p.files.some((r) => !r || f.rel === r || f.rel.startsWith(`${r}/`))).map((f) => f.rel)).size + p.entries.length;
  return html`<div class="extraction-bar" id="extraction-bar" aria-label="Selected files">
    <span>${count || selection.picked.length} ${(count || selection.picked.length) === 1 ? "file selected" : "files selected"}</span>
    <button type="button" class="ghost" id="extract-model" disabled=${!!action.disabled} title=${action.disabled || "Make a new model (N)"} onClick=${action.run}>${action.label}</button>
    ${(() => { const transfer = sendFilesAction(model); return html`<button type="button" class="ghost" id="send-model-files" disabled=${!!transfer.disabled} title=${transfer.disabled || transfer.label} onClick=${transfer.run}>${transfer.label}</button>`; })()}
    <button type="button" class="ghost" onClick=${clear}>Clear</button>
  </div>`;
}

export function ExtractDialog({ model, keys }) {
  const overview = useStore(ui, (s) => s.overview);
  const readOnly = useStore(ui, (s) => s.library?.read_only);
  const [name, setName] = useState("");
  const [schema, setSchema] = useState(null);
  const [values, setValues] = useState([]);
  const [keep, setKeep] = useState(true);
  const [mode, setMode] = useState("move");
  const [planState, setPlan] = useState(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useLayoutEffect(() => {
    setName(extractionName(keys)); setSchema(model.schema || null); setValues([...(model.path || [])]);
  }, [model, keys]);
  const selection = pickedFiles(keys);
  const args = { id: model.id, ...selection, name: name.trim(), schema, values };
  const signature = JSON.stringify(args);
  const plan = planState?.signature === signature ? planState.plan : null;
  const sc = overview?.schemas?.find((s) => s.id === schema);
  const where = sc ? [sc.name, ...values].join(" › ") : "Unsorted";
  const original = overview?.schemas?.find((s) => s.id === model.schema);
  const originalPlace = original ? [original.name, ...(model.path || [])].join(" › ") : "Unsorted";
  useLayoutEffect(() => {
    let live = true;
    setError("");
    if (!args.name) { setPlan(null); return; }
    const timer = setTimeout(() => api("model_extract_plan", args).then((p) => { if (live) setPlan({ signature, plan: p }); }, (e) => { if (live) setError(e.message || String(e)); }), 180);
    return () => { live = false; clearTimeout(timer); };
  }, [signature]);
  const submit = async () => {
    if (busy || !plan?.files?.length || plan.error || readOnly || !args.name) return;
    setBusy(true); setError("");
    try {
      const { job } = await api("model_extract", { ...args, mode, review: plan });
      const done = await followJob(job, `Making ${args.name} a new model`);
      if (done.error) throw new Error(done.error);
      const result = done.result;
      if (!result?.id) throw new Error("The new model was not made.");
      clear(); show("d:");
      await loadOverview();
      close();
      if (result.retired) location.hash = routeHash(`model:${result.id}`);
      recorded(`Made ${args.name} a new model in ${where}.`, result.journal, () => {
        clear(); show("d:");
        if (result.retired) location.hash = routeHash(`model:${model.id}`);
      }, [{ label: "Open", run: () => { location.hash = routeHash(`model:${result.id}`); } }]);
    } catch (e) { setError(e.message || String(e)); }
    finally { setBusy(false); }
  };
  const n = plan?.files?.length || 0;
  return html`<${Dialog} title="Make a new model" id="extract-dialog" onSubmit=${submit} busy=${busy || !n || !!plan?.error || !!readOnly} error=${error || plan?.error || (readOnly ? "The library is read-only." : "")} submitLabel=${`Make model (${n} ${n === 1 ? "file" : "files"})`}>
    <label class="field-block"><span>Name</span><input id="extract-name" required maxlength="200" value=${name} disabled=${busy} onInput=${(e) => setName(e.target.value)} /></label>
    <div class="field-block"><span class="field-label">Where</span>
      <label><input type="radio" name="extract-where" checked=${keep} disabled=${busy} onChange=${() => { setKeep(true); setSchema(model.schema || null); setValues([...(model.path || [])]); }} /> Keep in ${originalPlace}</label>
      <label><input type="radio" name="extract-where" checked=${!keep} disabled=${busy} onChange=${() => setKeep(false)} /> Choose another category</label>
      ${!keep ? html`<fieldset disabled=${busy}><${CategoryPicker} overview=${overview} schema=${schema} values=${values} idPrefix="extract" onChange=${(s, v) => { setSchema(s); setValues(v); }} /></fieldset>` : null}
    </div>
    <label class="field-block"><span>Move or copy</span><select id="extract-mode" value=${mode} disabled=${busy} onChange=${(e) => setMode(e.target.value)}><option value="move">Move — files leave this model</option><option value="copy">Copy — keep files in both models</option></select></label>
    ${selection.entries.length ? html`<p class="muted">Selected ZIP entries are unpacked into the new model. The ZIP stays unchanged.</p>` : null}
    <div class="field-block"><span class="field-label">Preview</span>
      ${plan ? html`<code class="preview-path" id="extract-destination">${plan.dest}/</code><ul class="extract-preview" id="extract-preview">${(plan.files || []).map((f) => html`<li key=${f.to}><code>${f.to}</code></li>`)}</ul>` : html`<p class="muted">${args.name ? "Checking the destination…" : "Enter a name to see the destination."}</p>`}
    </div>
  <//>`;
}
