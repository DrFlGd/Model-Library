// Compress to ZIP (a model, or its selected files) and Extract archive (a ZIP, 7z
// or RAR), reviewed: the commit must match the exact reviewed hashes and destinations.
import { html, useEffect, useState } from "../lib/html.js";
import { ui } from "./state.js";
import { api, followJob, loadOverview, recorded } from "./library.js";
import { size } from "./details.js";
import { Dialog, close } from "./dialogs.js";
import { useCheck } from "./checking.js";
import { isArchive } from "./filetypes.js";

const errorOf = (e) => e?.message || String(e);
const plural = (n, one) => `${n} ${n === 1 ? one : one + "s"}`;
export function ArchiveDialog({ request }) {
  const { id, action } = request;
  const files = request.files || null;
  const some = action === "compress" && !!files?.length;
  const [model, setModel] = useState(null);
  const [filename, setFilename] = useState("");
  // As in Make a model…: Move takes the originals out once the result is checked,
  // Copy keeps them. Either way it's one step, with Undo in the message.
  const [mode, setMode] = useState("move");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [done, setDone] = useState(null);
  const readOnly = !!ui.get().library?.read_only;
  useEffect(() => {
    let live = true;
    api("model_get", { id }).then((m) => {
      if (!live) return;
      setModel(m);
      const found = (m.files_list || []).filter((f) => isArchive(f.rel));
      const chosen = request.file && found.some((f) => f.rel === request.file) ? request.file : found[0]?.rel || "";
      setFilename(action === "extract" ? chosen : (some && request.name ? request.name : `${m.name || "Model"}.zip`).replace(/[\\/:*?"<>|]/g, "-").replace(/[. ]+$/, ""));
    }, (e) => { if (live) setError(errorOf(e)); });
    return () => { live = false; };
  }, [id, action]);
  const args = { id, action, file: filename, ...(some ? { files } : {}) };
  const check = useCheck("archive_plan", args, JSON.stringify([args, !!model]),
    { delay: 350, enabled: !done && !!model && !!filename && !busy });
  const archives = (model?.files_list || []).filter((f) => isArchive(f.rel));
  const plan = check.result;
  const compress = action === "compress";
  const name = filename.split("/").pop();
  const said = (journal, removed) => {
    close();
    recorded(compress ? (removed ? `Compressed ${plural(plan.files_count, "file")} into ${name}.` : `Made ${name}; the files are kept as well.`)
      : (removed ? `Extracted ${name} and removed it.` : `Extracted ${name}; it's kept as well.`), journal, () => loadOverview());
  };
  const submit = async () => {
    if (done) return said(done.journal, false);
    if (busy || !plan || readOnly) return;
    setBusy(true); setError("");
    let result = null;
    try {
      const { job } = await api("archive_execute", { ...args, reviewed_plan: plan });
      const ran = await followJob(job, compress ? "Compressing to ZIP" : "Extracting the archive");
      if (ran.error) throw new Error(ran.error);
      result = ran.result;
      if (!result?.verified) throw new Error("The result couldn't be checked, so nothing was removed.");
      if (mode === "move") {
        const { job: tidy } = await api("archive_cleanup", { journal: result.journal });
        const tidied = await followJob(tidy, compress ? "Removing the compressed files" : "Removing the archive");
        if (tidied.error) throw new Error(tidied.error);
      }
      await loadOverview();
      said(result.journal, mode === "move");
    } catch (e) {
      // The ZIP or the files are made and checked; only removing the originals failed.
      if (result?.verified) { setDone(result); await loadOverview(); setError(`${compress ? "The ZIP is made and checked" : "The files are extracted and checked"}, but ${compress ? "the files" : "the archive"} couldn't be removed: ${errorOf(e)}`); }
      else setError(errorOf(e));
    }
    finally { setBusy(false); }
  };
  return html`<${Dialog} id="archive-dialog" title=${compress ? (some ? "Compress to ZIP" : "Compress model to ZIP") : "Extract archive"} onSubmit=${submit} busy=${busy || (!done && !plan) || readOnly} error=${error || check.error || (readOnly ? "The library is read-only." : "")} submitLabel=${done ? "Close" : compress ? "Compress to ZIP" : "Extract files"}>
    <p class="muted">${compress ? (some ? `${files.length === 1 ? "The selected file or folder goes" : `The ${files.length} selected files and folders go`} into one ZIP in the model's folder, keeping their folders.${request.entries ? " Files picked inside an archive are left out." : ""}`
      : "The model's files go into one ZIP in its folder. model.json and the previews stay beside it.")
      : "The archive's files go into the model's folder, keeping their folders."} Files already there are never overwritten.</p>
    ${compress ? html`<label class="field-block"><span>ZIP name</span><input id="archive-name" required value=${filename} disabled=${busy || !!done} onInput=${(e) => setFilename(e.target.value)} /></label>` :
      html`<label class="field-block"><span>Archive</span><select id="archive-select" value=${filename} disabled=${busy || !!done} onChange=${(e) => setFilename(e.target.value)}>
        ${!archives.length ? html`<option value="">No archives</option>` : null}
        ${archives.map((f) => html`<option key=${f.rel} value=${f.rel}>${f.rel}</option>`)}
      </select></label>`}
    <label class="field-block"><span>Move or copy</span><select id="archive-mode" value=${mode} disabled=${busy || !!done} onChange=${(e) => setMode(e.target.value)}>
      ${compress ? html`<option value="move">Move — the files go into the ZIP</option><option value="copy">Copy — keep the files as well</option>`
        : html`<option value="move">Move — the archive is removed</option><option value="copy">Copy — keep the archive as well</option>`}
    </select></label>
    ${check.running && !check.view ? html`<p role="status" class="muted">Checking the files…</p>` : check.view}
    ${plan ? html`<div class="archive-plan" aria-label="What will be done">
      <p><strong>${plural(plan.files_count, "file")}</strong> · ${size(plan.bytes)}</p>
      <p class="muted">${compress ? (mode === "move" ? "The ZIP is read back and checked before the files are removed." : "The ZIP is read back and checked.")
        : (mode === "move" ? "The files are checked against the archive before it's removed." : "The files are checked against the archive.")}</p>
      <details><summary>The files (${plan.files_count})</summary><ul>
        ${plan.files.slice(0, 250).map((f) => html`<li key=${f.path}><code>${f.path}</code> <span class="muted">${size(f.size)}</span></li>`)}
        ${plan.files_count > 250 ? html`<li class="muted">… and ${plan.files_count - 250} more</li>` : null}
      </ul></details>
    </div>` : null}
  <//>`;
}
