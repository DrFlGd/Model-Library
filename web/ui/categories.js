// Changing categories after the fact (docs/PLAN.md, "Phase 4 design" and
// "Subcategory tree design"): renaming, merging or moving a subcategory, adding
// one, editing a category (schema) and its tree of subcategories, deleting one,
// and editing several models' details at once. Every change that moves folders
// shows a preview from the core first, then runs as a job that can be undone
// from its message or from Home. `categoryItems` is the Category menu, the same
// on every category page and on the sidebar.
import { html, useState, useEffect } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui } from "./state.js";
import { routeHash, schemaScope, ctx } from "./context.js";
import { Icon } from "./icons.js";
import { api, planChange, runChange, undoChange, loadOverview, toast, undoable, recorded, followIds } from "./library.js";
import { Dialog, close, FieldInput, FIELD_TYPES } from "./dialogs.js";
import { SubcategoryTree, editorTree, treeSpec, treeReady, treeList, nodeAt, origs } from "./category.js";
import { writable } from "./actions.js";
import { BUILTIN, CatIcon, iconOf } from "./caticons.js";
import { isDesktop } from "./library.js";
import { size } from "./details.js";
import { ColourSelect, remakePreview } from "./colours.js";

const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;

const KEY = "\u001f";
const placeName = (sc, path) => [sc.name, ...path].join(" › ");
const goTo = (id, path) => { location.hash = routeHash(`browse:${schemaScope(id, path)}`); };
const inside = (path, p) => p.length >= path.length && path.every((v, i) => v.toLowerCase() === p[i].toLowerCase());
/** Whether the page shows `path` of category `id` or something inside it. */
const showing = (id, path) => {
  const r = ui.get().route;
  if (!r.startsWith(`browse:schema:${id}`)) return false;
  const values = r.slice(`browse:schema:${id}`.length).split("/").filter(Boolean).map(decodeURIComponent);
  return inside(path, values);
};

/** A category change made, with Undo in its message (`back`: where to go after undoing it). */
function changed(r, text, back) {
  undoable(text, async () => {
    await undoChange(r.journal);
    if (back) goTo(...back);
    return "Undone: the folders are back where they were.";
  });
}

/** The Category menu: add, edit, rename or move, delete. `path`: [] for the
 *  category itself, or a subcategory's. */
export function categoryItems(schemaId, path) {
  const sc = ui.get().overview?.schemas?.find((x) => x.id === schemaId);
  if (!sc) return [];
  const ok = writable();
  const off = ok === true ? false : ok;
  return [
    { id: "add-subcategory", label: "Add subcategory…", icon: "plus", disabled: off, run: () => ui.set({ dialog: { type: "add-subcategory", schemaId, path } }) },
    { id: "edit-category", label: "Edit category…", icon: "edit", disabled: off, title: `Edit ${sc.name}: its subcategories, folders and fields`, run: () => ui.set({ dialog: { type: "edit-schema", schemaId } }) },
    { id: "choose-icon", label: "Choose icon…", icon: "image", disabled: off, run: () => ui.set({ dialog: { type: "choose-icon", schemaId, path } }) },
    ...(path.length ? [{ id: "rename-subcategory", label: "Rename or move…", icon: "move", disabled: off, run: () => ui.set({ dialog: { type: "rename-node", schemaId, path } }) }] : []),
    { id: "merge-categories", label: "Merge categories…", icon: "move", disabled: off, run: () => ui.set({ dialog: { type: "merge-categories", schemaId, path } }) },
    { sep: true },
    path.length
      ? { id: "delete-subcategory", label: "Remove subcategory…", icon: "trash", danger: true, disabled: off, run: () => ui.set({ dialog: { type: "delete-subcategory", schemaId, path } }) }
      : { id: "delete-category", label: "Delete category…", icon: "trash", danger: true, disabled: off, run: () => ui.set({ dialog: { type: "delete-schema", schemaId } }) },
  ];
}

/** What a change would move, from the core, refreshed as the form changes. */
export function ChangePreview({ change, onPlan }) {
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
    <p><b>${plan.moving ? `${plan.moving} ${plan.moving === 1 ? "model folder moves" : "model folders move"}` : "No model folders move"}</b>${plan.models > plan.moving ? `, ${plan.models - plan.moving} stay where they are` : ""}.${" "}
      ${plan.clashes ? html` <span class="warn-text">${plan.clashes} ${plan.clashes === 1 ? "lands" : "land"} on a folder that's already there and ${plan.clashes === 1 ? "gets" : "get"} a number, such as "(2)".</span>` : null}
      It can be undone.</p>
    ${plan.nodes != null ? html`<p class="muted">${plural(plan.nodes, "subcategory", "subcategories")} · ${plural(plan.files ?? 0, "file", "files")} · ${size(plan.bytes ?? 0)}</p>` : null}
    ${plan.child_collisions ? html`<p class="warn-note">${plan.child_collisions} ${plan.child_collisions === 1 ? "subcategory has a name" : "subcategories have names"} that's there already: they follow your choice below.</p>` : null}
    ${plan.node_mappings?.length ? html`<details class="muted"><summary>Where ${plural(plan.node_mappings.length, "subcategory goes", "subcategories go")}</summary>
      <ul class="move-sample" style="max-height:220px;overflow:auto">${plan.node_mappings.map((m, i) => html`<li key=${i}><span class="preview-path">${m.from}</span> → <span class="preview-path">${m.to}</span></li>`)}</ul></details>` : null}
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

/** Rename, merge or move one subcategory (and everything in it). */
export function RenameNode({ schemaId, path }) {
  const overview = useStore(ui, (s) => s.overview);
  const sc = overview?.schemas?.find((s) => s.id === schemaId);
  const [name, setName] = useState(path[path.length - 1]);
  const [parent, setParent] = useState(path.slice(0, -1).join(KEY));
  const [plan, setPlan] = useState(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  if (!sc) return null;
  const to = [...(parent ? parent.split(KEY) : []), name.trim()];
  const differs = name.trim() && to.join(KEY) !== path.join(KEY);
  const change = differs ? { kind: "category", schema: schemaId, from: path, to } : null;
  const merges = differs && !inside(path, to) && !!nodeAt(sc, to);
  const submit = () => run(change, setBusy, setError, (r) => {
    const here = showing(schemaId, path);
    if (here) goTo(schemaId, to);
    changed(r, `${plan?.label || "Done"}.`, here ? [schemaId, path] : null);
  });
  return html`<${Dialog} title=${`Rename or move ${path[path.length - 1]}`} id="rename-dialog" onSubmit=${submit} busy=${busy || !plan} error=${error} submitLabel="Move folders">
    <p class="muted">Change the name to rename it, or use the name of one that's already there to merge the two. Choose another place to move it, with everything in it.</p>
    <div class="form-two">
      <label class="field-block"><span>Name</span><input type="text" id="rename-name" required maxlength="80" value=${name} onInput=${(e) => setName(e.target.value)} /></label>
      <label class="field-block"><span>Inside</span>
        <select id="rename-parent" value=${parent} onChange=${(e) => setParent(e.target.value)}>
          <option value="">${`Top of ${sc.name}`}</option>
          ${treeList(sc).filter((n) => !inside(path, n.path)).map((n) => html`<option value=${n.path.join(KEY)} key=${n.path.join(KEY)}>${n.path.join(" › ")}</option>`)}
        </select></label>
    </div>
    ${merges ? html`<p class="warn-note" id="rename-merge">${[sc.name, ...to].join(" › ")} is already there: the two will be merged.</p>` : null}
    ${change ? html`<${ChangePreview} change=${change} onPlan=${setPlan} />` : null}
  <//>`;
}

/** Add a subcategory to a category or inside one of its subcategories (its folder is made too). */
/** Add a subcategory `name` under `path` of category `schemaId` (its folder is
 *  made now), with Undo in the message; the dialog and the menu's add row share it. */
export async function addSubcategory(schemaId, path, name) {
  const sc = ui.get().overview?.schemas?.find((s) => s.id === schemaId);
  const r = await api("subcategory_add", { schema: schemaId, path, name });
  await loadOverview();
  const made = Array.isArray(r?.path) && r.path.length ? r.path : [...path, name.trim()];
  undoable(`Added ${made[made.length - 1]} to ${placeName(sc, path)}.`, async () => {
    await api("subcategory_remove", { schema: schemaId, path: made });
    await loadOverview();
    if (showing(schemaId, made)) goTo(schemaId, path);
  });
  return made;
}

/** Give a category or subcategory an icon ({ icon: "builtin:…" | null } or { file }), with Undo. */
export async function setIcon(schemaId, path, choice) {
  const sc = ui.get().overview?.schemas?.find((s) => s.id === schemaId);
  const r = await api("category_icon_set", { schema: schemaId, path, ...choice });
  await loadOverview();
  const none = !choice.file && !choice.icon;
  undoable(`${none ? "Removed the icon of" : "Changed the icon of"} ${placeName(sc, path)}.`, async () => {
    await api("category_icon_set", { schema: schemaId, path, icon: r?.before ?? null });
    await loadOverview();
  });
}

/** Choose icon…: the built-in set (searchable), a picture of your own, or none. */
export function ChooseIcon({ schemaId, path }) {
  const overview = useStore(ui, (s) => s.overview);
  const sc = overview?.schemas?.find((s) => s.id === schemaId);
  const now = iconOf(sc, path);
  const [pick, setPick] = useState(now);
  const [q, setQ] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  if (!sc) return null;
  const words = q.toLowerCase().split(/\s+/).filter(Boolean);
  const shown = Object.entries(BUILTIN).filter(([name, [label, more]]) => words.every((w) => `${name} ${label} ${more}`.toLowerCase().includes(w)));
  const apply = async (choice) => {
    setBusy(true); setError("");
    try { await setIcon(schemaId, path, choice); close(); }
    catch (e) { setError(e.message || String(e)); }
    finally { setBusy(false); }
  };
  const own = async () => {
    const file = await ctx.platform.library.pickFile("Choose a picture for the icon", ["png", "jpg", "jpeg", "webp", "svg"]);
    if (file) apply({ file });
  };
  return html`<${Dialog} title=${`Icon for ${placeName(sc, path)}`} id="icon-dialog" onSubmit=${() => apply({ icon: pick })} busy=${busy || pick === now} error=${error} submitLabel="Use this icon">
    <input type="search" id="icon-search" placeholder="Search icons (castle, tool, plant…)" aria-label="Search icons" value=${q} onInput=${(e) => setQ(e.target.value)}
      onKeyDown=${(e) => { if (e.key === "Enter") { e.preventDefault(); if (shown.length) setPick(`builtin:${shown[0][0]}`); } }} />
    <div class="icon-grid" role="group" aria-label="Icons">
      <button type="button" class="icon-choice" data-icon="" aria-pressed=${!pick ? "true" : "false"} title="No icon" onClick=${() => setPick(null)}><span class="icon-none">${Icon.close(16)}</span><span>None</span></button>
      ${now?.startsWith("file:") ? html`<button type="button" class="icon-choice" data-icon=${now} aria-pressed=${pick === now ? "true" : "false"} title="Your picture" onClick=${() => setPick(now)}><${CatIcon} icon=${now} size=${22} /><span>Yours</span></button>` : null}
      ${shown.map(([name, [label]]) => html`<button type="button" key=${name} class="icon-choice" data-icon=${`builtin:${name}`} aria-pressed=${pick === `builtin:${name}` ? "true" : "false"} title=${label}
        onClick=${() => setPick(`builtin:${name}`)} onDblClick=${() => apply({ icon: `builtin:${name}` })}><${CatIcon} icon=${`builtin:${name}`} size=${22} /><span>${label}</span></button>`)}
    </div>
    ${!shown.length ? html`<p class="muted">No icon matches that.</p>` : null}
    ${isDesktop() ? html`<p><button type="button" class="ghost" id="icon-own" disabled=${busy} onClick=${own}>${Icon.image(15)} Use a picture of your own…</button>
      <span class="muted"> PNG, JPEG, WebP or SVG up to 512 KB, kept in the library.</span></p>` : null}
  </${Dialog}>`;
}

export function AddSubcategory({ schemaId, path }) {
  const overview = useStore(ui, (s) => s.overview);
  const sc = overview?.schemas?.find((s) => s.id === schemaId);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  if (!sc) return null;
  const submit = async () => {
    setBusy(true);
    setError("");
    try {
      await addSubcategory(schemaId, path, name);
      close();
    } catch (e) {
      setError(e.message || String(e));
    } finally {
      setBusy(false);
    }
  };
  return html`<${Dialog} title="Add subcategory" id="subcat-dialog" onSubmit=${submit} busy=${busy || !name.trim()} error=${error} submitLabel="Add">
    <p class="muted">A new subcategory in ${[sc.name, ...path].join(" › ")}. Its folder is made now, so it's there to import or move models into, even before it has any.</p>
    <label class="field-block"><span>Name</span><input id="subcat-name" type="text" required maxlength="80" value=${name} onInput=${(e) => setName(e.target.value)} /></label>
    <div class="field-block"><span class="field-label">Folder</span><code class="preview-path">${[sc.folder, ...path, name.trim() || "…"].join(" / ")}</code></div>
  <//>`;
}

/** Edit a category: name, top folder, its tree of subcategories, model folder names and fields. */
export function EditSchema({ schemaId }) {
  const overview = useStore(ui, (s) => s.overview);
  const sc = overview?.schemas?.find((s) => s.id === schemaId);
  const [name, setName] = useState(sc?.name || "");
  const [folder, setFolder] = useState(sc?.folder || "");
  const [tree, setTree] = useState(() => editorTree(sc));
  const [removed, setRemoved] = useState([]);
  const [template, setTemplate] = useState(sc?.model_folder || "{name} ({author})");
  const [renameFolders, setRenameFolders] = useState(false);
  const [fields, setFields] = useState((sc?.fields || []).map((f) => ({ ...f, choices: (f.choices || []).join(", ") })));
  const [plan, setPlan] = useState(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  if (!sc) return null;
  const setAt = (list, set, i, v) => set(list.map((x, j) => (j === i ? v : x)));
  const spec = { name, folder, model_folder: template, fields, subcategories: treeSpec(tree), removed };
  const moves = !!plan?.moving;
  const change = { kind: "schema", schema: sc.id, spec, rename_folders: renameFolders };
  const ready = name.trim() && treeReady(tree);
  const submit = async () => {
    await run(change, setBusy, setError, (r) => changed(r, `Saved the category ${name.trim()}.`));
  };
  return html`<${Dialog} title=${`Edit ${sc.name}`} id="edit-schema-dialog" onSubmit=${submit} busy=${busy || !ready || !plan} error=${error} submitLabel=${moves ? "Save and move folders" : "Save"}>
    <div class="form-two">
      <label class="field-block"><span>Name</span><input id="es-name" type="text" required maxlength="60" value=${name} onInput=${(e) => setName(e.target.value)} /></label>
      <label class="field-block"><span>Top folder</span><input id="es-folder" type="text" maxlength="60" value=${folder} onInput=${(e) => setFolder(e.target.value)} /></label>
    </div>
    <div class="field-block"><span class="field-label">Subcategories</span>
      <small class="muted">Renaming, moving or removing one moves its folders; a removed one's models move up to the one above it. The numbers are how many models each holds.</small>
      <${SubcategoryTree} id="es-tree" counts=${true} nodes=${tree} onChange=${(t, r) => { setTree(t); if (r.length) setRemoved([...removed, ...r]); }} /></div>
    <label class="field-block"><span>Model folder name</span>
      <input id="es-template" type="text" maxlength="80" value=${template} onInput=${(e) => setTemplate(e.target.value)} />
      <label class="check-row"><input type="checkbox" id="es-rename-folders" checked=${renameFolders} onChange=${(e) => setRenameFolders(e.target.checked)} /> Rename the model folders already there to match</label></label>
    <div class="field-block"><span class="field-label">Fields</span>
      ${fields.map((f, i) => html`<div class="field-row" key=${f.key || `new-${i}`}>
        <input type="text" class="es-field" maxlength="40" value=${f.label} aria-label=${`Field ${i + 1}`} onInput=${(e) => setAt(fields, setFields, i, { ...f, label: e.target.value })} />
        <select aria-label=${`Field ${i + 1} type`} value=${f.type} onChange=${(e) => setAt(fields, setFields, i, { ...f, type: e.target.value })}>
          ${FIELD_TYPES.map(([v, label]) => html`<option value=${v} key=${v}>${label}</option>`)}</select>
        ${f.type === "choice" ? html`<input type="text" placeholder="First choice, second choice" value=${f.choices || ""} aria-label=${`Field ${i + 1} choices`} onInput=${(e) => setAt(fields, setFields, i, { ...f, choices: e.target.value })} />` : null}
        <button type="button" class="ghost" aria-label=${`Remove field ${i + 1}`} onClick=${() => setFields(fields.filter((_, j) => j !== i))}>${Icon.close(13)}</button>
      </div>`)}
      <div><button type="button" class="ghost" onClick=${() => setFields([...fields, { label: "", type: "text" }])}>${Icon.plus(13)} Add a field</button></div>
    </div>
    ${ready ? html`<${ChangePreview} change=${change} onPlan=${setPlan} />` : null}
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
  const submit = () => run(change, setBusy, setError, (r) => {
    location.hash = routeHash("browse:unsorted");
    changed(r, `Deleted ${sc.name}; its models are in Unsorted.`, [sc.id, []]);
  });
  return html`<${Dialog} title=${`Delete ${sc.name}`} id="delete-schema-dialog" onSubmit=${submit} busy=${busy || !plan} error=${error} submitLabel="Delete category" danger=${true}>
    <p>The category goes, and its models move to <b>Unsorted</b> with their folders' names. Their files and details are kept.</p>
    <${ChangePreview} change=${change} onPlan=${setPlan} />
  <//>`;
}

/** Remove a subcategory with a reviewed choice of where its contents go. */
export function DeleteSubcategory({ schemaId, path }) {
  const sc = useStore(ui, (s) => s.overview)?.schemas?.find((s) => s.id === schemaId);
  const [mode, setMode] = useState("remove-up");
  const [plan, setPlan] = useState(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  if (!sc || !path.length) return null;
  const parent = path.slice(0, -1), name = path[path.length - 1];
  const change = { kind: "restructure", operation: mode, source: { schema: schemaId, path } };
  const submit = () => run(change, setBusy, setError, (r) => {
    if (showing(sc.id, path)) mode === "remove-unsorted" ? location.hash = routeHash("browse:unsorted") : goTo(sc.id, parent);
    changed(r, mode === "remove-unsorted"
      ? `Removed the subcategory ${name}; its models are in Unsorted.`
      : `Removed the subcategory ${name}; what was in it moved up into ${placeName(sc, parent)}.`, [sc.id, path]);
  });
  return html`<${Dialog} title=${`Remove subcategory ${name}`} id="delete-subcategory-dialog"
    onSubmit=${submit} busy=${busy || !plan} error=${error} submitLabel="Remove subcategory" danger=${true}>
    <p>Its models aren't deleted, and they keep their details and stars. Where should they go?</p>
    <div class="field-block" role="radiogroup" aria-label="Where its models go">
      <label class="check-row long"><input type="radio" name="remove-mode" checked=${mode === "remove-up"}
        onChange=${() => { setMode("remove-up"); setPlan(null); }} />
        <span>Up into ${placeName(sc, parent)} (its subcategories come too)</span></label>
      <label class="check-row long"><input type="radio" name="remove-mode" checked=${mode === "remove-unsorted"}
        onChange=${() => { setMode("remove-unsorted"); setPlan(null); }} />
        <span>To Unsorted (with the models in its subcategories, which go too)</span></label>
    </div>
    <${ChangePreview} change=${change} onPlan=${setPlan} />
    <p class="muted">A file or folder in it that isn't a model has to be made a model or moved out first.</p>
  <//>`;
}

/** Combine independent classification nodes into an existing or named new target. */
export function MergeCategories({ schemaId, path }) {
  const ov = useStore(ui, (s) => s.overview);
  const choices = (ov?.schemas || []).flatMap((sc) => [
    { schema: sc.id, path: [], label: sc.name },
    ...treeList(sc).map((n) => ({ schema: sc.id, path: n.path, label: [sc.name, ...n.path].join(" › ") })),
  ]);
  const key = (n) => JSON.stringify([n.schema, n.path]);
  const initial = key({ schema: schemaId, path });
  const [selected, setSelected] = useState([initial]);
  const [kind, setKind] = useState("existing");
  const [target, setTarget] = useState(key(choices.find((n) => key(n) !== initial) || choices[0] || { schema: schemaId, path: [] }));
  const [name, setName] = useState("");
  const [conflicts, setConflicts] = useState("cancel");
  const [plan, setPlan] = useState(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const sources = choices.filter((c) => selected.includes(key(c))).map(({ schema, path }) => ({ schema, path }));
  const dest = choices.find((c) => key(c) === target);
  const targetRef = kind === "new-category" ? { name: name.trim() }
    : kind === "new-subcategory" ? { schema: dest?.schema, parent: dest?.path || [], name: name.trim() }
    : { schema: dest?.schema, path: dest?.path || [] };
  const change = sources.length >= 2 && dest && (kind === "existing" || name.trim())
    ? { kind: "restructure", operation: "merge", sources, target: targetRef, child_conflicts: conflicts } : null;
  const submit = () => run(change, setBusy, setError, (r) => {
    location.hash = routeHash("home");
    changed(r, `${plan?.label || "Merged categories"}.`);
  });
  return html`<${Dialog} title="Merge categories and subcategories" id="merge-categories-dialog"
    onSubmit=${submit} busy=${busy || !plan} error=${error} submitLabel="Merge categories">
    <p>Choose two or more to merge, and where they go. Their models keep their details and stars.</p>
    <div class="field-block"><span class="field-label">Merge these</span>
      <div id="merge-sources" role="group" aria-label="Source categories" style="max-height:180px;overflow:auto">
        ${choices.map((c) => html`<label class="check-row" key=${key(c)}>
          <input type="checkbox" checked=${selected.includes(key(c))} onChange=${(e) => {
            setSelected(e.target.checked ? [...selected, key(c)] : selected.filter((k) => k !== key(c))); setPlan(null);
          }} /><span>${c.label}</span></label>`)}
      </div>
      <small class="muted">Don't choose a category together with a subcategory inside it.</small>
    </div>
    <label class="field-block"><span>Into</span><select id="merge-target-kind" value=${kind} onChange=${(e) => { setKind(e.target.value); setPlan(null); }}>
      <option value="existing">One that's already there</option>
      <option value="new-subcategory">A new subcategory</option>
      <option value="new-category">A new category</option>
    </select></label>
    ${kind !== "new-category" ? html`<label class="field-block"><span>${kind === "existing" ? "Which one" : "Inside"}</span>
      <select id="merge-target" value=${target} onChange=${(e) => { setTarget(e.target.value); setPlan(null); }}>
        ${choices.map((c) => html`<option key=${key(c)} value=${key(c)}>${c.label}</option>`)}
      </select></label>` : null}
    ${kind !== "existing" ? html`<label class="field-block"><span>New name</span>
      <input id="merge-name" maxlength="80" value=${name} onInput=${(e) => { setName(e.target.value); setPlan(null); }} /></label>` : null}
    <label class="field-block"><span>If both have a subcategory with the same name</span>
      <select id="merge-child-conflicts" value=${conflicts} onChange=${(e) => { setConflicts(e.target.value); setPlan(null); }}>
        <option value="cancel">Stop and ask me</option>
        <option value="merge">Merge them</option>
        <option value="rename">Keep both, numbering the new one</option>
      </select></label>
    ${!change && sources.length < 2 ? html`<p class="warn-note" role="status">Choose at least two to merge.</p>` : null}
    ${change ? html`<${ChangePreview} change=${change} onPlan=${setPlan} />` : null}
    <p class="muted">If two models' folders have the same name, both are kept and one gets a number. A file or folder that isn't a model has to be made a model or moved out first.</p>
  <//>`;
}

/** Several models' details at once. */
export function EditPicked({ models }) {
  const overview = useStore(ui, (s) => s.overview);
  const schemaId = models.every((m) => m.schema && m.schema === models[0].schema) ? models[0].schema : null;
  const sc = overview?.schemas?.find((s) => s.id === schemaId);
  const [form, setForm] = useState({ tags_add: "", tags_remove: "", authors: "", license: "", render_colour: "" });
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
      followIds(r.ids || {});
      await loadOverview();
      if (form.render_colour) remakePreview(models.map((m) => (r.ids || {})[m.id] || m.id));
      if (r.errors.length) { setError(r.errors.map((e) => e.error).join(" ")); return; }
      close();
      recorded(`Saved ${r.saved} ${r.saved === 1 ? "model's" : "models'"} details.`, r.journal);
    } catch (e) {
      setError(e.message || String(e));
    } finally {
      setBusy(false);
    }
  };
  return html`<${Dialog} title=${`Edit ${models.length} models`} id="bulk-dialog" onSubmit=${submit} busy=${busy} error=${error} submitLabel="Save">
    <p class="muted">Only what you fill in changes; the rest of each model's details stay as they are.</p>
    <div class="form-two">
      <label class="field-block"><span>Add tags</span><input id="bulk-tags-add" type="text" placeholder="tag one, tag two" value=${form.tags_add} onInput=${set("tags_add")} /></label>
      <label class="field-block"><span>Remove tags</span><input id="bulk-tags-remove" type="text" list="bulk-tag-list" placeholder=${tags.slice(0, 3).join(", ")} value=${form.tags_remove} onInput=${set("tags_remove")} />
        <datalist id="bulk-tag-list">${tags.map((t) => html`<option value=${t} key=${t} />`)}</datalist></label>
      <label class="field-block"><span>Authors</span><input id="bulk-authors" type="text" placeholder="Replaces theirs; separate several with commas" value=${form.authors} onInput=${set("authors")} /></label>
      <label class="field-block"><span>Licence</span><input id="bulk-license" type="text" value=${form.license} onInput=${set("license")} /></label>
      <label class="field-block"><span>3D colour</span><${ColourSelect} id="bulk-colour" value=${form.render_colour === "none" ? null : form.render_colour} keep="Leave as it is" none="None (as set on this computer)"
        onChange=${(v) => setForm({ ...form, render_colour: v === null ? "none" : v })} /></label>
    </div>
    ${sc?.fields?.length ? html`<div class="form-two">${sc.fields.map((f) => html`<label class="field-block" key=${f.key}><span>${f.label}</span>
      <${FieldInput} field=${f} value=${fields[f.key]} onChange=${(v) => setFields({ ...fields, [f.key]: v })} /></label>`)}</div>`
      : html`<p class="muted">${schemaId ? "" : "Pick models from one category to set its fields too."}</p>`}
  <//>`;
}
