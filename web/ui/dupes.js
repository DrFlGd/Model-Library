// Duplicates: models whose 3D, slicer and archive files are the same as another
// model's, and files found in more than one model. Extra copies are set aside in
// the library's _library/set-aside folder (Undo in the message, or on Home), and
// deleted from there only when asked. Right-click a model for the same actions as
// everywhere; Ctrl+A ticks every group and Esc unticks them.
import { html, useState, useEffect } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui } from "./state.js";
import { ctx, routeHash } from "./context.js";
import { api, isDesktop, followJob, loadOverview, toast, undoable, undoChange } from "./library.js";
import { size } from "./details.js";
import { Icon } from "./icons.js";
import { confirmDialog } from "./dialogs.js";
import { MODEL_ACTIONS, menuItems, openMenu, usePageKeys, selectAllKey } from "./actions.js";

const placeOf = (overview, m) => {
  if (!m.schema) return "Unsorted";
  const sc = overview?.schemas?.find((s) => s.id === m.schema);
  return [sc?.name || m.schema, ...(m.path || [])].join(" › ");
};
const when = (t) => (t || "").replace("T", " ").slice(0, 16);
const keyOf = (g) => g.models.map((m) => m.id).sort().join(",");
const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;

/** What a running search is doing, in words. */
function progressText(p) {
  if (!p?.step) return "Starting…";
  if (p.step === "list") return `Reading model folders: ${Math.min(p.item + 1, p.items)} of ${p.items}`;
  const what = p.step === "compare" ? "Comparing files of the same size" : "Checking files that look the same";
  return `${what}: ${size(p.bytes)} of ${size(p.total_bytes)}`;
}

function Group({ g, keep, setKeep, include, setInclude, overview, readOnly, busy, setAside }) {
  const k = keyOf(g);
  const extra = g.models.filter((m) => m.id !== keep);
  return html`<li class="dupe-group" data-key=${k}>
    <div class="dupe-group-head">
      <label class="dupe-include"><input type="checkbox" checked=${include} onChange=${(e) => setInclude(k, e.currentTarget.checked)} />
        <b>${plural(g.models.length, "copy", "copies")}</b> of the same ${g.files === 1 ? "file" : `${g.files} files`} <span class="muted">· ${size(g.bytes)} each</span></label>
      <button type="button" class="ghost dupe-set-aside" disabled=${readOnly || busy} onClick=${() => setAside(extra.map((m) => m.id))}>Set aside ${extra.length === 1 ? "the other copy" : `the other ${extra.length}`}</button>
    </div>
    <ul class="dupe-models">${g.models.map((m) => html`<li key=${m.id} class=${m.id === keep ? "keep" : ""} data-model=${m.id}
      onContextMenu=${(e) => { if (!e.target.closest("input")) openMenu(e, menuItems(MODEL_ACTIONS, [m], { page: "dupes" })); }}>
      <label class="dupe-keep" title="Keep this copy"><input type="radio" name=${`keep-${k}`} checked=${m.id === keep} onChange=${() => setKeep(k, m.id)} /> ${m.id === keep ? "Keep" : "Extra"}</label>
      <span class="dupe-what"><a href=${routeHash(`model:${m.id}`)}>${m.name}</a>
        <span class="muted dupe-rel">${m.rel}</span></span>
      <span class="muted dupe-meta">${placeOf(overview, m)} · ${plural(m.count || 0, "file", "files")} · ${size(m.bytes)}</span>
    </li>`)}</ul>
  </li>`;
}

export function DuplicatesPage() {
  const lib = useStore(ui, (s) => s.library);
  const overview = useStore(ui, (s) => s.overview);
  const [data, setData] = useState(null);
  const [error, setError] = useState(null);
  const [job, setJob] = useState(null); // { id, progress }
  const [keep, setKeepMap] = useState({});
  const [skip, setSkip] = useState({});
  const [sharedLimit, setSharedLimit] = useState(50);
  const load = () => api("dupes_get", {}).then((d) => { setData(d); setError(null); }, (e) => setError(e.message || String(e)));
  useEffect(() => { if (lib) load(); }, [lib?.path]);
  usePageKeys((e) => {
    const groups = data?.groups || [];
    if (selectAllKey(e) && groups.length) { e.preventDefault(); setSkip({}); }
    else if (e.key === "Escape" && groups.length) setSkip(Object.fromEntries(groups.map((g) => [keyOf(g), true])));
  });
  if (!lib) return html`<div class="pages"><h1>Duplicates</h1><p>Open a library first.</p></div>`;
  const readOnly = !!lib.read_only;
  const busy = !!job;

  const find = async () => {
    try {
      const { job: id } = await api("dupes_find", {});
      setJob({ id, progress: null });
      const done = await followJob(id, "Looking for duplicates", (j) => setJob({ id, progress: j.progress }));
      if (done.error) throw new Error(done.error);
      const r = done.result || {};
      toast(r.groups || r.shared ? `Found ${plural(r.groups, "model with copies", "models with copies")} and ${plural(r.shared, "file", "files")} in more than one model.` : "No duplicates found.", 5000);
    } catch (e) { toast(e.message === "Stopped." ? "Stopped looking." : `Couldn't look for duplicates: ${e.message || e}`, 6000); }
    finally { setJob(null); setKeepMap({}); setSkip({}); await load(); }
  };
  const stop = () => job && api("job_cancel", { id: job.id });

  const setAside = async (ids) => {
    if (!ids.length) return;
    try {
      const { job: id } = await api("dupes_set_aside", { ids });
      setJob({ id, progress: null });
      const done = await followJob(id, "Setting copies aside");
      if (done.error) throw new Error(done.error);
      const r = done.result || {};
      if (r.failed?.length) throw new Error(`${plural(r.failed.length, "copy", "copies")} couldn't be moved (${r.failed[0].name}: ${r.failed[0].error}). Home has the change: finish it or put things back.`);
      undoable(`Set aside ${plural(r.moved, "copy", "copies")}.`, async () => {
        await undoChange(r.journal);
        await load();
        return "The copies are back where they were.";
      });
    } catch (e) { toast(e.message || String(e), 8000); }
    finally { setJob(null); await loadOverview(); await load(); }
  };
  const emptyAside = () => confirmDialog({
    title: "Delete set-aside copies",
    text: `${plural(aside.count, "copy", "copies")} (${size(aside.bytes)}) in ${aside.rel} will be deleted for good.`,
    button: `Delete ${plural(aside.count, "copy", "copies")}`,
    run: async () => {
      try {
        const r = await api("dupes_empty", {});
        toast(`Deleted ${plural(r.count, "set-aside copy", "set-aside copies")} (${size(r.bytes)}).`, 5000);
      } finally { await load(); }
    },
  });

  const groups = data?.groups || [];
  const shared = data?.shared || [];
  const keepOf = (g) => keep[keyOf(g)] || g.keep;
  const chosen = groups.filter((g) => !skip[keyOf(g)]);
  const extras = chosen.flatMap((g) => g.models.filter((m) => m.id !== keepOf(g)).map((m) => m.id));
  const aside = data?.set_aside || { count: 0, bytes: 0 };
  const revealAside = () => ctx.platform.library.openPath(`${lib.path}/${aside.rel}`);

  return html`<div class="pages dupes-page" id="dupes-page">
    <h1>Duplicates</h1>
    <p class="muted">Models whose 3D, slicer and archive files are all the same as another model's, and files found in more than one model. Files are compared by what's in them, not by their names.</p>
    ${error ? html`<p class="warn-note" role="alert">${error}</p>` : null}
    <div class="home-actions">
      <button type="button" class="primary" id="dupes-find" disabled=${busy} onClick=${find}>${Icon.search(15)} Look for duplicates</button>
      ${job ? html`<span class="dupes-progress" id="dupes-progress" role="status">${progressText(job.progress)}</span>
        <button type="button" class="ghost" id="dupes-stop" onClick=${stop}>Stop</button>` : null}
    </div>
    ${data?.found ? html`<p id="dupes-summary">Looked on ${when(data.found)} through ${plural(data.models || 0, "model", "models")}: ${plural(groups.length, "model has", "models have")} copies, ${plural(data.shared_count || 0, "file is", "files are")} in more than one model${data.wasted ? html`, and about <b>${size(data.wasted)}</b> could be saved` : null}.</p>`
      : html`<p id="dupes-summary" class="muted">Not looked yet. Looking reads the start and end of files that have the same size, then the whole of those that still match, so the first look at a big library takes a while. What's learnt is kept in each model's model.json, so later looks are quick.</p>`}
    ${data?.unreadable?.length ? html`<p class="warn-note">${plural(data.unreadable.length, "file", "files")} couldn't be read, for example ${data.unreadable[0].rel}/${data.unreadable[0].file}.</p>` : null}

    ${aside.count ? html`<div class="home-card" id="dupes-aside">
      <h2>Set aside</h2>
      <p>${plural(aside.count, "copy is", "copies are")} in <b>${aside.rel}</b> (${size(aside.bytes)}), out of the library's lists. Setting them aside can be undone from Recent changes on Home.</p>
      <div class="home-actions">
        ${isDesktop() ? html`<button type="button" class="ghost" onClick=${revealAside}>${Icon.folder(15)} Show in folder</button>` : null}
        <button type="button" class="ghost danger-text" id="dupes-empty" disabled=${busy || readOnly} onClick=${emptyAside}>${Icon.trash(15)} Delete set-aside copies…</button>
      </div>
    </div>` : null}

    ${groups.length ? html`<section class="dupes-section" id="dupes-groups">
      <div class="dupes-section-head">
        <h2>Models with copies (${groups.length})</h2>
        <button type="button" class="primary" id="dupes-set-aside" disabled=${readOnly || busy || !extras.length} onClick=${() => setAside(extras)}>Set aside ${plural(extras.length, "extra copy", "extra copies")}</button>
      </div>
      <p class="muted">The copy to keep is chosen for you: one in a category before one in Unsorted, then the one with more files. Choose another if you like, or untick a model to leave it as it is (Ctrl+A ticks them all, Esc unticks them).</p>
      <ul class="dupe-list">${groups.map((g) => html`<${Group} key=${keyOf(g)} g=${g} keep=${keepOf(g)} overview=${overview} readOnly=${readOnly} busy=${busy} setAside=${setAside}
        setKeep=${(k, id) => setKeepMap({ ...keep, [k]: id })} include=${!skip[keyOf(g)]} setInclude=${(k, on) => setSkip({ ...skip, [k]: !on })} />`)}</ul>
    </section>` : null}

    ${shared.length ? html`<section class="dupes-section" id="dupes-shared">
      <h2>Files in more than one model (${data.shared_count})</h2>
      <p class="muted">The same file in models that aren't copies of each other, such as a part used by two models. Nothing is set aside here; open a model to tidy it.</p>
      <table class="dupe-files"><thead><tr><th>File</th><th>Size</th><th>In</th></tr></thead>
        <tbody>${shared.slice(0, sharedLimit).map((f) => html`<tr key=${f.sha256}>
          <td>${f.name}</td><td class="num">${size(f.size)}</td>
          <td>${f.in.map((m, i) => html`${i ? ", " : ""}<a href=${routeHash(`model:${m.id}`)} title=${`${m.rel}/${m.file}`}>${m.name}</a>`)}</td></tr>`)}</tbody></table>
      ${shared.length > sharedLimit ? html`<button type="button" class="ghost" onClick=${() => setSharedLimit(sharedLimit + 100)}>Show more (${shared.length - sharedLimit} left)</button>` : null}
    </section>` : null}
    ${data?.found && !groups.length && !shared.length ? html`<p class="muted" id="dupes-none">No duplicates.</p>` : null}
  </div>`;
}
