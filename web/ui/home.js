// Home: the open library, how its folders will be laid out, and what's coming.
import { html } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui } from "./state.js";
import { isDesktop, openLibrary, showLibraryFolder } from "./library.js";

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
  ["Phase 0", "The app, its library folder and settings", true],
  ["Phase 1", "Schemas, model details and search"],
  ["Phase 2", "Importing models: files, folders and ZIPs, moved or copied into place"],
  ["Phase 3", "Viewing models, their parts, pictures, PDFs and videos"],
  ["Phase 4", "Editing schemas and categories, with folders moved to match"],
];

export function Home() {
  const s = useStore(ui, (st) => ({ library: st.library, error: st.libraryError }));
  const lib = s.library;
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

const BROWSE = {
  all: ["All models", "Every model in the library will be listed here, with search and filters by schema, category, author and tags."],
  unsorted: ["Unsorted", "Models imported without a schema land in the Unsorted folder, ready to be sorted later."],
  favs: ["Favourites", "Models you star will be listed here."],
};

/** A place in the library, before there are models to list. */
export function BrowseEmpty() {
  const route = useStore(ui, (st) => st.route);
  const [title, text] = BROWSE[route.slice(7)] || BROWSE.all;
  return html`<div class="browse-empty">
    <h1>${title}</h1>
    <p>No models yet. ${text}</p>
    <p>Importing comes in Phase 2; until then the library is an empty folder you can look at in your file manager.</p>
  </div>`;
}
