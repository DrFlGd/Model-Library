// An in-place editor for the model workspace's right Details ribbon. As in the
// library's details panel, leaving a box (or Enter in a one-line box) saves it,
// with Undo in the message; Esc puts back what hasn't been saved.
import { html, useState, useEffect, useLayoutEffect, useRef } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui } from "./state.js";
import { api, saveDetails, recorded } from "./library.js";
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

/** Only values editable in this panel participate in conflict detection. */
export const detailsSnapshot = (model) => ({
  form: detailsDraft(model),
  fields: { ...(model.fields || {}) },
});

/** Key-order-insensitive comparison (schema fields can be returned in a new order). */
export function sameDetails(a, b) {
  if (Object.is(a, b)) return true;
  if (a === null || b === null || typeof a !== "object" || typeof b !== "object") return false;
  const ak = Object.keys(a), bk = Object.keys(b);
  return ak.length === bk.length && ak.every((key) => Object.hasOwn(b, key) && sameDetails(a[key], b[key]));
}

export const hasDetailEdits = (draft, baseline) =>
  !sameDetails(draft.form, baseline.form) || !sameDetails(draft.fields, baseline.fields);

/** Overlay only locally changed fields on fresh data; leave independent external edits alone. */
export function rebaseEditedFields(baseline, edited, latest) {
  const result = { ...latest };
  for (const key of new Set([...Object.keys(baseline), ...Object.keys(edited)])) {
    if (sameDetails(baseline[key], edited[key])) continue;
    if (Object.hasOwn(edited, key)) result[key] = edited[key];
    else delete result[key];
  }
  return result;
}

export function changedBoth(baseline, edited, latest) {
  return [...new Set([...Object.keys(baseline), ...Object.keys(edited), ...Object.keys(latest)])]
    .filter((key) => !sameDetails(baseline[key], edited[key]) &&
      !sameDetails(baseline[key], latest[key]) && !sameDetails(edited[key], latest[key]));
}

export function WorkspaceDetails({ model, onModelChange }) {
  const overview = useStore(ui, (s) => s.overview);
  const readOnly = useStore(ui, (s) => !!s.library?.read_only);
  const schema = overview?.schemas?.find((s) => s.id === model.schema);
  const latest = detailsSnapshot(model);
  // The baseline is the last *accepted* model snapshot, not the newest props.
  // An external refresh must never turn a previously clean form into a draft.
  const [editor, setEditor] = useState(() => ({
    ...latest, baseline: latest, modelId: model.id,
  }));
  const { form, fields, baseline } = editor;
  const dirty = hasDetailEdits(editor, baseline);
  const concurrent = dirty && !sameDetails(baseline, latest);
  const overlaps = concurrent ? [
    ...changedBoth(baseline.form, form, latest.form),
    ...changedBoth(baseline.fields, fields, latest.fields).map((key) => `field: ${key}`),
  ] : [];
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const errorRef = useRef(null);
  // save() runs from focus and key events, after the render that took the
  // last keystroke; it reads the newest state from here.
  const now = useRef(null);
  now.current = { editor, latest, dirty, concurrent, busy };

  useLayoutEffect(() => {
    setEditor((prior) => {
      if (prior.modelId === model.id && hasDetailEdits(prior, prior.baseline)) return prior;
      // No local changes: always accept the newest model, including dialog edits,
      // external watcher refreshes, and Undo.
      const incoming = detailsSnapshot(model);
      return { ...incoming, baseline: incoming, modelId: model.id };
    });
  }, [model.id, model.name, model.authors, model.tags, model.details, model.fields]);

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

  const change = (key) => (event) => {
    const value = event.target.value;
    setError("");
    setEditor((s) => ({ ...s, form: { ...s.form, [key]: value } }));
  };
  const updateField = (key, value) => {
    setError("");
    setEditor((s) => ({ ...s, fields: { ...s.fields, [key]: value } }));
  };
  const discard = () => {
    const incoming = detailsSnapshot(model);
    setEditor({ ...incoming, baseline: incoming, modelId: model.id });
    setError("");
  };
  const keepEdits = () => {
    // This explicit action resolves remote changes. Only *our* changed values
    // overwrite new values; untouched values are always taken from latest.
    setEditor({
      form: rebaseEditedFields(baseline.form, form, latest.form),
      fields: rebaseEditedFields(baseline.fields, fields, latest.fields),
      baseline: latest, modelId: model.id,
    });
    setError("");
    setTimeout(save);
  };

  const save = async () => {
    const { editor: draft, dirty, concurrent, busy } = now.current;
    if (busy || readOnly || !dirty || concurrent) return;
    const { form, fields, baseline } = draft;
    setBusy(true); setError("");
    try {
      // Check once more immediately before writing. A newer watcher response
      // must not silently cause a stale full-details patch to overwrite it.
      const fresh = await api("model_get", { id: model.id });
      if (!sameDetails(detailsSnapshot(fresh), baseline)) {
        onModelChange?.(fresh);
        setError("The model changed before your change was saved. Choose which details to keep below.");
        return;
      }
      const patch = { ...form, fields: { ...(fresh.fields || {}), ...fields } };
      const result = await saveDetails(fresh, patch);
      // model_update returns an index entry, not the full model_get payload
      // (which also contains details, files_list and main). Preserve those
      // workspace properties immediately after Save instead of replacing
      // the page's model with a partial object.
      const full = await api("model_get", { id: result.id });
      const saved = detailsSnapshot(full);
      // Keep anything typed while it saved, on top of what was saved.
      setEditor((s) => ({
        form: rebaseEditedFields(form, s.form, saved.form),
        fields: rebaseEditedFields(fields, s.fields, saved.fields),
        baseline: saved, modelId: full.id,
      }));
      // Saving can give a legacy model its first persistent ID. Clear the
      // navigation guard before the existing follow-IDs route replacement.
      ui.set({ workspaceDirty: false });
      onModelChange?.(full);
      recorded(`Saved ${result.name}'s details.`, result.journal);
    } catch (e) {
      setError(e.message || String(e));
      requestAnimationFrame(() => errorRef.current?.focus());
    } finally {
      setBusy(false);
    }
  };
  const images = (model.files_list || []).filter((f) => f.kind === "image");
  const field = (t) => ["INPUT", "SELECT", "TEXTAREA"].includes(t?.tagName);
  const leave = (e) => { if (field(e.target)) setTimeout(save); };
  const key = (e) => {
    if (!field(e.target)) return;
    if (e.key === "Escape" && now.current.dirty) { e.preventDefault(); e.stopPropagation(); discard(); }
    else if (e.key === "Enter" && e.target.tagName === "INPUT") { e.preventDefault(); setTimeout(save); }
  };
  return html`<form class="workspace-details-form" id="workspace-details-form" onSubmit=${(e) => { e.preventDefault(); setTimeout(save); }} onfocusout=${leave} onKeyDown=${key}
    aria-label="Edit model details" data-dirty=${dirty ? "true" : "false"} data-blocked=${concurrent ? "true" : "false"} aria-busy=${busy ? "true" : "false"}>
    <${Cover} model=${model} cls="insp-thumb" />
    <p class="insp-sub"><${PlaceLinks} m=${model} overview=${overview} /></p>
    <p class="muted details-save-note">${readOnly ? "This library is read-only." : "Changes are saved when you leave a box. Esc puts it back."}</p>
    <label class="field-block"><span>Name</span><input id="workspace-edit-name" required maxlength="200" type="text" value=${form.name} disabled=${readOnly} onInput=${change("name")} /></label>
    <label class="field-block"><span>Authors</span><input id="workspace-edit-authors" type="text" placeholder="Separate several with commas" value=${form.authors} disabled=${readOnly} onInput=${change("authors")} /></label>
    <label class="field-block"><span>Released</span><input type="date" value=${form.released} disabled=${readOnly} onInput=${change("released")} /></label>
    <label class="field-block"><span>Licence</span><input type="text" value=${form.license} disabled=${readOnly} onInput=${change("license")} /></label>
    <label class="field-block"><span>Source</span><input type="url" placeholder="https://…" value=${form.source} disabled=${readOnly} onInput=${change("source")} /></label>
    <label class="field-block"><span>Tags</span><input id="workspace-edit-tags" type="text" placeholder="tag one, tag two" value=${form.tags} disabled=${readOnly} onInput=${change("tags")} /></label>
    ${(schema?.fields || []).map((field) => html`<label class="field-block" key=${field.key}><span>${field.label}</span>
      <${FieldInput} field=${field} value=${fields[field.key]} onChange=${(value) => updateField(field.key, value)} /></label>`)}
    ${images.length ? html`<label class="field-block"><span>Cover picture</span><select value=${form.cover} disabled=${readOnly} onChange=${change("cover")}>
      <option value="">Chosen for me</option>${images.map((f) => html`<option key=${f.rel} value=${f.rel}>${f.rel}</option>`)}</select></label>` : null}
    <label class="field-block"><span>Notes</span><textarea id="workspace-edit-notes" rows="4" value=${form.notes} disabled=${readOnly} onInput=${change("notes")}></textarea></label>
    ${concurrent ? html`<div class="details-conflict" role="alert" id="details-concurrent">
      <strong>Model details changed elsewhere.</strong>
      <p>Your change isn't saved yet. Use the latest details, or keep your edits on top of them (that saves them).</p>
      ${overlaps.length ? html`<p><strong>Both versions changed:</strong> ${overlaps.join(", ")}. Keeping yours will overwrite these particular values.</p>` : null}
      <div class="details-conflict-actions">
        <button type="button" class="ghost" id="details-load-latest" disabled=${busy} onClick=${discard}>Use latest details</button>
        <button type="button" class="ghost" id="details-keep-draft" disabled=${busy} onClick=${keepEdits}>Keep my edits</button>
      </div>
    </div>` : null}
    ${error ? html`<p class="form-error" role="alert" tabindex="-1" ref=${errorRef}>${error}</p>` : null}
  </form>`;
}
