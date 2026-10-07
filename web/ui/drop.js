// Folders and files dropped on the window from outside (docs/PLAN.md, "Import
// follow-ups design"): a menu where they were dropped asks what to do with them.
// On a category page they can go straight into that category as a model (moved
// in, or copied), anywhere else into Unsorted; a folder can instead be sorted on
// the Import page, to choose which folders are models first. While something is
// dragged over the window, it says where a drop goes.
import { html } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui } from "./state.js";
import { routeHash } from "./context.js";
import { api, followJob, loadOverview, recorded, toast } from "./library.js";
import { openMenu } from "./actions.js";
import { addSources, loadSession } from "./sort.js";
import { Icon } from "./icons.js";

const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;
const baseName = (p) => p.split(/[\\/]/).filter(Boolean).pop() || p;

/** Where a drop goes: the category or subcategory shown, else Unsorted. */
export function dropPlace(st = ui.get()) {
  const r = st.route || "";
  if (r.startsWith("browse:schema:")) {
    const [id, ...rest] = r.slice(14).split("/");
    const values = rest.map(decodeURIComponent);
    const sc = st.overview?.schemas?.find((x) => x.id === id);
    if (sc) return { schema: id, values, label: [sc.name, ...values].join(" › ") };
  }
  return { schema: null, values: [], label: "Unsorted" };
}

/** A background job, followed to the end; its result, or it throws. */
async function job(cmd, args, label) {
  const { job: id } = await api(cmd, args);
  const done = await followJob(id, label);
  if (done.error) throw new Error(done.error);
  return done.result;
}

/** Put what was dropped into `place`: each one a model (`one`: all of it one
 *  model), moved in or copied (`mode`). It goes through the Import page's
 *  workspace, so it's checked, recorded and undone as an import is. */
async function addTo(info, place, { one, mode }) {
  const paths = info.map((i) => i.path);
  try {
    await job("sort_add", { paths, contents: false }, info.length === 1 ? `Reading ${info[0].name}` : `Reading ${info.length} things`);
    const se = await api("sort_get");
    let ids = paths.map((p) => se.items.find((i) => i.path === p && !i.done)?.id);
    if (ids.some((x) => !x)) {
      await loadSession();
      throw new Error("Some of that is on the Import page already: give it a category there.");
    }
    if (one && ids.length > 1) ids = [(await api("sort_group", { ids })).id];
    await api("sort_send", { ids, schema: place.schema, values: place.values });
    const r = await job("sort_commit", { ids, mode, forget: true }, mode === "copy" ? "Copying in" : "Moving in");
    await loadSession();
    await loadOverview();
    const ok = (r.results || []).filter((x) => !x.error);
    if (!ok.length) throw new Error((r.results || []).find((x) => x.error)?.error || "Nothing was added.");
    const what = ok.length === 1 ? ok[0].name : plural(ok.length, "model", "models");
    const bad = (r.results || []).length - ok.length;
    recorded(`${mode === "copy" ? "Copied" : "Moved"} ${what} into ${place.label}.${bad ? ` ${plural(bad, "one wasn't", "weren't")} added: see the Import page.` : ""}`, r.journal);
  } catch (e) {
    toast(e.message || String(e), 8000);
  }
}

/** Sort what was dropped on the Import page: folders with what's in them read
 *  as models, files as models of their own. */
async function sortOnImport(info) {
  if (ui.get().route !== "import") location.hash = routeHash("import");
  const dirs = info.filter((i) => i.dir).map((i) => i.path);
  const files = info.filter((i) => !i.dir).map((i) => i.path);
  if (dirs.length) await addSources(dirs, true);
  if (files.length) await addSources(files, false);
}

/** Folders and files dropped on the window: ask what to do with them, in a menu
 *  where they were dropped (`at`: the point, in CSS pixels). */
export async function onDropped(paths, at) {
  ui.set({ dragging: false });
  const st = ui.get();
  if (!paths?.length || !st.library) return;
  let info;
  try { info = await api("sort_paths", { paths }); } catch { info = paths.map((p) => ({ path: p, name: baseName(p), dir: false, there: true })); }
  info = info.filter((i) => i.there);
  if (!info.length) return toast("Nothing to add: what was dropped isn't there any more.");
  const n = info.length;
  const folders = info.filter((i) => i.dir).length;
  const head = n === 1 ? `${info[0].name}${info[0].dir ? " (folder)" : ""}`
    : `${[folders ? plural(folders, "folder", "folders") : "", n - folders ? plural(n - folders, "file", "files") : ""].filter(Boolean).join(" and ")}: ${info.slice(0, 3).map((i) => i.name).join(", ")}${n > 3 ? "…" : ""}`;
  const ro = st.library.read_only ? "The library is read-only." : false;
  let items;
  if (st.route === "import") {
    items = [
      { head },
      { id: "drop-add", label: n === 1 ? "Add as a model" : `Add as ${n} models`, icon: "box", sub: "Each one is a model; give it a category here", run: () => addSources(info.map((i) => i.path), false) },
      ...(folders ? [{ id: "drop-sort", label: folders === 1 ? "Sort what's in it" : "Sort what's in them", icon: "folder", sub: "Choose which folders are models", run: () => sortOnImport(info) }] : []),
    ];
  } else {
    const place = dropPlace(st);
    items = [
      { head },
      { id: "drop-add", label: `Add to ${place.label} as ${n === 1 ? "a model" : `${n} models`}`, icon: "move", sub: "Moves it into the library; Undo puts it back", disabled: ro, run: () => addTo(info, place, { one: n === 1, mode: "move" }) },
      ...(n > 1 ? [{ id: "drop-add-one", label: `Add to ${place.label} as one model`, icon: "layers", disabled: ro, run: () => addTo(info, place, { one: true, mode: "move" }) }] : []),
      { id: "drop-copy", label: `Copy to ${place.label} instead`, icon: "copy", sub: "The originals stay where they are", disabled: ro, run: () => addTo(info, place, { one: n === 1, mode: "copy" }) },
      ...(folders ? [{ sep: true }, { id: "drop-sort", label: "Sort it on Import…", icon: "inbox", sub: "Choose which folders are models first", run: () => sortOnImport(info) }] : []),
    ];
  }
  const x = at?.x ?? innerWidth / 2 - 140;
  const y = at?.y ?? innerHeight / 3;
  openMenu({ clientX: x, clientY: y }, items);
}

/** While something is dragged over the window: where a drop goes. */
export function DropHint() {
  const st = useStore(ui, (s) => ({ dragging: s.dragging, route: s.route, overview: s.overview, library: s.library }));
  if (!st.dragging || !st.library) return null;
  const where = st.route === "import" ? "add it to the workspace or sort it" : `add it to ${dropPlace(st).label} or sort it`;
  return html`<div class="drop-hint" id="drop-hint" aria-hidden="true"><div class="drop-hint-box">${Icon.download(28)}<span>Drop to ${where}</span></div></div>`;
}
