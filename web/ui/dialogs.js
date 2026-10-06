// Dialogs: making a new schema, and editing a model's details. Which one is open
// is `ui.dialog` ({ type: "new-schema" } or { type: "edit-model", model, schema }).
import { html, useState, useEffect } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui } from "./state.js";
import { routeHash, schemaScope } from "./context.js";
import { Icon } from "./icons.js";
import { api, loadOverview, saveDetails, toast } from "./library.js";

const FIELD_TYPES = [["text", "Text"], ["number", "Number"], ["choice", "Choice"], ["yes-no", "Yes or no"], ["date", "Date"]];
const close = () => ui.set({ dialog: null });

function Dialog({ title, id, onSubmit, busy, error, submitLabel, children }) {
  useEffect(() => {
    const key = (e) => { if (e.key === "Escape") close(); };
    addEventListener("keydown", key);
    return () => removeEventListener("keydown", key);
  }, []);
  return html`<div class="dialog-backdrop" onMouseDown=${(e) => { if (e.target === e.currentTarget) close(); }}>
    <form class="dialog" id=${id} role="dialog" aria-modal="true" aria-label=${title} onSubmit=${(e) => { e.preventDefault(); onSubmit(); }}>
      <div class="dialog-head"><h2>${title}</h2>
        <button type="button" class="ghost" aria-label="Close" onClick=${close}>${Icon.close(15)}</button></div>
      ${children}
      ${error ? html`<p class="form-error" role="alert">${error}</p>` : null}
      <div class="dialog-actions">
        <button type="button" class="ghost" onClick=${close}>Cancel</button>
        <button type="submit" class="primary" disabled=${busy}>${submitLabel}</button>
      </div>
    </form>
  </div>`;
}

/** Make a folder-friendly name the way the core will (for the preview only). */
const folderish = (s) => s.replace(/[<>:"/\\|?*]/g, "-").replace(/\s+/g, " ").trim().replace(/[. ]+$/, "");

function NewSchema() {
  const [name, setName] = useState("");
  const [folder, setFolder] = useState("");
  const [levels, setLevels] = useState(["", ""]);
  const [fields, setFields] = useState([]);
  const [template, setTemplate] = useState("{name} ({author})");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const top = folderish(folder || name) || "Wargames";
  const shownLevels = levels.map((l, i) => l.trim() || (levels.length === 2 ? ["Game", "Faction"][i] : `Level ${i + 1}`));
  const sample = (template.includes("{name}") ? template : "{name} ({author})").replace("{name}", "Hive Tyrant").replace("{author}", "Jo Smith");
  const setAt = (list, set, i, v) => set(list.map((x, j) => (j === i ? v : x)));
  const submit = async () => {
    setBusy(true);
    setError("");
    try {
      const s = await api("schema_create", { schema: { name, folder, levels: levels.map((label) => ({ label })), model_folder: template, fields } });
      await loadOverview();
      close();
      location.hash = routeHash(`browse:${schemaScope(s.id)}`);
      toast(`Made the schema ${s.name}, in the folder ${s.folder}.`);
    } catch (e) {
      setError(e.message || String(e));
    } finally {
      setBusy(false);
    }
  };
  return html`<${Dialog} title="New schema" id="schema-dialog" onSubmit=${submit} busy=${busy} error=${error} submitLabel="Make schema">
    <p class="muted">A schema is a kind of model with its own top folder. Its levels are the folders below that, and every model gets a folder at the bottom.</p>
    <div class="form-two">
      <label class="field-block"><span>Name</span>
        <input id="schema-name" type="text" required maxlength="60" placeholder="Wargames" value=${name} onInput=${(e) => setName(e.target.value)} /></label>
      <label class="field-block"><span>Top folder</span>
        <input id="schema-folder" type="text" maxlength="60" placeholder=${folderish(name) || "Same as the name"} value=${folder} onInput=${(e) => setFolder(e.target.value)} /></label>
    </div>
    <div class="field-block"><span class="field-label">Levels</span>
      ${levels.map((l, i) => html`<div class="level-row" key=${i}>
        <input type="text" class="schema-level" maxlength="40" placeholder=${levels.length === 2 ? ["Game", "Faction"][i] : `Level ${i + 1}`} value=${l} aria-label=${`Level ${i + 1}`}
          onInput=${(e) => setAt(levels, setLevels, i, e.target.value)} />
        <button type="button" class="ghost" aria-label=${`Remove level ${i + 1}`} onClick=${() => setLevels(levels.filter((_, j) => j !== i))}>${Icon.close(13)}</button>
      </div>`)}
      <div><button type="button" class="ghost" id="add-level" onClick=${() => setLevels([...levels, ""])} disabled=${levels.length >= 6}>${Icon.plus(13)} Add a level</button></div>
    </div>
    <label class="field-block"><span>Model folder name</span>
      <input id="schema-template" type="text" maxlength="80" value=${template} onInput=${(e) => setTemplate(e.target.value)} />
      <small class="muted">Use {name} and {author}. Folders already named this way are read back into a model's name and author.</small></label>
    <div class="field-block"><span class="field-label">Fields</span>
      <small class="muted">Details every model of this kind can have, such as Scale or Presupported. Name, authors, tags, source and release date are always there.</small>
      ${fields.map((f, i) => html`<div class="field-row" key=${i}>
        <input type="text" class="schema-field" maxlength="40" placeholder="Scale" value=${f.label} aria-label=${`Field ${i + 1}`} onInput=${(e) => setAt(fields, setFields, i, { ...f, label: e.target.value })} />
        <select aria-label=${`Field ${i + 1} type`} class="schema-field-type" value=${f.type} onChange=${(e) => setAt(fields, setFields, i, { ...f, type: e.target.value })}>
          ${FIELD_TYPES.map(([v, l]) => html`<option value=${v} key=${v}>${l}</option>`)}</select>
        ${f.type === "choice" ? html`<input type="text" class="schema-field-choices" placeholder="28mm, 32mm, 75mm" value=${f.choices || ""} aria-label=${`Field ${i + 1} choices`}
          onInput=${(e) => setAt(fields, setFields, i, { ...f, choices: e.target.value })} />` : null}
        <button type="button" class="ghost" aria-label=${`Remove field ${i + 1}`} onClick=${() => setFields(fields.filter((_, j) => j !== i))}>${Icon.close(13)}</button>
      </div>`)}
      <div><button type="button" class="ghost" id="add-field" onClick=${() => setFields([...fields, { label: "", type: "text" }])}>${Icon.plus(13)} Add a field</button></div>
    </div>
    <div class="field-block"><span class="field-label">Models will go in</span>
      <code class="preview-path" id="schema-preview">${[top, ...shownLevels.map((l) => `<${l}>`), sample].join(" / ")}</code></div>
  <//>`;
}

/** An input for one of the schema's fields, by its type. */
function FieldInput({ field, value, onChange }) {
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

function EditModel({ model, schema }) {
  const d = model.details || {};
  const [form, setForm] = useState({
    name: model.name,
    authors: model.authors.join(", "),
    released: d.released || "",
    source: d.source?.url || (typeof d.source === "string" ? d.source : ""),
    license: d.license || "",
    tags: (model.tags || []).join(", "),
    notes: d.notes || "",
    cover: d.cover || "",
  });
  const [fields, setFields] = useState({ ...(model.fields || {}) });
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
      await saveDetails(model, patch);
      close();
      toast("Details saved to model.json.");
    } catch (e) {
      setError(e.message || String(e));
    } finally {
      setBusy(false);
    }
  };
  return html`<${Dialog} title="Edit details" id="details-dialog" onSubmit=${submit} busy=${busy} error=${error} submitLabel="Save">
    <p class="muted">Saved in <span class="preview-path">${model.rel}/model.json</span>. Changing the name doesn't rename the folder.</p>
    <div class="form-two">
      <label class="field-block"><span>Name</span><input id="edit-name" type="text" required maxlength="200" value=${form.name} onInput=${set("name")} /></label>
      <label class="field-block"><span>Authors</span><input id="edit-authors" type="text" placeholder="Separate several with commas" value=${form.authors} onInput=${set("authors")} /></label>
      <label class="field-block"><span>Released</span><input id="edit-released" type="date" value=${form.released} onInput=${set("released")} /></label>
      <label class="field-block"><span>Licence</span><input id="edit-license" type="text" value=${form.license} onInput=${set("license")} /></label>
    </div>
    <label class="field-block"><span>Source</span><input id="edit-source" type="url" placeholder="https://…" value=${form.source} onInput=${set("source")} /></label>
    <label class="field-block"><span>Tags</span><input id="edit-tags" type="text" placeholder="presupported, monster" value=${form.tags} onInput=${set("tags")} /></label>
    ${schema?.fields?.length ? html`<div class="form-two">${schema.fields.map((f) => html`<label class="field-block" key=${f.key}><span>${f.label}</span>
      <${FieldInput} field=${f} value=${fields[f.key]} onChange=${(v) => setFields({ ...fields, [f.key]: v })} /></label>`)}</div>` : null}
    ${images.length ? html`<label class="field-block"><span>Cover picture</span>
      <select id="edit-cover" value=${form.cover} onChange=${set("cover")}><option value="">Chosen for me</option>
        ${images.map((f) => html`<option value=${f.rel} key=${f.rel}>${f.rel}</option>`)}</select></label>` : null}
    <label class="field-block"><span>Notes</span><textarea id="edit-notes" rows="3" value=${form.notes} onInput=${set("notes")}></textarea></label>
  <//>`;
}

export function Dialogs() {
  const dialog = useStore(ui, (s) => s.dialog);
  if (!dialog) return null;
  if (dialog.type === "new-schema") return html`<${NewSchema} />`;
  if (dialog.type === "edit-model") return html`<${EditModel} model=${dialog.model} schema=${dialog.schema} key=${dialog.model.id} />`;
  return null;
}
