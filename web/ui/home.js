// Home: the open library, how its folders are laid out, and what's coming.
// Also the first-start screen, which asks where the library should go.
import { html, useState, useEffect } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui } from "./state.js";
import { ctx, routeHash, schemaScope } from "./context.js";
import { api, isDesktop, openLibrary, showLibraryFolder, rescan, followJob, loadOverview, toast, undoChange, finishChange } from "./library.js";
import { sortFolder } from "./sort.js";
import { Icon } from "./icons.js";

const TREE = `_library/                  the app's own files
Unsorted/                  models with no category yet
Category/                  a category's top folder
  Subcategory/             as many as you like,
    Subcategory/           inside each other, as deep as each needs
      Model name (Author)/   one folder per model, at any level
        model.json           author, source, date…
        Model name.stl
        Parts/               parts keep their folders
        _media/              pictures, PDFs, videos
        _thumbs/             previews`;

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

const STATES = { done: "", undone: "undone", running: "interrupted", undoing: "interrupted while undoing", stopped: "stopped partway", emptied: "copies deleted" };

/** Category changes recorded in the library: undo the newest; finish or put back an interrupted one. */
function RecentChanges({ lib, rev }) {
  const [list, setList] = useState(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => { api("journals").then(setList, () => setList([])); }, [lib.path, rev]);
  if (!list?.length) return null;
  const act = async (fn, id, what) => {
    setBusy(true);
    try { await fn(id); toast(what); } catch (e) { toast(e.message || String(e), 8000); } finally { setBusy(false); }
  };
  const live = list.filter((j) => j.state !== "undone");
  const newest = live[0];
  const broken = list.find((j) => ["running", "undoing", "stopped"].includes(j.state));
  const when = (t) => (t || "").replace("T", " ").slice(0, 16);
  return html`<div class="home-card home-counts" id="recent-changes">
    <h2>Recent changes</h2>
    ${broken && !lib.read_only ? html`<div class="warn-note" role="alert" id="change-broken">
      <p>${broken.label} didn't finish${broken.error ? `: ${broken.error}` : "."} Some folders may have moved and others not.</p>
      ${broken.direction === "undo"
        ? html`<div class="home-actions"><button type="button" class="primary" id="finish-change" disabled=${busy} onClick=${() => act(undoChange, broken.id, "Undone: the folders are back where they were.")}>Finish undoing it</button>
          <button type="button" class="ghost" id="putback-change" disabled=${busy} onClick=${() => act(finishChange, broken.id, "The change is made again.")}>Make the change again</button></div>`
        : html`<div class="home-actions"><button type="button" class="primary" id="finish-change" disabled=${busy} onClick=${() => act(finishChange, broken.id, "Finished.")}>Finish it</button>
          <button type="button" class="ghost" id="putback-change" disabled=${busy} onClick=${() => act(undoChange, broken.id, "Put things back as they were.")}>Put things back</button></div>`}</div>` : null}
    <ul class="ls-list">${list.slice(0, 6).map((j) => html`<li key=${j.id} class=${j.state === "undone" ? "muted" : ""}><span>${j.label} <span class="muted">· ${when(j.created)} · ${j.models} ${j.models === 1 ? "model" : "models"}${STATES[j.state] ? ` · ${STATES[j.state]}` : ""}</span></span>
      ${j === newest && j.state === "done" && !lib.read_only ? html`<button type="button" class="ghost" id="undo-change" disabled=${busy} onClick=${() => act(undoChange, j.id, "Undone: the folders are back where they were.")}>Undo</button>` : null}</li>`)}</ul>
  </div>`;
}

const NEXT = [
  ["Phase 0", "The app, its library folder and settings"],
  ["Phase 1", "Categories, model details and search"],
  ["Phase 2", "Importing models: files, folders and ZIPs, moved or copied into place, and moving models between categories"],
  ["Phase 3", "Viewing models: a 3D viewer, parts and variants, previews, pictures, readmes, PDFs and videos"],
  ["Phase 4", "Editing categories: a tree of subcategories, renamed, merged and moved with folders to match, and undo"],
  ["Phase 5", "Large collections: sorting a whole folder tree as it is, finding duplicates, noticing changes made outside the app"],
  ["UI pass", "The same actions, keys and undo on every page, then one page layout", true],
  ["Phase 6", "Running on a server (Docker) with sign-in, to browse the library from another computer (on hold)"],
];

export function Home() {
  const s = useStore(ui, (st) => ({ library: st.library, error: st.libraryError, firstRun: st.firstRun, overview: st.overview, rev: st.catalogRev }));
  const lib = s.library;
  if (s.firstRun && !lib) return html`<${FirstRun} />`;
  const ov = s.overview;
  return html`<div class="home">
    <div class="home-head">
      <h1>${lib ? lib.name : "Model Library"}</h1>
      <p>${lib ? html`Your library is in <b>${lib.path}</b>. Models go into folders laid out by your categories, so the library makes sense in any file manager, with or without this app.`
        : "Organise your 3D models into folders laid out by your own categories."}</p>
    </div>
    ${s.error ? html`<p class="warn-note" role="alert">${s.error}</p>` : null}
    ${lib?.read_only ? html`<p class="warn-note" role="alert">${lib.read_only}</p>` : null}
    ${isDesktop() ? html`<div class="home-actions">
      <button type="button" class="ghost" onClick=${showLibraryFolder} disabled=${!lib}>${Icon.folder(15)} Show in folder</button>
      <button type="button" class="ghost" id="rescan" onClick=${() => rescan(true)} disabled=${!lib} title="Read every model folder again, in case something changed that the app missed">${Icon.refresh(15)} Read again</button>
      ${lib && !lib.read_only ? html`<button type="button" class="ghost" id="make-previews" onClick=${makePreviews} title="Draw a preview for every model that has none yet">${Icon.image(15)} Make missing previews</button>` : null}
      <button type="button" class="ghost" onClick=${() => openLibrary()}>Open another library…</button>
    </div>` : lib && !lib.read_only ? html`<div class="home-actions"><button type="button" class="ghost" id="make-previews" onClick=${makePreviews}>${Icon.image(15)} Make missing previews</button></div>` : null}
    ${ov ? html`<div class="home-card home-counts" id="home-counts">
      <h2>In this library</h2>
      <p><a href=${routeHash("browse:all")}><b>${ov.all}</b> ${ov.all === 1 ? "model" : "models"}</a>${ov.unsorted ? html`, <a href=${routeHash("browse:unsorted")}>${ov.unsorted} unsorted</a>` : null}.
        ${ov.schemas.length ? html` Categories: ${ov.schemas.map((sc, i) => html`${i ? ", " : ""}<a href=${routeHash(`browse:${schemaScope(sc.id)}`)} key=${sc.id}>${sc.name} (${sc.count})</a>`)}.`
          : html` No categories yet: make one with <b>New category…</b> in the menu, then put model folders under its folder.`}</p>
    </div>` : null}
    ${lib ? html`<${RecentChanges} lib=${lib} rev=${s.rev} />` : null}
    ${ov?.loose?.length ? html`<div class="home-card home-counts" id="home-loose">
      <h2>Folders not sorted yet</h2>
      <p>These folders are in the library but not in a category or Unsorted, so their models aren't listed. Sort them to give each model a place.</p>
      <ul class="ls-list">${ov.loose.map((f) => html`<li key=${f}><span>${f}</span>
        <button type="button" class="ghost sort-loose" data-folder=${f} onClick=${() => sortFolder(`${lib.path}/${f}`)}>Sort…</button></li>`)}</ul>
    </div>` : null}
    <div class="home-pair">
      <section class="home-card">
        <h2>How the library is laid out</h2>
        <pre class="folder-tree" aria-label="Example folder layout">${TREE}</pre>
      </section>
      <section class="home-card">
        <h2>What's coming</h2>
        <ol class="phase-list">${NEXT.map(([phase, what, now]) => html`<li class=${now ? "now" : ""} key=${phase}>${what}${now ? " (this version)" : ""}</li>`)}</ol>
      </section>
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
