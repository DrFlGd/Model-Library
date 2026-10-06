// Settings: the open library (name, folder, other libraries), the theme, and
// the app's version.
import { html, useState } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, THEMES, setTheme } from "./state.js";
import { ctx } from "./context.js";
import { isDesktop, openLibrary, renameLibrary, showLibraryFolder, setVariantFolders, toast } from "./library.js";

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

/** The folder names that make a model's variants (kept in the library). */
function VariantFolders({ lib }) {
  const names = lib.variant_folders || [];
  const [text, setText] = useState("");
  const [error, setError] = useState("");
  const off = !!lib.read_only;
  const save = async (next) => {
    setError("");
    try { await setVariantFolders(next); } catch (err) { setError(err.message || String(err)); }
  };
  const add = async (e) => {
    e.preventDefault();
    const n = text.trim();
    if (!n) return;
    if (names.some((x) => x.toLowerCase() === n.toLowerCase())) { setError(`${n} is already in the list.`); return; }
    await save([...names, n]);
    setText("");
  };
  return html`<div class="field-block" id="settings-variants"><span class="field-label">Variant folders</span>
    <small class="muted">A folder with one of these names, or with one of them as whole words (such as "Resin 32mm"), is a variant of the model rather than a part. A model's page then shows a switch between its variants. Case, spaces and dashes don't matter.</small>
    <ul class="chip-list" id="variant-names">${names.map((n) => html`<li key=${n} class="chip-item"><span>${n}</span>
      <button type="button" class="chip-x" aria-label=${`Remove ${n}`} title="Remove" disabled=${off} onClick=${() => save(names.filter((x) => x !== n))}>×</button></li>`)}</ul>
    <form class="inline-form" onSubmit=${add}>
      <input id="variant-add" type="text" maxlength="60" placeholder="Add a folder name" value=${text} onInput=${(e) => setText(e.target.value)} aria-label="New variant folder name" disabled=${off} />
      <button type="submit" class="ghost" id="variant-add-btn" disabled=${off || !text.trim()}>Add</button>
      <button type="button" class="ghost" id="variant-reset" disabled=${off} onClick=${() => save(null)}>Reset to the defaults</button>
    </form>
    ${error ? html`<p class="form-error" role="alert">${error}</p>` : null}
  </div>`;
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
        </div>
        <${VariantFolders} lib=${lib} />` : html`<p class="muted">No library is open.</p>`}
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
