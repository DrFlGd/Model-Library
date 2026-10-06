// Home: the open library, how its folders are laid out, and what's coming.
// Also the first-start screen, which asks where the library should go.
import { html } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui } from "./state.js";
import { ctx, routeHash, schemaScope } from "./context.js";
import { isDesktop, openLibrary, showLibraryFolder, rescan } from "./library.js";

const TREE = `_library/                  the app's own files
Unsorted/                  imports with no schema yet
Wargames/
  Warhammer 40k/
    Tyranid/
      Hive Tyrant (Author)/  one folder per model
        model.json           author, source, date…
        Hive Tyrant.stl
        Arms/                parts keep their folders
        _media/              pictures, PDFs, videos
        _thumbs/             previews`;

const NEXT = [
  ["Phase 0", "The app, its library folder and settings"],
  ["Phase 1", "Schemas, model details and search", true],
  ["Phase 2", "Importing models: files, folders and ZIPs, moved or copied into place"],
  ["Phase 3", "Viewing models, their parts, pictures, PDFs and videos"],
  ["Phase 4", "Editing schemas and categories, with folders moved to match"],
];

export function Home() {
  const s = useStore(ui, (st) => ({ library: st.library, error: st.libraryError, firstRun: st.firstRun, overview: st.overview }));
  const lib = s.library;
  if (s.firstRun && !lib) return html`<${FirstRun} />`;
  const ov = s.overview;
  return html`<div class="home">
    <div class="home-head">
      <h1>${lib ? lib.name : "Model Library"}</h1>
      <p>${lib ? html`Your library is in <b>${lib.path}</b>. Models go into folders laid out by your schemas, so the library makes sense in any file manager, with or without this app.`
        : "Organise your 3D models into folders laid out by your own schemas."}</p>
    </div>
    ${s.error ? html`<p class="warn-note" role="alert">${s.error}</p>` : null}
    ${lib?.read_only ? html`<p class="warn-note" role="alert">${lib.read_only}</p>` : null}
    ${isDesktop() ? html`<div class="home-actions">
      <button type="button" class="ghost" onClick=${showLibraryFolder} disabled=${!lib}>Show in folder</button>
      <button type="button" class="ghost" onClick=${() => openLibrary()}>Open another library…</button>
      <button type="button" class="ghost" id="rescan" onClick=${() => rescan(true)} disabled=${!lib}>Read the folders again</button>
    </div>` : null}
    ${ov ? html`<div class="home-card home-counts" id="home-counts">
      <h2>In this library</h2>
      <p><a href=${routeHash("browse:all")}><b>${ov.all}</b> ${ov.all === 1 ? "model" : "models"}</a>${ov.unsorted ? html`, <a href=${routeHash("browse:unsorted")}>${ov.unsorted} unsorted</a>` : null}.
        ${ov.schemas.length ? html` Schemas: ${ov.schemas.map((sc, i) => html`${i ? ", " : ""}<a href=${routeHash(`browse:${schemaScope(sc.id)}`)} key=${sc.id}>${sc.name} (${sc.count})</a>`)}.`
          : html` No schemas yet: make one with <b>New schema…</b> in the menu, then put model folders under its folder.`}</p>
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
    <p>The library is an ordinary folder. Models are sorted into folders inside it by your schemas, with their details saved next to them, so you can move it to another drive or computer and open it again later.</p>
    <p>Choose an empty folder for a new library, or a folder that already is one.</p>
    <div class="home-actions">
      ${def ? html`<button type="button" class="primary" id="use-default" onClick=${() => openLibrary(def)}>Use ${def}</button>` : null}
      <button type="button" class=${def ? "ghost" : "primary"} id="choose-library" onClick=${() => openLibrary()}>Choose a folder…</button>
    </div>
    <p class="muted">You can open a different library at any time in Settings.</p>
  </div>`;
}
