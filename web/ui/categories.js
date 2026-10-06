// Changing categories after the fact (docs/PLAN.md, "Phase 4 design"): renaming,
// merging or moving a category value, editing a category (schema), deleting one,
// and editing several models' details at once. Every change that moves folders
// shows a preview from the core first, then runs as a job that can be undone
// from Home.
import { html, useState, useEffect } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui } from "./state.js";
import { routeHash, schemaScope } from "./context.js";
import { Icon } from "./icons.js";
import { api, planChange, runChange, valuesAt, loadOverview, toast } from "./library.js";
import { Dialog, close, FieldInput, FIELD_TYPES } from "./dialogs.js";

/** What a change would move, from the core, refreshed as the form changes. */
function ChangePreview({ change, onPlan }) {
  const [plan, setPlan] = useState(null);
  const [error, setError] = useState("");
  const key = JSON.stringify(change);
  useEffect(() => {
    if (!change) { setPlan(null); setError(""); onPlan(null); return; }
    let live = true;
    const t = setTimeout(() => {
      planChange(change).then((p) => { if (live) { setPlan(p); setError(""); onPlan(p); } },
        (e) => { if (live) { setPlan(null); setError(e.message || String(e)); onPlan(null); } });
    }, 200);
    return () => { live = false; clearTimeout(t); };
  }, [key]);
  if (error) return html`<p class="warn-note" id="change-preview" role="status">${error}</p>`;
  if (!plan) return html`<p class="muted" id="change-preview">Working out what moves…</p>`;
  return html`<div class="change-preview" id="change-preview" data-moving=${plan.moving}>
    <p><b>${plan.moving ? `${plan.moving} ${plan.moving === 1 ? "model folder moves" : "model folders move"}` : "No folders move"}</b>${plan.models > plan.moving ? `, ${plan.models - plan.moving} stay where they are` : ""}.${" "}
      ${plan.clashes ? html` <span class="warn-text">${plan.clashes} ${plan.clashes === 1 ? "lands" : "land"} on a folder that's already there and ${plan.clashes === 1 ? "gets" : "get"} a number, such as "(2)".</span>` : null}
      You can undo it from Home.</p>
    ${plan.sample.length ? html`<ul class="move-sample">${plan.sample.map((m) => html`<li key=${m.from}><span class="preview-path">${m.from}</span><span class="muted"> → </span><span class="preview-path">${m.to}</span></li>`)}
      ${plan.moving > plan.sample.length ? html`<li class="muted">and ${plan.moving - plan.sample.length} more</li>` : null}</ul>` : null}
  </div>`;
}

async function run(change, setBusy, setError, after) {
  setBusy(true);
  setError("");
  try {
    const r = await runChange(change);
    close();
    after?.(r);
  } catch (e) {
    setError(e.message || String(e));
  } finally {
    setBusy(false);
  }
}

/** Rename, merge or move one category value (and everything under it). */
export function RenameNode({ schemaId, path }) {
  const overview = useStore(ui, (s) => s.overview);
  const sc = overview?.schemas?.find((s) => s.id === schemaId);
  const [values, setValues] = useState([...path]);
  const [plan, setPlan] = useState(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  if (!sc) return null;
  const levels = sc.levels.slice(0, path.length);
  const last = levels[levels.length - 1];
  const changed = values.some((v, i) => v.trim() !== path[i]) && values.every((v) => v.trim());
  const change = changed ? { kind: "category", schema: schemaId, from: path, to: values.map((v) => v.trim()) } : null;
  const merges = changed && valuesAt(sc, values, path.length - 1).some((v) => v.toLowerCase() === values[path.length - 1].trim().toLowerCase())
    && values.slice(0, -1).every((v, i) => v.trim() === path[i]);
  const submit = () => run(change, setBusy, setError, () => {
    location.hash = routeHash(`browse:${schemaScope(schemaId, values.map((v) => v.trim()))}`);
    toast(`${plan?.label || "Done"}. You can undo it from Home.`, 5000);
  });
  return html`<${Dialog} title=${`Rename or move ${path[path.length - 1]}`} id="rename-dialog" onSubmit=${submit} busy=${busy || !plan || !plan.moving} error=${error} submitLabel="Move folders">
    <p class="muted">Change the ${last.label.toLowerCase()} to rename it. Use a ${last.label.toLowerCase()} that's already there to merge the two. Change a level above to move it. Every model below it moves to match.</p>
    <div class="cat-picker">${levels.map((l, i) => html`<label class="field-block" key=${l.key}><span>${l.label}</span>
      <input type="text" id=${`rename-${l.key}`} value=${values[i]} list=${`rename-list-${i}`} onInput=${(e) => setValues(values.map((v, j) => (j === i ? e.target.value : v)))} />
      <datalist id=${`rename-list-${i}`}>${valuesAt(sc, values, i).map((v) => html`<option value=${v} key=${v} />`)}</datalist></label>`)}</div>
    ${merges ? html`<p class="warn-note" id="rename-merge">${values[path.length - 1].trim()} is already there: the two will be merged.</p>` : null}
    ${change ? html`<${ChangePreview} change=${change} onPlan=${setPlan} />` : null}
  <//>`;
}

/** Edit a category: name, top folder, levels, model folder names and fields. */
export function EditSchema({ schemaId }) {
  const overview = useStore(ui, (s) => s.overview);
  const sc = overview?.schemas?.find((s) => s.id === schemaId);
  const [name, setName] = useState(sc?.name || "");
  const [folder, setFolder] = useState(sc?.folder || "");
  const [levels, setLevels] = useState((sc?.levels || []).map((l) => ({ key: l.key, label: l.label })));
  const [template, setTemplate] = useState(sc?.model_folder || "{name} ({author})");
  const [renameFolders, setRenameFolders] = useState(false);
  const [fields, setFields] = useState((sc?.fields || []).map((f) => ({ ...f, choices: (f.choices || []).join(", ") })));
  const [plan, setPlan] = useState(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  if (!sc) return null;
  const setAt = (list, set, i, v) => set(list.map((x, j) => (j === i ? v : x)));
  const swap = (i, d) => { const l = [...levels]; [l[i], l[i + d]] = [l[i + d], l[i]]; setLevels(l); };
  const spec = { name, folder, model_folder: template, levels, fields };
  const sameLevels = levels.length === sc.levels.length && levels.every((l, i) => l.key === sc.levels[i].key);
  const moves = folder.trim() !== sc.folder || !sameLevels || (renameFolders && template !== sc.model_folder);
  const change = { kind: "schema", schema: sc.id, spec, rename_folders: renameFolders };
  const ready = name.trim() && levels.every((l) => l.label.trim() && (l.key || l.value?.trim()));
  const submit = async () => {
    await run(change, setBusy, setError, () => toast(`Saved the category ${name.trim()}.${moves ? " You can undo it from Home." : ""}`, 5000));
  };
  return html`<${Dialog} title=${`Edit ${sc.name}`} id="edit-schema-dialog" onSubmit=${submit} busy=${busy || !ready || !plan} error=${error} submitLabel=${moves ? "Save and move folders" : "Save"}>
    <div class="form-two">
      <label class="field-block"><span>Name</span><input id="es-name" type="text" required maxlength="60" value=${name} onInput=${(e) => setName(e.target.value)} /></label>
      <label class="field-block"><span>Top folder</span><input id="es-folder" type="text" maxlength="60" value=${folder} onInput=${(e) => setFolder(e.target.value)} /></label>
    </div>
    <div class="field-block"><span class="field-label">Levels</span>
      <small class="muted">Renaming a level only changes its label. Adding, removing or reordering levels moves the folders.</small>
      ${levels.map((l, i) => html`<div class="level-row" key=${l.key || `new-${i}`}>
        <input type="text" class="es-level" maxlength="40" value=${l.label} aria-label=${`Level ${i + 1}`} onInput=${(e) => setAt(levels, setLevels, i, { ...l, label: e.target.value })} />
        ${l.key ? null : html`<input type="text" class="es-level-value" maxlength="80" placeholder="Value for models already there" value=${l.value || ""} aria-label=${`Level ${i + 1} value for existing models`}
          onInput=${(e) => setAt(levels, setLevels, i, { ...l, value: e.target.value })} />`}
        <button type="button" class="ghost" aria-label=${`Move level ${i + 1} up`} disabled=${i === 0} onClick=${() => swap(i, -1)}>↑</button>
        <button type="button" class="ghost" aria-label=${`Move level ${i + 1} down`} disabled=${i === levels.length - 1} onClick=${() => swap(i, 1)}>↓</button>
        <button type="button" class="ghost" aria-label=${`Remove level ${i + 1}`} onClick=${() => setLevels(levels.filter((_, j) => j !== i))}>${Icon.close(13)}</button>
      </div>`)}
      <div><button type="button" class="ghost" id="es-add-level" onClick=${() => setLevels([...levels, { key: null, label: "", value: "" }])} disabled=${levels.length >= 6}>${Icon.plus(13)} Add a level</button></div>
    </div>
    <label class="field-block"><span>Model folder name</span>
      <input id="es-template" type="text" maxlength="80" value=${template} onInput=${(e) => setTemplate(e.target.value)} />
      <label class="check-row"><input type="checkbox" id="es-rename-folders" checked=${renameFolders} onChange=${(e) => setRenameFolders(e.target.checked)} /> Rename the model folders already there to match</label></label>
    <div class="field-block"><span class="field-label">Fields</span>
      ${fields.map((f, i) => html`<div class="field-row" key=${f.key || `new-${i}`}>
        <input type="text" class="es-field" maxlength="40" value=${f.label} aria-label=${`Field ${i + 1}`} onInput=${(e) => setAt(fields, setFields, i, { ...f, label: e.target.value })} />
        <select aria-label=${`Field ${i + 1} type`} value=${f.type} onChange=${(e) => setAt(fields, setFields, i, { ...f, type: e.target.value })}>
          ${FIELD_TYPES.map(([v, label]) => html`<option value=${v} key=${v}>${label}</option>`)}</select>
        ${f.type === "choice" ? html`<input type="text" placeholder="28mm, 32mm, 75mm" value=${f.choices || ""} aria-label=${`Field ${i + 1} choices`} onInput=${(e) => setAt(fields, setFields, i, { ...f, choices: e.target.value })} />` : null}
        <button type="button" class="ghost" aria-label=${`Remove field ${i + 1}`} onClick=${() => setFields(fields.filter((_, j) => j !== i))}>${Icon.close(13)}</button>
      </div>`)}
      <div><button type="button" class="ghost" onClick=${() => setFields([...fields, { label: "", type: "text" }])}>${Icon.plus(13)} Add a field</button></div>
    </div>
    ${ready ? html`<${ChangePreview} change=${change} onPlan=${setPlan} />` : null}
    <div><button type="button" class="ghost danger-text" id="es-delete" onClick=${() => ui.set({ dialog: { type: "delete-schema", schemaId: sc.id } })}>Delete this category…</button></div>
  <//>`;
}

/** Delete a category: its models go to Unsorted. */
export function DeleteSchema({ schemaId }) {
  const overview = useStore(ui, (s) => s.overview);
  const sc = overview?.schemas?.find((s) => s.id === schemaId);
  const [plan, setPlan] = useState(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  if (!sc) return null;
  const change = { kind: "delete", schema: sc.id };
  const submit = () => run(change, setBusy, setError, () => {
    location.hash = routeHash("browse:unsorted");
    toast(`Deleted ${sc.name}; its models are in Unsorted. You can undo it from Home.`, 5000);
  });
  return html`<${Dialog} title=${`Delete ${sc.name}`} id="delete-schema-dialog" onSubmit=${submit} busy=${busy || !plan} error=${error} submitLabel="Delete category">
    <p>The category goes, and its models move to <b>Unsorted</b> with their folders' names. Their files and details are kept.</p>
    <${ChangePreview} change=${change} onPlan=${setPlan} />
  <//>`;
}

/** Several models' details at once. */
export function EditPicked({ models }) {
  const overview = useStore(ui, (s) => s.overview);
  const schemaId = models.every((m) => m.schema && m.schema === models[0].schema) ? models[0].schema : null;
  const sc = overview?.schemas?.find((s) => s.id === schemaId);
  const [form, setForm] = useState({ tags_add: "", tags_remove: "", authors: "", license: "" });
  const [fields, setFields] = useState({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const set = (k) => (e) => setForm({ ...form, [k]: e.target.value });
  const tags = [...new Set(models.flatMap((m) => m.tags || []))].sort();
  const submit = async () => {
    setBusy(true);
    setError("");
    try {
      const f = Object.fromEntries(Object.entries(fields).filter(([, v]) => v !== "" && v != null));
      const r = await api("models_update", { ids: models.map((m) => m.id), patch: { ...form, fields: f } });
      await loadOverview();
      if (r.errors.length) { setError(r.errors.map((e) => e.error).join(" ")); return; }
      close();
      toast(`Saved ${r.saved} ${r.saved === 1 ? "model" : "models"}.`);
    } catch (e) {
      setError(e.message || String(e));
    } finally {
      setBusy(false);
    }
  };
  return html`<${Dialog} title=${`Edit ${models.length} models`} id="bulk-dialog" onSubmit=${submit} busy=${busy} error=${error} submitLabel="Save">
    <p class="muted">Only what you fill in changes; the rest of each model's details stay as they are.</p>
    <div class="form-two">
      <label class="field-block"><span>Add tags</span><input id="bulk-tags-add" type="text" placeholder="presupported, monster" value=${form.tags_add} onInput=${set("tags_add")} /></label>
      <label class="field-block"><span>Remove tags</span><input id="bulk-tags-remove" type="text" list="bulk-tag-list" placeholder=${tags.slice(0, 3).join(", ")} value=${form.tags_remove} onInput=${set("tags_remove")} />
        <datalist id="bulk-tag-list">${tags.map((t) => html`<option value=${t} key=${t} />`)}</datalist></label>
      <label class="field-block"><span>Authors</span><input id="bulk-authors" type="text" placeholder="Replaces theirs; separate several with commas" value=${form.authors} onInput=${set("authors")} /></label>
      <label class="field-block"><span>Licence</span><input id="bulk-license" type="text" value=${form.license} onInput=${set("license")} /></label>
    </div>
    ${sc?.fields?.length ? html`<div class="form-two">${sc.fields.map((f) => html`<label class="field-block" key=${f.key}><span>${f.label}</span>
      <${FieldInput} field=${f} value=${fields[f.key]} onChange=${(v) => setFields({ ...fields, [f.key]: v })} /></label>`)}</div>`
      : html`<p class="muted">${schemaId ? "" : "Pick models from one category to set its fields too."}</p>`}
  <//>`;
}
