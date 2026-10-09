// Model-level ZIP review. The backend revalidates the preview before writing.
import { html, useEffect, useState } from "../lib/html.js";
import { ui } from "./state.js";
import { api, followJob, loadOverview, recorded } from "./library.js";
import { size } from "./details.js";
import { Dialog, close } from "./dialogs.js";

const errorOf = (e) => e?.message || String(e);
export function ArchiveDialog({ request }) {
  const { id, action } = request;
  const [model, setModel] = useState(null);
  const [filename, setFilename] = useState("");
  const [remove, setRemove] = useState(false);
  const [preview, setPreview] = useState(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const readOnly = !!ui.get().library?.read_only;
  useEffect(() => {
    let live = true;
    api("model_get", { id }).then((m) => {
      if (!live) return;
      setModel(m);
      const zips = (m.files_list || []).filter((f) => /\.zip$/i.test(f.rel));
      const chosen = request.file && zips.some((f) => f.rel === request.file) ? request.file : zips[0]?.rel || "";
      setFilename(action === "extract" ? chosen : (m.name || "Model").replace(/[\\/:*?"<>|]/g, "-").replace(/[. ]+$/, "") + ".zip");
    }, (e) => { if (live) setError(errorOf(e)); });
    return () => { live = false; };
  }, [id, action]);
  useEffect(() => {
    setPreview(null);
    if (!model || !filename || busy) return;
    let live = true;
    const timer = setTimeout(() => {
      api("archive_plan", { id, action, file: filename }).then((plan) => {
        if (live) setPreview({ plan });
      }, (e) => { if (live) setPreview({ error: errorOf(e) }); });
    }, 350);
    return () => { live = false; clearTimeout(timer); };
  }, [id, action, filename, model, busy]);
  const archives = (model?.files_list || []).filter((f) => /\.zip$/i.test(f.rel));
  const plan = preview?.plan;
  const submit = async () => {
    if (busy || !plan || readOnly) return;
    setBusy(true); setError("");
    try {
      const { job } = await api("archive_execute", { id, action, file: filename, remove_sources: remove });
      const done = await followJob(job, action === "compress" ? "Compressing ZIP" : "Extracting ZIP");
      if (done.error) throw new Error(done.error + " If any outputs were published, Home → Recent changes contains the recovery journal.");
      const result = done.result;
      if (!result?.verified) throw new Error("Verification did not finish. Source files were kept.");
      await loadOverview();
      close();
      recorded(action === "compress" ? "Created a verified ZIP." : "Extracted the ZIP after checking its contents.", result.journal, () => loadOverview());
    } catch (e) { setError(errorOf(e)); }
    finally { setBusy(false); }
  };
  return html`<${Dialog id="archive-dialog" title=${{action === "compress" ? "Compress model to ZIP" : "Extract archive into model"} onSubmit=${{submit} busy=${{busy || !plan || readOnly} error=${{error || preview?.error || (readOnly ? "The library is read-only." : "")} submitLabel=${{action === "compress" ? "Compress to ZIP" : "Extract files"}>
    <p class="muted">This changes files in the existing model. Its identity, category and model.json stay in place. Existing files are never overwritten.</p>
    ${{action === "compress" ? html`<label class="field-block"><span>ZIP file name (in the model's folder)</span><input id="archive-name" required value=${{filename} disabled=${{busy} onInput=${{(e) => setFilename(e.target.value)} /></label>` :
      html`<label class="field-block"><span>Archive to extract</span><select id="archive-select" value=${{filename} disabled=${{busy} onChange=${{(e) => setFilename(e.target.value)}>
        ${{!archives.length ? html`<option value="">No ZIP files</option>` : null}
        ${{archives.map((f) => html`<option key=${{f.rel} value=${{f.rel}>${{f.rel}</option>`)}
      </select></label>`}
    ${{!preview && filename ? html`<p role="status" class="muted">Checking files, destinations and hashes…</p>` : null}
    ${{plan ? html`<div class="archive-plan" aria-label="ZIP operation preview">
      <p><strong>${{plan.files_count} files</strong> · ${{size(plan.bytes)} of content · approximately ${{size(plan.required_bytes)} of staging space required.</p>
      <p class="muted">${{action === "compress" ? "Output ZIP:" : "Source archive:"} <code>${{plan.file}</code></p>
      <p class="muted">${{plan.metadata}</p>
      <details><summary>Review file paths (${{plan.files_count})</summary><ul>
        ${{plan.files.map((f) => html`<li key=${{f.path}><code>${{f.path}</code> <span class="muted">${{size(f.size)}</span></li>`)}
      </ul></details>
      <label class="archive-cleanup"><input type="checkbox" checked=${{remove} disabled=${{busy} onChange=${{(e) => setRemove(e.target.checked)} />
      ${{action === "compress" ? "Remove original content only after the ZIP passes hash verification" : "Remove source ZIP only after extracted files pass hash verification"}</label>
      <p class="muted">By default, originals remain. Optional removal keeps exact sources in journal recovery for Undo. Changed files are never removed.</p>
    </div>` : null}
  </${Dialog>`;
}
