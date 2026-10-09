// An in-place editor for the model workspace's right Details ribbon.
import { html, useState, useEffect, useRef } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui } from "./state.js";
import { saveDetails, recorded } from "./library.js";
import { Cover, PlaceLinks } from "./details.js";
import { FieldInput } from "./dialogs.js";

export function detailsDraft(model) {
  const d = model.details || {};
  return {
    name: model.name || "",
    authors: (model.authors || []).join(", "),
    released: d.released || "",
    source: d.source?.url || (typeof d.source === "string" ? d.source : ""),
    license: d.license || "",
    tags: (model.tags || []).join(", "),
    notes: d.notes || "",
    cover: d.cover || "",
  };
}

export function WorkspaceDetails({ model }) {
  const overview = useStore(ui, (s) => s.overview);
  const readOnly = useStore(ui, (s) => !!s.library?.read_only);
  const schema = overview?.schemas?.find((s) => s.id === model.schema);
  const seed = detailsDraft(model);
  const [form, setForm] = useState(() => seed);
  const [fields, setFields] = useState(() => ({ ...(model.fields || {}) }));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const errorRef = useRef(null);
  const dirty = JSON.stringify(form) !== JSON.stringify(seed) ||
    JSON.stringify(fields) !== JSON.stringify(model.fields || {});
  useEffect(() => {
    if (!dirty) {
      setForm(seed);
      setFields({ ...(model.fields || {}) });
    }
  }, [model.id, model.name, model.details, model.fields]);
  useEffect(() => {
    ui.set({ workspaceDirty: dirty });
    return () => ui.set({ workspaceDirty: false });
  }, [dirty, model.id]);
  useEffect(() => {
    if (!dirty) return;
    const warn = (event) => { event.preventDefault(); event.returnValue = ""; };
    window.addEventListener("beforeunload", warn);
    return () => window.removeEventListener("beforeunload", warn);
  }, [dirty]);
  const change = (key) => (event) => { setError(""); setForm((f) => ({ ...f, [key]: event.target.value })); };
  const discard = () => { setForm(detailsDraft(model)); setFields({ ...(model.fields || {}) }); setError(""); };
  const submit = async (event) => {
    event.preventDefault();
    if (busy || readOnly || !dirty) return;
    setBusy(true); setError("");
    try {
      const patch = { ...form, fields: { ...(model.fields || {}), ...fields } };
      const result = await saveDetails(model, patch);
      setForm(detailsDraft(result));
      setFields({ ...(result.fields || {}) });
      recorded(`Saved ${result.name}'s details.`, result.journal);
    } catch (e) {
      setError(e.message || String(e));
      requestAnimationFrame(() => errorRef.current?.focus());
    } finally {
      setBusy(false);
    }
  };
  const images = (model.files_list || []).filter((f) => f.kind === "image");
  return html`<form class="workspace-details-form" id="workspace-details-form" onSubmit=${submit} aria-label="Edit model details">
    <${Cover} model=${model} cls="insp-thumb" />
    <p class="insp-sub"><${PlaceLinks} m=${model} overview=${overview} /></p>
    <p class="muted details-save-note">Changes are saved only when you choose Save; closing Details keeps your draft.</p>
    <label class="field-block"><span>Name</span><input id="workspace-edit-name" required maxlength="200" type="text" value=${form.name} disabled=${readOnly || busy} onInput=${change("name")} /></label>
    <label class="field-block"><span>Authors</span><input id="workspace-edit-authors" type="text" placeholder="Separate several with commas" value=${form.authors} disabled=${readOnly || busy} onInput=${change("authors")} /></label>
    <label class="field-block"><span>Released</span><input type="date" value=${form.released} disabled=${readOnly || busy} onInput=${change("released")} /></label>
    <label class="field-block"><span>Licence</span><input type="text" value=${form.license} disabled=${readOnly || busy} onInput=${change("license")} /></label>
    <label class="field-block"><span>Source</span><input type="url" placeholder="https://…" value=${form.source} disabled=${readOnly || busy} onInput=${change("source")} /></label>
    <label class="field-block"><span>Tags</span><input type="text" placeholder="tag one, tag two" value=${form.tags} disabled=${readOnly || busy} onInput=${change("tags")} /></label>
    ${(schema?.fields || []).map((field) => html`<label class="field-block" key=${field.key}><span>${field.label}</span>
      <${FieldInput} field=${field} value=${fields[field.key]} onChange=${(value) => setFields((was) => ({ ...was, [field.key]: value }))} /></label>`)}
    ${images.length ? html`<label class="field-block"><span>Cover picture</span><select value=${form.cover} disabled=${readOnly || busy} onChange=${change("cover")}>
      <option value="">Chosen for me</option>${images.map((f) => html`<option key=${f.rel} value=${f.rel}>${f.rel}</option>`)}</select></label>` : null}
    <label class="field-block"><span>Notes</span><textarea rows="4" value=${form.notes} disabled=${readOnly || busy} onInput=${change("notes")}></textarea></label>
    ${error ? html`<p class="form-error" role="alert" tabindex="-1" ref=${errorRef}>${error}</p>` : null}
    <div class="workspace-details-actions">
      <button type="button" class="ghost" disabled=${busy || !dirty} onClick=${discard}>Cancel changes</button>
      <button type="submit" class="primary" disabled=${busy || readOnly || !dirty}>${busy ? "Saving…" : "Save"}</button>
    </div>
    ${readOnly ? html`<p class="muted">This library is read-only.</p>` : null}
  </form>`;
}
