// Home: the open library, how its folders are laid out, and what's coming.
// Also the first-start screen, which asks where the library should go.
import { html } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui } from "./state.js";
import { ctx, routeHash, schemaScope } from "./context.js";
import { api, isDesktop, openLibrary, showLibraryFolder, rescan, followJob, loadOverview, toast } from "./library.js";
import { sortFolder } from "./import.js";

const TREE = `_library/                  the app's own files
Unsorted/                  models with no category yet
Wargames/
  Warhammer 40k/
    Tyranid/
      Hive Tyrant (Author)/  one folder per model
        model.json           author, source, date…
        Hive Tyrant.stl
        Arms/                parts keep their folders
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

const NEXT = [
  ["Phase 0", "The app, its library folder and settings"],
  ["Phase 1", "Categories, model details and search"],
  ["Phase 2", "Importing models: files, folders and ZIPs, moved or copied into place, and moving models between categories"],
  ["Phase 3", "Viewing models: a 3D viewer, parts and variants, previews, pictures, readmes, PDFs and videos", true],
  ["Phase 4", "Editing categories, with folders moved to match"],
];

export function Home() {
  const s = useStore(ui, (st) => ({ library: st.library, error: st.libraryError, firstRun: st.firstRun, overview: st.overview }));
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
      <button type="button" class="ghost" onClick=${showLibraryFolder} disabled=${!lib}>Show in folder</button>
      <button type="button" class="ghost" onClick=${() => openLibrary()}>Open another library…</button>
      <button type="button" class="ghost" id="rescan" onClick=${() => rescan(true)} disabled=${!lib}>Read the folders again</button>
    </div>` : null}
    ${lib && !lib.read_only ? html`<div class="home-actions"><button type="button" class="ghost" id="make-previews" onClick=${makePreviews}>Make previews for models without one</button></div>` : null}
    ${ov ? html`<div class="home-card home-counts" id="home-counts">
      <h2>In this library</h2>
      <p><a href=${routeHash("browse:all")}><b>${ov.all}</b> ${ov.all === 1 ? "model" : "models"}</a>${ov.unsorted ? html`, <a href=${routeHash("browse:unsorted")}>${ov.unsorted} unsorted</a>` : null}.
        ${ov.schemas.length ? html` Categories: ${ov.schemas.map((sc, i) => html`${i ? ", " : ""}<a href=${routeHash(`browse:${schemaScope(sc.id)}`)} key=${sc.id}>${sc.name} (${sc.count})</a>`)}.`
          : html` No categories yet: make one with <b>New category…</b> in the menu, then put model folders under its folder.`}</p>
    </div>` : null}
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
