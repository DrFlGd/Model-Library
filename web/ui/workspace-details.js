// An in-place editor for the model workspace's right Details ribbon.
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
  };

  const submit = async (event) => {
    event.preventDefault();
    if (busy || readOnly || !dirty || concurrent) return;
    setBusy(true); setError("");
    try {
      // Check once more immediately before writing. A newer watcher response
      // must not silently cause a stale full-details patch to overwrite it.
      const fresh = await api("model_get", { id: model.id });
      if (!sameDetails(detailsSnapshot(fresh), baseline)) {
        onModelChange?.(fresh);
        setError("The model changed before Save. Review the new values before trying again.");
        return;
      }
      const patch = { ...form, fields: { ...(fresh.fields || {}), ...fields } };
      const result = await saveDetails(fresh, patch);
      const saved = detailsSnapshot(result);
      setEditor({ ...saved, baseline: saved, modelId: result.id });
      onModelChange?.(result);
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
      <${FieldInput} field=${field} value=${fields[field.key]} onChange=${(value) => updateField(field.key, value)} /></label>`)}
    ${images.length ? html`<label class="field-block"><span>Cover picture</span><select value=${form.cover} disabled=${readOnly || busy} onChange=${change("cover")}>
      <option value="">Chosen for me</option>${images.map((f) => html`<option key=${f.rel} value=${f.rel}>${f.rel}</option>`)}</select></label>` : null}
    <label class="field-block"><span>Notes</span><textarea rows="4" value=${form.notes} disabled=${readOnly || busy} onInput=${change("notes")}></textarea></label>
    ${concurrent ? html`<div class="details-conflict" role="alert" id="details-concurrent">
      <strong>Model details changed elsewhere.</strong>
      <p>Your unsaved draft is preserved. Reload the latest details, or keep your edits on top of the latest values.</p>
      ${overlaps.length ? html`<p><strong>Both versions changed:</strong> ${overlaps.join(", ")}. Keeping yours will overwrite these particular values.</p>` : null}
      <div class="details-conflict-actions">
        <button type="button" class="ghost" id="details-load-latest" disabled=${busy} onClick=${discard}>Use latest details</button>
        <button type="button" class="ghost" id="details-keep-draft" disabled=${busy} onClick=${keepEdits}>Keep my edits</button>
      </div>
    </div>` : null}
    ${error ? html`<p class="form-error" role="alert" tabindex="-1" ref=${errorRef}>${error}</p>` : null}
    <div class="workspace-details-actions">
      <button type="button" class="ghost" disabled=${busy || !dirty} onClick=${discard}>Cancel changes</button>
      <button type="submit" class="primary" disabled=${busy || readOnly || !dirty || concurrent}>${busy ? "Saving…" : "Save"}</button>
    </div>
    ${readOnly ? html`<p class="muted">This library is read-only.</p>` : null}
  </form>`;
}
