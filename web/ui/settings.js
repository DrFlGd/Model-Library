// Settings: the open library (name, folder, variant folders, other libraries),
// the theme, and the app's version with a link to the About page.
import { html, useState } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, THEMES, setTheme, setPref } from "./state.js";
import { ctx } from "./context.js";
import { isDesktop, openLibrary, renameLibrary, showLibraryFolder, setVariantFolders, setPreviews, undoable, toast } from "./library.js";
import { CommandBar } from "./layout.js";
import { GRAY, COLOURS, pickColour, colourName } from "./colours.js";

/** A colour choice: the usual colours, and Other… (the system colour picker). */
function ColourSelect({ id, value, onChange, disabled }) {
  const known = COLOURS.some(([, h]) => h === value);
  return html`<span class="colour-select"><span class="swatch" style=${{ background: value }}></span>
    <select id=${id} value=${known ? value : "other"} disabled=${disabled}
      onChange=${async (e) => { const v = e.target.value === "other" ? await pickColour(value) : e.target.value; if (v) onChange(v); else e.target.value = known ? value : "other"; }}>
      ${COLOURS.map(([name, hex]) => html`<option key=${hex} value=${hex}>${name}</option>`)}
      <option value="other">${known ? "Other colour…" : `Other colour (${value})…`}</option>
    </select></span>`;
}

/** How the library's previews are drawn: their colour, and 3MF pictures (R10, R11). */
function Previews({ lib }) {
  const colour = lib.preview_colour || GRAY;
  const pictures = lib.use_3mf_pictures !== false;
  const [busy, setBusy] = useState(false);
  const off = !!lib.read_only || busy;
  const save = async (c, p, said) => {
    setBusy(true);
    try {
      const r = await setPreviews(c === GRAY ? null : c, p);
      toast(`${said} Made ${r.made || 0} ${r.made === 1 ? "preview" : "previews"} again.`, 6000);
    } catch (e) { toast(`Couldn't make the previews again: ${e.message || e}`, 6000); }
    setBusy(false);
  };
  return html`<div class="field-block" id="settings-previews"><span class="field-label">Previews</span>
    <small class="muted">The pictures on the cards are kept in the library, so these are the library's settings. Changing one makes every preview again (Stop is in the status bar). A model with a colour of its own keeps it.</small>
    <label class="field-row"><span>Colour</span><${ColourSelect} id="preview-colour" value=${colour} disabled=${off}
      onChange=${(c) => save(c, pictures, `Previews are drawn in ${colourName(c).toLowerCase()}.`)} /></label>
    <label class="check-row"><input type="checkbox" id="use-3mf-pictures" checked=${pictures} disabled=${off}
      onChange=${(e) => save(colour, e.target.checked, e.target.checked ? "3MF files' own pictures are used." : "3MF files are drawn like the others.")} />
      <span>Use the picture saved in a 3MF file (by the slicer) as its preview</span></label>
  </div>`;
}

function RenameForm({ lib }) {
  const [name, setName] = useState(lib.name || "");
  const [error, setError] = useState("");
  const submit = async (e) => {
    e.preventDefault();
    setError("");
    try {
      const before = lib.name;
      await renameLibrary(name);
      undoable(`Renamed the library ${name.trim()}.`, async () => { await renameLibrary(before); setName(before); });
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
  /** Save the names, with Undo in the message (`next` null: the defaults). */
  const save = async (next, said) => {
    setError("");
    const before = [...names];
    try {
      await setVariantFolders(next);
      undoable(said, () => setVariantFolders(before));
    } catch (err) { setError(err.message || String(err)); }
  };
  const add = async (e) => {
    e.preventDefault();
    const n = text.trim();
    if (!n) return;
    if (names.some((x) => x.toLowerCase() === n.toLowerCase())) { setError(`${n} is already in the list.`); return; }
    await save([...names, n], `Added ${n} to the variant folders.`);
    setText("");
  };
  return html`<div class="field-block" id="settings-variants"><span class="field-label">Variant folders</span>
    <small class="muted">A folder with one of these names, or with one of them as whole words (such as "Resin 32mm"), is a variant of the model rather than a part. A model's page then shows a switch between its variants. Case, spaces and dashes don't matter.</small>
    <ul class="chip-list" id="variant-names">${names.map((n) => html`<li key=${n} class="chip-item"><span>${n}</span>
      <button type="button" class="chip-x" aria-label=${`Remove ${n}`} title=${`Remove ${n}`} disabled=${off} onClick=${() => save(names.filter((x) => x !== n), `Removed ${n} from the variant folders.`)}>×</button></li>`)}</ul>
    <form class="inline-form" onSubmit=${add}>
      <input id="variant-add" type="text" maxlength="60" placeholder="Add a folder name" value=${text} onInput=${(e) => setText(e.target.value)} aria-label="New variant folder name" disabled=${off} />
      <button type="submit" class="ghost" id="variant-add-btn" disabled=${off || !text.trim()}>Add</button>
      <button type="button" class="ghost" id="variant-reset" disabled=${off} onClick=${() => save(null, "Set the variant folders back to the defaults.")}>Reset to the defaults</button>
    </form>
    ${error ? html`<p class="form-error" role="alert">${error}</p>` : null}
  </div>`;
}

export function Settings() {
  const s = useStore(ui, (st) => ({ library: st.library, recent: st.recent, theme: st.theme, viewColour: st.viewColour, fileColours: st.fileColours }));
  const lib = s.library;
  const others = (s.recent || []).filter((p) => p !== lib?.path);
  return html`<${CommandBar} id="settings-head" title="Settings" /><div class="pages"><div class="library-settings">
    <section class="ls-section" id="settings-library">
      <h2>Library</h2>
      ${lib ? html`
        <div class="field-block"><label class="field-label" for="library-name">Name</label>
          <${RenameForm} lib=${lib} key=${lib.path} /></div>
        <div class="field-block"><span class="field-label">Folder</span>
          <div class="pick-row"><span class="pick-path" id="library-path">${lib.path}</span>
            ${isDesktop() ? html`<button type="button" class="ghost" onClick=${showLibraryFolder}>Show in folder</button>
              <button type="button" class="ghost" id="open-library" title="Open a library, or make a new one in an empty folder" onClick=${() => openLibrary()}>Open another library…</button>` : null}</div>
          <small class="muted">Everything about your models is kept in this folder, so you can move, copy or sync it and open it again here.</small>
        </div>
        <${VariantFolders} lib=${lib} />
        <${Previews} lib=${lib} />` : html`<p class="muted">No library is open.</p>`}
      ${!lib && isDesktop() ? html`<div><button type="button" class="ghost" id="open-library" title="Open a library, or make a new one in an empty folder" onClick=${() => openLibrary()}>Open another library…</button></div>` : null}
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
      <div class="field-block" id="settings-view-colour"><span class="field-label">3D colour</span>
        <small class="muted">The colour 3D files are drawn in on this computer (the viewer's Colour ▾ menu changes it too). A model with a colour of its own is drawn in that.</small>
        <label class="field-row"><span>Colour</span><${ColourSelect} id="view-colour" value=${s.viewColour || GRAY} onChange=${(c) => setPref({ viewColour: c === GRAY ? null : c })} /></label>
        <label class="check-row"><input type="checkbox" id="file-colours" checked=${!!s.fileColours} onChange=${(e) => setPref({ fileColours: e.target.checked })} />
          <span>Show a 3MF file's own colours when it has some</span></label></div>
    </section>
    <section class="ls-section" id="settings-about">
      <h2>About</h2>
      <p>Model Library ${ctx.platform?.info?.version || ""}</p>
      <p><a href="#/about" id="settings-about-link">Versions, what's changed, what's coming and licences</a></p>
    </section>
  </div></div>`;
}
