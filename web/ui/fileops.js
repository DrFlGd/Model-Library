// File transfer, add and recoverable model deletion review dialogs.
import { html, useState, useEffect } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui } from "./state.js";
import { ctx, routeHash } from "./context.js";
import { api, followJob, loadOverview, recorded, toast } from "./library.js";
import { writable } from "./actions.js";
import { Dialog, close } from "./dialogs.js";
import { fileSel, pickedFiles, clear, show } from "./filesel.js";
import { size } from "./details.js";

const message = (e) => e?.message || String(e);
function modelFolders(model) {
  const found = new Set([""]);
  for (const f of model?.files_list || []) {
    const bits = f.rel.split("/");
    let at = "";
    for (const bit of bits.slice(0, -1)) { at = at ? `${at}/${bit}` : bit; found.add(at); }
  }
  return [...found].sort((a, b) => a.localeCompare(b));
}
function Preview({ plan }) {
  return html`<div class="field-block"><span class="field-label">Review</span>
    ${plan ? html`<p class="muted">${plan.count} ${plan.count === 1 ? "file" : "files"} · ${size(plan.bytes || 0)}. ${plan.cleanup} Undo is available from Recent changes.</p>
      <ul class="extract-preview" aria-label="Target paths">${(plan.files || []).slice(0, 100).map((f, i) => html`<li key=${i}><code>${f.to}</code> <span class="muted">(${size(f.bytes || 0)})</span></li>`)}</ul>
      ${plan.files.length > 100 ? html`<p class="muted">${plan.files.length - 100} additional paths</p>` : null}
      ${plan.conflicts?.length ? html`<p class="warn-note" role="status">${plan.conflicts.length} destination name conflicts. Choose Keep both, Skip or Cancel.</p>` : null}`
    : html`<p class="muted">Choose a destination to see the planned paths, file count and conflicts.</p>`}
  </div>`;
}
export async function addFiles(model) {
  if (writable() !== true) return toast(writable());
  const pick = ctx.platform.library?.pickFiles;
  if (!pick) return toast("Adding files requires the desktop file picker.");
  try {
    const sources = await pick("Choose files to add to this model");
    if (sources?.length) {
      const fresh = await api("model_get", { id: model.id });
      ui.set({ dialog: { type: "add-model-files", model: fresh, sources } });
    }
  } catch (e) { toast("Couldn't choose files: " + message(e), 6000); }
}
export function openSendFiles(model, keys = fileSel.get().picked) {
  if (writable() !== true) return toast(writable());
  if (!keys.length) return toast("Select some files or folders first.");
  ui.set({ dialog: { type: "send-model-files", model, keys: [...keys] } });
}
export function sendFilesAction(model) {
  const selected = fileSel.get().picked;
  return { id: "send-model-files", label: "Send to another model…", icon: "move",
    disabled: writable() !== true ? writable() : !selected.length ? "Select files first." : false,
    run: () => openSendFiles(model) };
}
function ConflictChoice({ value, setValue, busy, id }) {
  return html`<label class="field-block"><span>Name conflicts</span><select id=${id} value=${value} disabled=${busy} onChange=${(e) => setValue(e.target.value)}>
    <option value="cancel">Cancel conflicting operation</option><option value="keep_both">Keep both (rename additions)</option><option value="skip">Skip conflicting files</option>
  </select></label>`;
}
export function SendFilesDialog({ model, keys }) {
  const [q, setQ] = useState("");
  const [results, setResults] = useState([]);
  const [target, setTarget] = useState("");
  const [targetModel, setTargetModel] = useState(null);
  const [folder, setFolder] = useState("");
  const [mode, setMode] = useState("move");
  const [conflict, setConflict] = useState("cancel");
  const [review, setReview] = useState(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const readOnly = useStore(ui, (s) => s.library?.read_only);
  const selected = pickedFiles(keys);
  useEffect(() => {
    let live = true;
    const t = setTimeout(() => api("models_query", { scope: "all", q, sort: "name", limit: 2000 })
      .then((r) => { if (live) setResults((r.items || []).filter((m) => m.id !== model.id)); },
        (e) => { if (live) setError(message(e)); }), 180);
    return () => { live = false; clearTimeout(t); };
  }, [q, model.id]);
  useEffect(() => {
    let live = true;
    setTargetModel(null); setFolder("");
    if (target) api("model_get", { id: target }).then((v) => { if (live) setTargetModel(v); }, (e) => { if (live) setError(message(e)); });
    return () => { live = false; };
  }, [target]);
  const action = { kind: "send", id: model.id, target, ...selected, folder, mode, conflict };
  const signature = JSON.stringify(action);
  const plan = review?.signature === signature ? review.plan : null;
  useEffect(() => {
    let live = true; setError("");
    if (!targetModel || targetModel.id !== target) { setReview(null); return; }
    const t = setTimeout(() => api("model_files_plan", action).then((p) => {
      if (live) setReview({ signature, plan: p });
    }, (e) => { if (live) { setReview(null); setError(message(e)); } }), 180);
    return () => { live = false; clearTimeout(t); };
  }, [signature, targetModel?.id]);
  const submit = async () => {
    if (!plan?.files?.length || busy || readOnly || (conflict === "cancel" && plan.conflicts?.length)) return;
    setBusy(true); setError("");
    try {
      const { job } = await api("model_files_apply", { action, review: plan });
      const done = await followJob(job, "Sending files");
      if (done.error) throw new Error(done.error);
      const result = done.result;
      clear(); show("d:"); await loadOverview(); close();
      recorded(`Sent ${result.count} ${result.count === 1 ? "file" : "files"} to ${targetModel.name}.`, result.journal);
    } catch (e) { setError(message(e)); } finally { setBusy(false); }
  };
  return html`<${Dialog} title="Send to another model" id="send-files-dialog" onSubmit=${submit}
    busy=${busy || readOnly || !plan?.files?.length || (conflict === "cancel" && !!plan?.conflicts?.length)}
    error=${error || (readOnly ? "The library is read-only." : "")} submitLabel=${mode === "move" ? "Move files" : "Copy files"}>
    <p class="muted">From ${model.name}. ${selected.files.length} selected files/folders and ${selected.entries.length} ZIP entries; folders include their descendants. Destination model identity and details remain unchanged.</p>
    <label class="field-block"><span>Find a destination model</span><input id="send-search" type="search" placeholder="Search models" value=${q} disabled=${busy} onInput=${(e) => setQ(e.target.value)} /></label>
    <label class="field-block"><span>Destination</span><select id="send-target" value=${target} disabled=${busy} required onChange=${(e) => setTarget(e.target.value)}>
      <option value="">Choose another model…</option>${results.map((m) => html`<option key=${m.id} value=${m.id}>${m.name} · ${m.schema ? (m.path || []).join(" › ") || "Category" : "Unsorted"}</option>`)}
      ${target && !results.some((m) => m.id === target) && targetModel ? html`<option value=${target}>${targetModel.name}</option>` : null}
    </select></label>
    ${targetModel ? html`<label class="field-block"><span>Folder in destination</span><select id="send-folder" value=${folder} disabled=${busy} onChange=${(e) => setFolder(e.target.value)}>
      ${modelFolders(targetModel).map((f) => html`<option key=${f} value=${f}>${f || "Model root"}</option>`)}</select></label>` : null}
    <label class="field-block"><span>Action</span><select id="send-mode" value=${mode} disabled=${busy} onChange=${(e) => setMode(e.target.value)}>
      <option value="move">Move — remove verified source files</option><option value="copy">Copy — keep source files</option>
    </select></label>
    <${ConflictChoice} id="send-conflict" value=${conflict} setValue=${setConflict} busy=${busy} />
    ${selected.entries.length ? html`<p class="muted">ZIP entries can be copied but not moved. Choose Copy to unpack entries without changing the archive.</p>` : null}
    <${Preview} plan=${plan} />
  <//>`;
}
export function AddFilesDialog({ model, sources }) {
  const [folder, setFolder] = useState("");
  const [conflict, setConflict] = useState("cancel");
  const [review, setReview] = useState(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const readOnly = useStore(ui, (s) => s.library?.read_only);
  const action = { kind: "add", target: model.id, sources, folder, conflict };
  const signature = JSON.stringify(action);
  const plan = review?.signature === signature ? review.plan : null;
  useEffect(() => {
    let live = true; setError("");
    const t = setTimeout(() => api("model_files_plan", action).then((p) => {
      if (live) setReview({ signature, plan: p });
    }, (e) => { if (live) { setReview(null); setError(message(e)); } }), 180);
    return () => { live = false; clearTimeout(t); };
  }, [signature]);
  const submit = async () => {
    if (busy || !plan?.files?.length || (conflict === "cancel" && plan.conflicts?.length)) return;
    setBusy(true); setError("");
    try {
      const { job } = await api("model_files_apply", { action, review: plan });
      const done = await followJob(job, "Adding files");
      if (done.error) throw new Error(done.error);
      const result = done.result;
      await loadOverview(); close();
      recorded(`Added ${result.count} ${result.count === 1 ? "file" : "files"} to ${model.name}. The originals were kept.`, result.journal);
    } catch (e) { setError(message(e)); } finally { setBusy(false); }
  };
  return html`<${Dialog} title="Add files" id="add-files-dialog" onSubmit=${submit}
    busy=${busy || readOnly || !plan?.files?.length || (conflict === "cancel" && !!plan?.conflicts?.length)}
    error=${error || (readOnly ? "The library is read-only." : "")} submitLabel="Add files">
    <p class="muted">Add to ${model.name}, not as new models. Images, videos, PDFs, archives and unknown extensions are accepted. External originals are kept.</p>
    <ul class="extract-preview" aria-label="Selected external files">${sources.map((p,i) => html`<li key=${i}><code>${p.replace(/\\/g,"/").split("/").pop()}</code></li>`)}</ul>
    <label class="field-block"><span>Folder in this model</span><select id="add-files-folder" value=${folder} disabled=${busy} onChange=${(e) => setFolder(e.target.value)}>
      ${modelFolders(model).map((f) => html`<option value=${f} key=${f}>${f || "Model root"}</option>`)}</select></label>
    <${ConflictChoice} id="add-files-conflict" value=${conflict} setValue=${setConflict} busy=${busy} />
    <${Preview} plan=${plan} />
  <//>`;
}
export function DeleteModelsDialog({ models }) {
  const [review, setReview] = useState(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const ids = models.map((m) => m.id);
  const readOnly = useStore(ui, (s) => s.library?.read_only);
  useEffect(() => {
    let live = true;
    api("models_delete_plan", { ids }).then((p) => { if (live) setReview(p); }, (e) => { if (live) setError(message(e)); });
    return () => { live = false; };
  }, [ids.join("|")]);
  const submit = async () => {
    if (busy || !review || readOnly) return;
    setBusy(true); setError("");
    try {
      const { job } = await api("models_delete", { action: { ids }, review });
      const done = await followJob(job, "Deleting models");
      if (done.error) throw new Error(done.error);
      const result = done.result;
      ui.set({ picked: [], selection: null, anchor: null });
      if (ui.get().route.startsWith("model:")) location.hash = routeHash("browse:all");
      await loadOverview(); close();
      recorded(`Deleted ${result.deleted} ${result.deleted === 1 ? "model" : "models"} into recovery storage.`, result.journal);
    } catch (e) { setError(message(e)); } finally { setBusy(false); }
  };
  return html`<${Dialog} title="Delete models" id="delete-models-dialog" danger=${true} onSubmit=${submit}
    busy=${busy || !review || readOnly} error=${error || (readOnly ? "The library is read-only." : "")}
    submitLabel=${`Delete ${models.length} ${models.length === 1 ? "model" : "models"}`}>
    <p>Remove ${models.length} ${models.length === 1 ? "model" : "models"} from the active library, including their files, details and previews?</p>
    <ul class="extract-preview" aria-label="Models to delete">${(review?.models || models).map((m) => html`<li key=${m.id}>${m.name} ${m.count != null ? html`<span class="muted">(${m.count} files · ${size(m.bytes || 0)})</span>` : null}</li>`)}</ul>
    <p class="muted">Complete model folders are moved into application-managed recovery storage with journalled Undo. Shared assets outside the models are untouched. Categories remain, even if empty. Recovery data has no automatic expiry.</p>
  <//>`;
}
