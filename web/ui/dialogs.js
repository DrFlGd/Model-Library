// Dialogs: making a new schema, and editing a model's details. Which one is open
// is `ui.dialog` ({ type: "new-schema" } or { type: "edit-model", model, schema }).
import { html, useState, useLayoutEffect, useRef } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui } from "./state.js";
import { routeHash, schemaScope } from "./context.js";
import { Icon } from "./icons.js";
import { api, loadOverview, saveDetails, runChange, undoable, recorded } from "./library.js";
import { CategoryPicker, SubcategoryTree, treeSpec, treeReady, firstBranch } from "./category.js";
import { RenameNode, EditSchema, DeleteSchema, DeleteSubcategory, EditPicked, AddSubcategory, ChangePreview } from "./categories.js";

export const FIELD_TYPES = [["text", "Text"], ["number", "Number"], ["choice", "Choice"], ["yes-no", "Yes or no"], ["date", "Date"]];
export const close = () => ui.set({ dialog: null });

/** A dialog: Esc or Cancel closes it; `danger` makes the button red (it deletes).
 *  Esc and the first box work as soon as it shows (a layout effect, not after a paint). */
export function Dialog({ title, id, onSubmit, busy, error, submitLabel, danger = false, children }) {
  const form = useRef(null);
  useLayoutEffect(() => {
    const key = (e) => { if (e.key === "Escape") close(); };
    addEventListener("keydown", key);
    const f = form.current;
    if (f && !f.contains(document.activeElement)) (f.querySelector("input:not([type=hidden]):not([disabled]), select, textarea") || f).focus();
    return () => removeEventListener("keydown", key);
  }, []);
  return html`<div class="dialog-backdrop" onMouseDown=${(e) => { if (e.target === e.currentTarget) close(); }}>
    <form class="dialog" id=${id} ref=${form} tabindex="-1" role="dialog" aria-modal="true" aria-label=${title} onSubmit=${(e) => { e.preventDefault(); onSubmit(); }}>
      <div class="dialog-head"><h2>${title}</h2>
        <button type="button" class="ghost" aria-label="Close" title="Close (Esc)" onClick=${close}>${Icon.close(15)}</button></div>
      ${children}
      ${error ? html`<p class="form-error" role="alert">${error}</p>` : null}
      <div class="dialog-actions">
        <button type="button" class="ghost" onClick=${close}>Cancel</button>
        <button type="submit" class=${danger ? "primary danger" : "primary"} disabled=${busy}>${submitLabel}</button>
      </div>
    </form>
  </div>`;
}

/** Make a folder-friendly name the way the core will (for the preview only). */
const folderish = (s) => s.replace(/[<>:"/\\|?*]/g, "-").replace(/\s+/g, " ").trim().replace(/[. ]+$/, "");

function NewSchema() {
  const [name, setName] = useState("");
  const [folder, setFolder] = useState("");
  const [tree, setTree] = useState([]);
  const [fields, setFields] = useState([]);
  const [template, setTemplate] = useState("{name} ({author})");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const top = folderish(folder || name) || "Category";
  const sample = (template.includes("{name}") ? template : "{name} ({author})").replace("{name}", "Model name").replace("{author}", "Author");
  const setAt = (list, set, i, v) => set(list.map((x, j) => (j === i ? v : x)));
  const submit = async () => {
    setBusy(true);
    setError("");
    try {
      const s = await api("schema_create", { schema: { name, folder, subcategories: treeSpec(tree), model_folder: template, fields } });
      await loadOverview();
      close();
      location.hash = routeHash(`browse:${schemaScope(s.id)}`);
      undoable(`Made the category ${s.name}, in the folder ${s.folder}.`, async () => {
        const now = ui.get().overview?.schemas?.find((x) => x.id === s.id);
        if (now?.count) throw new Error(`${s.name} has models in it now: use Delete category… if you want it gone.`);
        await runChange({ kind: "delete", schema: s.id, label: `Took back the new category ${s.name}` }, "Deleting the category");
        if (ui.get().route.startsWith(`browse:schema:${s.id}`)) location.hash = routeHash("home");
      });
    } catch (e) {
      setError(e.message || String(e));
    } finally {
      setBusy(false);
    }
  };
  return html`<${Dialog} title="New category" id="schema-dialog" onSubmit=${submit} busy=${busy || !treeReady(tree)} error=${error} submitLabel="Make category">
    <p class="muted">A category has its own top folder, and its subcategories are folders inside it. Add as many as you like, inside each other as deep as each needs. Every model gets its own folder, in any subcategory or at the top.</p>
    <div class="form-two">
      <label class="field-block"><span>Name</span>
        <input id="schema-name" type="text" required maxlength="60" placeholder="Category name" value=${name} onInput=${(e) => setName(e.target.value)} /></label>
      <label class="field-block"><span>Top folder</span>
        <input id="schema-folder" type="text" maxlength="60" placeholder=${folderish(name) || "Same as the name"} value=${folder} onInput=${(e) => setFolder(e.target.value)} /></label>
    </div>
    <div class="field-block"><span class="field-label">Subcategories</span>
      <small class="muted">Press Enter to add the next one beside it. ← and → move one out a level, or into the one above it.</small>
      <${SubcategoryTree} nodes=${tree} onChange=${(t) => setTree(t)} /></div>
    <label class="field-block"><span>Model folder name</span>
      <input id="schema-template" type="text" maxlength="80" value=${template} onInput=${(e) => setTemplate(e.target.value)} />
      <small class="muted">Use {name} and {author}. Folders already named this way are read back into a model's name and author.</small></label>
    <div class="field-block"><span class="field-label">Fields</span>
      <small class="muted">Details every model in this category can have, such as a size or a material. Name, authors, tags, source and release date are always there.</small>
      ${fields.map((f, i) => html`<div class="field-row" key=${i}>
        <input type="text" class="schema-field" maxlength="40" placeholder="Field name" value=${f.label} aria-label=${`Field ${i + 1}`} onInput=${(e) => setAt(fields, setFields, i, { ...f, label: e.target.value })} />
        <select aria-label=${`Field ${i + 1} type`} class="schema-field-type" value=${f.type} onChange=${(e) => setAt(fields, setFields, i, { ...f, type: e.target.value })}>
          ${FIELD_TYPES.map(([v, l]) => html`<option value=${v} key=${v}>${l}</option>`)}</select>
        ${f.type === "choice" ? html`<input type="text" class="schema-field-choices" placeholder="First choice, second choice" value=${f.choices || ""} aria-label=${`Field ${i + 1} choices`}
          onInput=${(e) => setAt(fields, setFields, i, { ...f, choices: e.target.value })} />` : null}
        <button type="button" class="ghost" aria-label=${`Remove field ${i + 1}`} onClick=${() => setFields(fields.filter((_, j) => j !== i))}>${Icon.close(13)}</button>
      </div>`)}
      <div><button type="button" class="ghost" id="add-field" onClick=${() => setFields([...fields, { label: "", type: "text" }])}>${Icon.plus(13)} Add a field</button></div>
    </div>
    <div class="field-block"><span class="field-label">Models will go in</span>
      <code class="preview-path" id="schema-preview">${[top, ...firstBranch(tree), sample].join(" / ")}</code></div>
  <//>`;
}

/** An input for one of the schema's fields, by its type. */
export function FieldInput({ field, value, onChange }) {
  const id = `field-${field.key}`;
  if (field.type === "choice") {
    const choices = field.choices || [];
    return html`<select id=${id} value=${value ?? ""} onChange=${(e) => onChange(e.target.value)}>
      <option value="">—</option>
      ${[...choices, ...(value && !choices.includes(value) ? [value] : [])].map((c) => html`<option value=${c} key=${c}>${c}</option>`)}</select>`;
  }
  if (field.type === "yes-no") {
    const v = value === true || value === "yes" ? "yes" : value === false || value === "no" ? "no" : "";
    return html`<select id=${id} value=${v} onChange=${(e) => onChange(e.target.value === "" ? "" : e.target.value === "yes")}>
      <option value="">—</option><option value="yes">Yes</option><option value="no">No</option></select>`;
  }
  const type = { number: "number", date: "date" }[field.type] || "text";
  return html`<input id=${id} type=${type} value=${value ?? ""} onInput=${(e) => onChange(e.target.value)} />`;
}

/** What the details form starts from: the model's details as the form has them. */
function formOf(model) {
  const d = model.details || {};
  return {
    name: model.name,
    authors: model.authors.join(", "),
    released: d.released || "",
    source: d.source?.url || (typeof d.source === "string" ? d.source : ""),
    license: d.license || "",
    tags: (model.tags || []).join(", "),
    notes: d.notes || "",
    cover: d.cover || "",
  };
}

function EditModel({ model, schema, select }) {
  const [form, setForm] = useState(() => formOf(model));
  const [fields, setFields] = useState({ ...(model.fields || {}) });
  const nameBox = useRef(null);
  useLayoutEffect(() => { nameBox.current?.focus(); if (select) nameBox.current?.select(); }, []);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const set = (k) => (e) => setForm({ ...form, [k]: e.target.value });
  const images = (model.files_list || []).filter((f) => f.kind === "image");
  const submit = async () => {
    setBusy(true);
    setError("");
    try {
      const patch = { ...form, fields: {} };
      for (const f of schema?.fields || []) patch.fields[f.key] = fields[f.key] ?? "";
      const v = await saveDetails(model, patch);
      close();
      recorded(`Saved ${v.name}'s details.`, v.journal);
    } catch (e) {
      setError(e.message || String(e));
    } finally {
      setBusy(false);
    }
  };
  return html`<${Dialog} title="Edit details" id="details-dialog" onSubmit=${submit} busy=${busy} error=${error} submitLabel="Save">
    <p class="muted">Saved in <span class="preview-path">${model.rel}/model.json</span>. Changing the name doesn't rename the folder.</p>
    <div class="form-two">
      <label class="field-block"><span>Name</span><input id="edit-name" ref=${nameBox} type="text" required maxlength="200" value=${form.name} onInput=${set("name")} /></label>
      <label class="field-block"><span>Authors</span><input id="edit-authors" type="text" placeholder="Separate several with commas" value=${form.authors} onInput=${set("authors")} /></label>
      <label class="field-block"><span>Released</span><input id="edit-released" type="date" value=${form.released} onInput=${set("released")} /></label>
      <label class="field-block"><span>Licence</span><input id="edit-license" type="text" value=${form.license} onInput=${set("license")} /></label>
    </div>
    <label class="field-block"><span>Source</span><input id="edit-source" type="url" placeholder="https://…" value=${form.source} onInput=${set("source")} /></label>
    <label class="field-block"><span>Tags</span><input id="edit-tags" type="text" placeholder="tag one, tag two" value=${form.tags} onInput=${set("tags")} /></label>
    ${schema?.fields?.length ? html`<div class="form-two">${schema.fields.map((f) => html`<label class="field-block" key=${f.key}><span>${f.label}</span>
      <${FieldInput} field=${f} value=${fields[f.key]} onChange=${(v) => setFields({ ...fields, [f.key]: v })} /></label>`)}</div>` : null}
    ${images.length ? html`<label class="field-block"><span>Cover picture</span>
      <select id="edit-cover" value=${form.cover} onChange=${set("cover")}><option value="">Chosen for me</option>
        ${images.map((f) => html`<option value=${f.rel} key=${f.rel}>${f.rel}</option>`)}</select></label>` : null}
    <label class="field-block"><span>Notes</span><textarea id="edit-notes" rows="3" value=${form.notes} onInput=${set("notes")}></textarea></label>
  <//>`;
}

/** Move one or several models to a category (or Unsorted). Their folders keep their
 *  names; the folders that move are listed first, and the move can be undone. */
function MoveModels({ models }) {
  const overview = useStore(ui, (s) => s.overview);
  const first = models[0];
  const same = models.every((m) => m.schema === first.schema);
  const [schema, setSchema] = useState(same ? first.schema : null);
  const [values, setValues] = useState(same ? [...(first.path || [])] : []);
  const [plan, setPlan] = useState(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const sc = overview?.schemas?.find((s) => s.id === schema);
  const where = sc ? [sc.folder, ...values].join(" / ") : "Unsorted";
  const change = { kind: "move", ids: models.map((m) => m.id), schema: schema || null, values };
  const n = plan?.moving || 0;
  const submit = async () => {
    setBusy(true);
    setError("");
    try {
      const r = await runChange(change, models.length === 1 ? `Moving ${first.name}` : `Moving ${models.length} models`);
      close();
      recorded(`${plan?.label || "Moved"}.`, r.journal);
    } catch (e) {
      setError(e.message || String(e));
    } finally {
      setBusy(false);
    }
  };
  return html`<${Dialog} title=${models.length === 1 ? `Move ${first.name} to a category` : `Move ${models.length} models to a category`} id="move-dialog" onSubmit=${submit} busy=${busy || !n} error=${error}
      submitLabel=${n ? `Move ${n} ${n === 1 ? "model" : "models"}` : "Move"}>
    <p class="muted">${models.length === 1 ? "Its folder moves" : "Their folders move"} into the subcategory's folder and ${models.length === 1 ? "keeps its" : "keep their"} name. A new subcategory is added to the category.</p>
    <div class="field-block"><span class="field-label">Category</span>
      <${CategoryPicker} overview=${overview} schema=${schema} values=${values} idPrefix="move" onChange=${(s, v) => { setSchema(s); setValues(v); }} /></div>
    <div class="field-block"><span class="field-label">Goes to</span><code class="preview-path" id="move-preview">${where} / …</code></div>
    <${ChangePreview} change=${change} onPlan=${setPlan} />
  <//>`;
}

/** Ask before something that can't be undone (docs/PLAN.md, "UI pass design", rule 6):
 *  one red button that names what it does. Open it with `confirmDialog`. */
function ConfirmDialog({ title, text, button, run }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const submit = async () => {
    setBusy(true);
    setError("");
    try {
      await run();
      close();
    } catch (e) {
      setError(e.message || String(e));
    } finally {
      setBusy(false);
    }
  };
  return html`<${Dialog} title=${title} id="confirm-dialog" onSubmit=${submit} busy=${busy} error=${error} submitLabel=${button} danger=${true}>
    <p>${text}</p>
    <p class="muted">This can't be undone.</p>
  <//>`;
}

/** `{ title, text, button, run }`: `run()` is what happens after the red button. */
export const confirmDialog = (opts) => ui.set({ dialog: { type: "confirm", ...opts } });

export function Dialogs() {
  const dialog = useStore(ui, (s) => s.dialog);
  if (!dialog) return null;
  if (dialog.type === "new-schema") return html`<${NewSchema} />`;
  if (dialog.type === "move-models") return html`<${MoveModels} models=${dialog.models} />`;
  if (dialog.type === "add-subcategory") return html`<${AddSubcategory} schemaId=${dialog.schemaId} path=${dialog.path} />`;
  if (dialog.type === "rename-node") return html`<${RenameNode} schemaId=${dialog.schemaId} path=${dialog.path} />`;
  if (dialog.type === "edit-schema") return html`<${EditSchema} schemaId=${dialog.schemaId} />`;
  if (dialog.type === "delete-schema") return html`<${DeleteSchema} schemaId=${dialog.schemaId} />`;
  if (dialog.type === "delete-subcategory") return html`<${DeleteSubcategory} schemaId=${dialog.schemaId} path=${dialog.path} />`;
  if (dialog.type === "edit-picked") return html`<${EditPicked} models=${dialog.models} />`;
  if (dialog.type === "confirm") return html`<${ConfirmDialog} ...${dialog} />`;
  if (dialog.type === "edit-model") return html`<${EditModel} model=${dialog.model} schema=${dialog.schema} select=${!!dialog.select} key=${dialog.model.id} />`;
  return null;
}
