// Collections (docs/PLAN.md, "Collections design"): named lists of models that
// cut across categories, such as "Print next" or "Gift ideas". A model can be in
// any number and its folder never moves. They're in the menu under Categories;
// "Add to collection…" (C) is with the other model actions, and every change
// says what it did with Undo.
import { html, useState, useEffect, useRef } from "../lib/html.js";
import { ui } from "./state.js";
import { routeHash } from "./context.js";
import { Icon } from "./icons.js";
import { api, loadOverview, followIds, toast, undoable } from "./library.js";
import { Dialog, close } from "./dialogs.js";

const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;
const nameOf = (ms) => (ms.length === 1 ? ms[0].name : plural(ms.length, "model", "models"));
const writableNow = () => !ui.get().library?.read_only;

/** The collection a browse scope shows (`coll:<id>`), from the overview. */
export function collectionOf(scope, overview) {
  if (!scope?.startsWith("coll:")) return null;
  const id = scope.slice(5);
  return overview?.collections?.find((c) => c.id === id) || { id, name: null, count: 0, gone: true };
}

/** The models' ids changed (they got a model.json): selection and stars follow. */
function follow(r) {
  const map = {};
  for (const x of r.ids || []) if (x.was !== x.id) map[x.was] = x.id;
  if (Object.keys(map).length) followIds(map);
}

/** Take models off a collection, with Undo. */
export async function removeFromCollection(ms, c) {
  try {
    await api("collections_change", { changes: [{ id: c.id, remove: ms.map((m) => m.id) }] });
    ui.set({ picked: [] });
    await loadOverview();
    undoable(`Took ${nameOf(ms)} off ${c.name}.`, async () => {
      await api("collections_change", { changes: [{ id: c.id, add: ms.map((m) => m.id) }] });
      await loadOverview();
    });
  } catch (e) { toast(e.message || String(e), 6000); }
}

/** Delete a collection (only the list: its models stay where they are), with Undo. */
export async function deleteCollection(c) {
  try {
    const gone = await api("collection_delete", { id: c.id });
    if (ui.get().route === `browse:coll:${c.id}`) location.hash = routeHash("browse:all");
    await loadOverview();
    undoable(`Deleted the collection ${c.name}. Its models are still in the library.`, async () => {
      await api("collection_restore", { collection: gone });
      await loadOverview();
    });
  } catch (e) { toast(e.message || String(e), 6000); }
}

/** A collection's menu: on its menu row (right-click) and its page's ⋯. */
export function collectionItems(c) {
  const ro = writableNow() ? false : "The library is read-only.";
  return [
    { id: "collection-rename", label: "Rename…", icon: "edit", disabled: ro, run: () => ui.set({ dialog: { type: "name-collection", collection: c } }) },
    { id: "collection-delete", label: "Delete collection", sub: "Its models stay in the library", icon: "trash", danger: true, disabled: ro, run: () => deleteCollection(c) },
  ];
}

/** New collection (from the menu, empty) or Rename (`collection`). */
export function NameCollection({ collection = null }) {
  const [name, setName] = useState(collection?.name || "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const submit = async () => {
    setBusy(true);
    setError("");
    try {
      if (collection) {
        const was = collection.name;
        const r = await api("collection_rename", { id: collection.id, name });
        await loadOverview();
        close();
        if (r.name !== was) undoable(`Renamed ${was} to ${r.name}.`, async () => { await api("collection_rename", { id: collection.id, name: was }); await loadOverview(); });
      } else {
        const r = await api("collection_create", { name });
        await loadOverview();
        close();
        location.hash = routeHash(`browse:coll:${r.id}`);
        undoable(`Made the collection ${r.name}. Select models and press C to add them.`, async () => {
          await api("collection_delete", { id: r.id });
          if (ui.get().route === `browse:coll:${r.id}`) location.hash = routeHash("browse:all");
          await loadOverview();
        });
      }
    } catch (e) { setError(e.message || String(e)); } finally { setBusy(false); }
  };
  return html`<${Dialog} title=${collection ? `Rename ${collection.name}` : "New collection"} id="collection-name-dialog" onSubmit=${submit} busy=${busy || !name.trim()} error=${error}
      submitLabel=${collection ? "Rename" : "Make collection"}>
    ${collection ? null : html`<p class="muted">A collection is a list of models from any categories, such as "Print next". A model can be in several; its folder doesn't move.</p>`}
    <label class="field-block"><span>Name</span><input type="text" id="collection-name" required maxlength="80" value=${name} onInput=${(e) => setName(e.target.value)} /></label>
  <//>`;
}

/** A tick box that can also show "some of them" (a dash). */
function Tri({ state, onChange, label, count }) {
  const ref = useRef(null);
  useEffect(() => { if (ref.current) ref.current.indeterminate = state === "some"; }, [state]);
  return html`<label class="collect-row" data-collection=${label}>
    <input type="checkbox" ref=${ref} checked=${state === "all"} aria-checked=${state === "some" ? "mixed" : state === "all" ? "true" : "false"}
      onChange=${() => onChange(state === "all" ? "none" : "all")} />
    <span class="collect-name">${Icon.bookmark(14)} ${label}</span>
    <span class="muted">${count}</span>
  </label>`;
}

/** Add to collection… (C): each collection ticked when every model chosen is in it,
 *  a dash when some are; tick or untick, or name a new one, then Save. */
export function CollectDialog({ models }) {
  const [list, setList] = useState(null);
  const [want, setWant] = useState({}); // collection id: "all" | "none"
  const [fresh, setFresh] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const ids = models.map((m) => m.id);
  useEffect(() => {
    api("collections_list", {}).then(setList, (e) => setError(e.message || String(e)));
  }, []);
  const was = (c) => {
    const n = ids.filter((id) => c.models.includes(id)).length;
    return n === 0 ? "none" : n === ids.length ? "all" : "some";
  };
  const stateOf = (c) => want[c.id] || was(c);
  const submit = async () => {
    setBusy(true);
    setError("");
    try {
      let made = null;
      if (fresh.trim()) made = await api("collection_create", { name: fresh });
      const changes = (list || []).filter((c) => want[c.id]).map((c) => {
        const inIt = ids.filter((id) => c.models.includes(id));
        return want[c.id] === "all"
          ? { id: c.id, name: c.name, add: ids.filter((id) => !inIt.includes(id)), remove: [] }
          : { id: c.id, name: c.name, add: [], remove: inIt };
      }).filter((ch) => ch.add.length || ch.remove.length);
      if (made) changes.push({ id: made.id, name: made.name, add: ids, remove: [] });
      if (!changes.length) { close(); return; }
      const r = await api("collections_change", { changes: changes.map(({ id, add, remove }) => ({ id, add, remove })) });
      follow(r);
      const now = (id) => (r.ids || []).find((x) => x.was === id)?.id || id;
      await loadOverview();
      close();
      const added = changes.filter((ch) => ch.add.length).map((ch) => ch.name);
      const taken = changes.filter((ch) => ch.remove.length).map((ch) => ch.name);
      const text = [added.length ? `Added ${nameOf(models)} to ${added.join(", ")}` : "", taken.length ? `${added.length ? "took" : "Took"} ${added.length ? (models.length === 1 ? "it" : "them") : nameOf(models)} off ${taken.join(", ")}` : ""].filter(Boolean).join(" and ");
      undoable(`${text}.`, async () => {
        const back = changes.filter((ch) => !made || ch.id !== made.id).map((ch) => ({ id: ch.id, add: ch.remove, remove: ch.add.map(now) }));
        if (back.length) await api("collections_change", { changes: back });
        if (made) await api("collection_delete", { id: made.id });
        await loadOverview();
      });
    } catch (e) { setError(e.message || String(e)); } finally { setBusy(false); }
  };
  return html`<${Dialog} title=${`Add ${nameOf(models)} to a collection`} id="collect-dialog" onSubmit=${submit} busy=${busy || !list} error=${error} submitLabel="Save">
    ${list?.length ? html`<div class="collect-list" role="group" aria-label="Collections">${list.map((c) => html`<${Tri} key=${c.id} label=${c.name} count=${c.models.length}
        state=${stateOf(c)} onChange=${(v) => setWant({ ...want, [c.id]: v })} />`)}</div>
      ${list.some((c) => was(c) === "some") ? html`<p class="muted" id="collect-dash-note">A dash means some of them are in it already.</p>` : null}` : list ? html`<p class="muted">No collections yet: name the first one below.</p>` : null}
    <label class="field-block"><span>New collection</span><input type="text" id="collect-new" maxlength="80" placeholder="Name, such as Print next" value=${fresh} onInput=${(e) => setFresh(e.target.value)} /></label>
  <//>`;
}

/** The collections a model is in, as links to their pages. */
export function CollectionLinks({ list, id = null }) {
  if (!list?.length) return null;
  return html`<span class="tag-links collection-links" id=${id}>${list.map((c) => html`<a key=${c.id} class="chip collection-link" href=${routeHash(`browse:coll:${c.id}`)}
    title=${`Show the collection ${c.name}`}>${Icon.bookmark(12)} ${c.name}</a>`)}</span>`;
}
