// Settings: the open library (name, folder, other libraries), the theme, and
// the app's version.
import { html, useState } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, THEMES, setTheme } from "./state.js";
import { ctx } from "./context.js";
import { isDesktop, openLibrary, renameLibrary, showLibraryFolder, toast } from "./library.js";

function RenameForm({ lib }) {
  const [name, setName] = useState(lib.name || "");
  const [error, setError] = useState("");
  const submit = async (e) => {
    e.preventDefault();
    setError("");
    try {
      await renameLibrary(name);
      toast("Library renamed.");
    } catch (err) {
      setError(err.message || String(err));
    }
  };
  return html`<form class="inline-form" onSubmit=${submit}>
    <input id="library-name" type="text" maxlength="80" value=${name} onInput=${(e) => setName(e.target.value)} aria-label="Library name" disabled=${!!lib.read_only} />
    <button type="submit" class="ghost" disabled=${!!lib.read_only || name.trim() === lib.name}>Rename</button>
    ${error ? html`<p class="form-error" role="alert">${error}</p>` : null}
  </form>`;
}

export function Settings() {
  const s = useStore(ui, (st) => ({ library: st.library, recent: st.recent, theme: st.theme }));
  const lib = s.library;
  const others = (s.recent || []).filter((p) => p !== lib?.path);
  return html`<div class="pages"><div class="library-settings">
    <h1>Settings</h1>
    <section class="ls-section" id="settings-library">
      <h2>Library</h2>
      ${lib ? html`
        <div class="field-block"><label class="field-label" for="library-name">Name</label>
          <${RenameForm} lib=${lib} key=${lib.path} /></div>
        <div class="field-block"><span class="field-label">Folder</span>
          <div class="pick-row"><span class="pick-path" id="library-path">${lib.path}</span>
            ${isDesktop() ? html`<button type="button" class="ghost" onClick=${showLibraryFolder}>Show in folder</button>` : null}</div>
          <small class="muted">Everything about your models is kept in this folder, so you can move, copy or sync it and open it again here.</small>
        </div>` : html`<p class="muted">No library is open.</p>`}
      ${isDesktop() ? html`<div><button type="button" class="ghost" id="open-library" onClick=${() => openLibrary()}>Open or create another library…</button></div>` : null}
      ${others.length ? html`<h3>Libraries opened before</h3>
        <ul class="ls-list" id="recent-libraries">${others.map((p) => html`<li key=${p}><span>${p}</span>
          <button type="button" class="ghost" onClick=${() => openLibrary(p)}>Open</button></li>`)}</ul>` : null}
    </section>
    <section class="ls-section">
      <h2>Appearance</h2>
      <label class="field-block"><span>Theme</span>
        <select id="theme-select" value=${s.theme} onChange=${(e) => setTheme(e.target.value)}>
          ${THEMES.map(([v, label]) => html`<option value=${v} key=${v}>${label}</option>`)}
        </select></label>
    </section>
    <section class="ls-section">
      <h2>About</h2>
      <p>Model Library ${ctx.platform?.info?.version || ""}</p>
    </section>
  </div></div>`;
}
