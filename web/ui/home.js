// Home: the place to start (docs/PLAN.md, "UI pass design", step 3): models added
// lately, what's waiting to be sorted, the changes made lately (with Undo), the
// categories, and one Library ▾ menu. Also the first-start screen, which asks
// where the library should go.
import { html, useState, useEffect } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, setPref } from "./state.js";
import { ctx, routeHash, schemaScope } from "./context.js";
import { api, isDesktop, openLibrary, showLibraryFolder, rescan, followJob, loadOverview, toast, undoChange, finishChange, recorded } from "./library.js";
import { sortFolder } from "./sort.js";
import { Cover } from "./details.js";
import { PageHead } from "./layout.js";

const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;

/** Draw a preview for every model that has none yet. */
async function makePreviews() {
  try {
    const { job } = await api("thumbs_make", {});
    const done = await followJob(job, "Making previews");
    await loadOverview();
    if (done.error) throw new Error(done.error);
    const r = done.result || {};
    if (!r.made && !r.no_3d && !r.failed?.length) return toast("Every model has a preview already.");
    toast(`Made ${r.made || 0} ${r.made === 1 ? "preview" : "previews"}${r.failed?.length ? `; ${r.failed.length} couldn't be drawn` : ""}.`, 5000);
  } catch (e) { toast(`Couldn't make previews: ${e.message || e}`, 6000); }
}

/** The Library ▾ menu: what's done to the whole library. */
function libraryItems(lib) {
  return [
    ...(isDesktop() ? [{ id: "show-library", label: "Show in folder", icon: "folder", run: showLibraryFolder }] : []),
    { id: "rescan", label: "Read again", icon: "refresh", title: "Read every model folder again, in case something changed that the app missed", run: () => rescan(true) },
    { id: "make-previews", label: "Make missing previews", icon: "image", disabled: lib.read_only ? "The library is read-only." : false,
      title: "Draw a preview for every model that has none yet", run: makePreviews },
    ...(isDesktop() ? [{ sep: true }, { id: "open-another", label: "Open another library…", icon: "folder", run: () => openLibrary() }] : []),
  ];
}

const STATES = { done: "", undone: "undone", running: "interrupted", undoing: "interrupted while undoing", interrupted: "needs recovery", stopped: "stopped partway", emptied: "copies deleted" };

/** Changes recorded in the library (categories, moves, imports, details): undo one
 *  (changes that move folders newest first; details while nothing newer touched the
 *  same models); finish or put back an interrupted one. */
function RecentChanges({ lib, rev }) {
  const [list, setList] = useState(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => { api("journals").then(setList, () => setList([])); }, [lib.path, rev]);
  if (!list?.length) return null;
  const act = async (fn, id, what) => {
    setBusy(true);
    try { await fn(id); toast(what); } catch (e) { toast(e.message || String(e), 8000); } finally { setBusy(false); }
  };
  const first = list.find((j) => j.undo === true);
  const broken = list.find((j) => ["running", "undoing", "stopped"].includes(j.state) || (j.kind === "archive-op" && j.state === "interrupted"));
  const when = (t) => (t || "").replace("T", " ").slice(0, 16);
  return html`<section class="home-card home-wide" id="recent-changes">
    <h2>Recent changes</h2>
    ${broken && !lib.read_only ? html`<div class="warn-note" role="alert" id="change-broken">
      <p>${broken.label} didn't finish${broken.error ? `: ${broken.error}` : "."} ${broken.kind === "archive-op" ? "Verified outputs are preserved until the original contents can be recovered." : "Some folders may have moved and others not."}</p>
      ${broken.kind === "archive-op"
        ? html`<div class="home-actions"><button type="button" class="primary" id="archive-recover" disabled=${busy || broken.undo !== true} title=${broken.undo !== true ? broken.undo || "Undo the newer change first." : "Verify originals before removing any ZIP outputs"} onClick=${() => act(undoChange, broken.id, "Archive operation safely undone.")}>Undo partial ZIP operation</button></div>`
      : broken.category_import
        ? html`<div class="home-actions"><button type="button" class="primary" id="putback-change" disabled=${busy}
            onClick=${() => act(undoChange, broken.id, "Reviewed import recovered; original files restored.")}>Put imported models back</button></div>`
        : broken.direction === "undo"
        ? html`<div class="home-actions"><button type="button" class="primary" id="finish-change" disabled=${busy} onClick=${() => act(undoChange, broken.id, "Undone: the folders are back where they were.")}>Finish undoing it</button>
          <button type="button" class="ghost" id="putback-change" disabled=${busy} onClick=${() => act(finishChange, broken.id, "The change is made again.")}>Make the change again</button></div>`
        : html`<div class="home-actions"><button type="button" class="primary" id="finish-change" disabled=${busy} onClick=${() => act(finishChange, broken.id, "Finished.")}>Finish it</button>
          <button type="button" class="ghost" id="putback-change" disabled=${busy} onClick=${() => act(undoChange, broken.id, "Put things back as they were.")}>Put things back</button></div>`}</div>` : null}
    <ul class="ls-list">${list.slice(0, 6).map((j) => html`<li key=${j.id} class=${j.state === "undone" ? "muted" : ""}><span>${j.label} <span class="muted">· ${when(j.created)} · ${plural(j.models, "model", "models")}${STATES[j.state] ? ` · ${STATES[j.state]}` : ""}</span></span>
      ${j.state === "done" && !lib.read_only ? html`<button type="button" class="ghost undo-change" id=${j === first ? "undo-change" : null} data-id=${j.id} disabled=${busy || j.undo !== true}
        title=${j.undo === true ? `Undo: ${j.label}` : j.undo || ""} onClick=${() => act(undoChange, j.id, `Undone: ${j.label.charAt(0).toLowerCase()}${j.label.slice(1)}.`)}>Undo</button>` : null}</li>`)}</ul>
  </section>`;
}

/** The models added last, newest first. */
function RecentlyAdded({ rev, total }) {
  const [list, setList] = useState(null);
  useEffect(() => { api("models_query", { scope: "all", sort: "added", limit: 6 }).then((r) => setList(r.items || []), () => setList([])); }, [rev]);
  if (!list?.length) return null;
  const seeAll = (e) => { e.preventDefault(); setPref({ sort: "added" }); location.hash = routeHash("browse:all"); };
  return html`<section class="home-card home-wide" id="home-added">
    <div class="home-card-head"><h2>Recently added</h2>${total > list.length ? html`<a href=${routeHash("browse:all")} onClick=${seeAll}>See all ${total}</a>` : null}</div>
    <div class="home-strip">${list.map((m) => html`<a class="home-model" key=${m.id} href=${routeHash(`model:${m.id}`)} title=${m.name} data-model=${m.id}>
      <${Cover} model=${m} /><span class="card-name">${m.name}</span><span class="card-sub">${(m.added || "").slice(0, 10)}</span></a>`)}</div>
  </section>`;
}

/** Models with no place yet: Unsorted, and folders in the library outside any category. */
function Waiting({ lib, ov }) {
  const loose = ov.loose || [];
  const files = ov.loose_files || [];
  const [busy, setBusy] = useState(false);
  const wrap = async (rels) => {
    setBusy(true);
    try {
      const { job } = await api("loose_wrap", { files: rels });
      const done = await followJob(job, "Putting files in folders");
      await loadOverview();
      if (done.error) throw new Error(done.error);
      const result = done.result || {};
      if (result.failed?.length) throw new Error(result.failed.map(f => f.error || f).join("; "));
      const journal = result.journal || result.items?.at(-1)?.journal;
      recorded(rels.length === 1 ? "Put the file in its own model folder." : "Put the files in their own model folders. Undo each from Recent changes.", journal);
    } catch (e) { toast(e.message || String(e), 8000); }
    finally { setBusy(false); }
  };
  return html`<section class="home-card" id="home-waiting">
    <h2>Waiting to be sorted</h2>
    ${ov.unsorted ? html`<p><a href=${routeHash("browse:unsorted")}>${plural(ov.unsorted, "model is", "models are")} in Unsorted</a>: select ${ov.unsorted === 1 ? "it" : "them"} there and choose Move to category… to give ${ov.unsorted === 1 ? "it" : "them"} a place.</p>` : null}
    ${loose.length ? html`<div id="home-loose">
      <p>These folders are in the library but not in a category or Unsorted, so their models aren't listed. Sort them to give each model a place.</p>
      <ul class="ls-list">${loose.map((f) => html`<li key=${f}><span>${f}</span>
        <button type="button" class="ghost sort-loose" data-folder=${f} onClick=${() => sortFolder(`${lib.path}/${f}`)}>Sort…</button></li>`)}</ul></div>` : null}
    ${files.length ? html`<div id="home-loose-files">
      <p>These model files need their own folders. Matching pictures and documents go with them.</p>
      <button type="button" class="ghost" id="wrap-all-loose" disabled=${busy || !!lib.read_only} title=${lib.read_only || "Keep each model in its current category"} onClick=${() => wrap(files.map(f => f.rel))}>Put all in folders</button>
      <ul class="ls-list">${files.map(f => html`<li key=${f.rel}><span class="loose-file-path">${f.rel}</span>
        <button type="button" class="ghost wrap-loose" data-file=${f.rel} disabled=${busy || !!lib.read_only} title=${lib.read_only || ""} onClick=${() => wrap([f.rel])}>Put in a folder</button></li>`)}</ul>
    </div>` : null}
    ${!ov.unsorted && !loose.length && !files.length ? html`<p class="muted">Nothing: every model has a category. New models come in through <a href=${routeHash("import")}>Import</a>.</p>` : null}
  </section>`;
}

function Categories({ ov }) {
  return html`<section class="home-card" id="home-categories">
    <h2>Categories</h2>
    ${ov.schemas.length ? html`<ul class="ls-list">${ov.schemas.map((sc) => html`<li key=${sc.id}><span><a href=${routeHash(`browse:${schemaScope(sc.id)}`)}>${sc.name}</a></span><span class="muted">${plural(sc.count, "model", "models")}</span></li>`)}</ul>`
      : html`<p>No categories yet: make one with <b>New category…</b> in the menu, then put model folders under its folder.</p>`}
  </section>`;
}

export function Home() {
  const s = useStore(ui, (st) => ({ library: st.library, error: st.libraryError, firstRun: st.firstRun, overview: st.overview, rev: st.catalogRev }));
  const lib = s.library;
  if (s.firstRun && !lib) return html`<${FirstRun} />`;
  const ov = s.overview;
  if (!lib) {
    return html`<div class="home">
      <${PageHead} title="Model Library" sub="Organise your 3D models into folders laid out by your own categories." />
      ${s.error ? html`<p class="warn-note" role="alert">${s.error}</p>` : null}
      ${isDesktop() ? html`<div class="home-actions"><button type="button" class="primary" id="home-open-library" onClick=${() => openLibrary()}>Open a library…</button></div>` : null}
    </div>`;
  }
  const count = ov ? html`<span class="page-count" id="home-counts">${plural(ov.all, "model", "models")}${ov.unsorted ? `, ${ov.unsorted} unsorted` : ""}</span>` : null;
  return html`<div class="home">
    <${PageHead} title=${lib.name} count=${count} sub=${html`Your library is in <b>${lib.path}</b>. How its folders are laid out is in Settings.`}
      menu=${{ id: "library-menu", label: "Library", icon: "folder", title: "Show in folder, read again, make previews, open another library", items: () => libraryItems(lib) }} />
    ${s.error ? html`<p class="warn-note" role="alert">${s.error}</p>` : null}
    ${lib.read_only ? html`<p class="warn-note" role="alert">${lib.read_only}</p>` : null}
    <div class="home-grid">
      <${RecentlyAdded} rev=${s.rev} total=${ov?.all || 0} />
      ${ov ? html`<${Waiting} lib=${lib} ov=${ov} />` : null}
      ${ov ? html`<${Categories} ov=${ov} />` : null}
      <${RecentChanges} lib=${lib} rev=${s.rev} />
    </div>
  </div>`;
}

/** First start: no library was ever opened, so ask where it should go. */
function FirstRun() {
  const def = ctx.platform?.info?.default_library || "";
  return html`<div class="first-run" id="first-run">
    <h1>Where should your library go?</h1>
    <p>The library is an ordinary folder. Models are sorted into folders inside it by your categories, with their details saved next to them, so you can move it to another drive or computer and open it again later.</p>
    <p>Choose an empty folder for a new library, or a folder that already is one.</p>
    <div class="home-actions">
      ${def ? html`<button type="button" class="primary" id="use-default" onClick=${() => openLibrary(def)}>Use ${def}</button>` : null}
      <button type="button" class=${def ? "ghost" : "primary"} id="choose-library" onClick=${() => openLibrary()}>Choose a folder…</button>
    </div>
    <p class="muted">You can open a different library at any time in Settings.</p>
  </div>`;
}
