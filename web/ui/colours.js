// The colours 3D files are drawn in (docs/PLAN.md, "Group 3 design", R9 and R10):
// the library's preview colour (gray unless chosen in Settings) or a model's own
// (`render_colour` in its model.json, so it's read back wherever the library is
// opened); a 3MF's own colours when `fileColours` is on.
import { html } from "../lib/html.js";
import { ui } from "./state.js";
import { api, followIds, loadOverview, recorded, toast } from "./library.js";

export const GRAY = "#a3a9ae";

/** The colours offered everywhere: [name, "#rrggbb"]. */
export const COLOURS = [
  ["Gray", GRAY], ["White", "#f2f2ef"], ["Black", "#2e3135"], ["Red", "#c8352b"],
  ["Blue", "#2f6fd0"], ["Green", "#3a9a4a"], ["Orange", "#e8781e"], ["Yellow", "#f2b705"],
];

export const colourName = (hex) => [...COLOURS, ...MORE_COLOURS].find(([, h]) => h === hex)?.[0] || hex;

/** More colours offered when choosing one: [name, "#rrggbb"]. */
export const MORE_COLOURS = [
  ["Silver", "#c9ced3"], ["Bone", "#e9dfc7"], ["Gold", "#c9a227"], ["Brown", "#8a5a3c"],
  ["Purple", "#7b4bb3"], ["Pink", "#d9578f"], ["Teal", "#2a9d8f"], ["Navy", "#23365e"],
];

/** Ask for a colour in the app's own colour dialog; resolves with "#rrggbb", or
 *  null when it's closed. (Not the system's picker through a hidden box: whether
 *  it was cancelled can't be told reliably, docs/PLAN.md "0.5.17".) */
export function pickColour(start = GRAY, title = "Choose a colour") {
  return new Promise((resolve) => ui.set({ dialog: { type: "colour", start, title, resolve } }));
}

/** A row of swatches in a menu, then Other colour…: `current` is ticked, `choose(hex)`. */
export function colourItems(current, choose, idPrefix = "colour", label = "Colours") {
  const known = COLOURS.some(([, h]) => h === current);
  return [
    { swatches: COLOURS.map(([name, hex]) => ({ id: `${idPrefix}-${name.toLowerCase()}`, label: name, hex, on: hex === current, run: () => choose(hex) })), label },
    { id: `${idPrefix}-other`, label: current && !known ? `Other colour (${colourName(current)})…` : "Other colour…", swatch: current && !known ? current : null, icon: current && !known ? "check" : null,
      run: async () => { const v = await pickColour(current || GRAY); if (v) choose(v); } },
  ];
}

/** The colour 3D files are drawn in when their model has none ("#rrggbb"): the
 *  library's preview colour. */
export const libraryColour = (s = ui.get()) => s.library?.preview_colour || GRAY;

/** Save a model's own colour (null: none), with Undo, and draw its preview again. */
export async function setModelColour(model, hex) {
  try {
    const v = await api("model_update", { id: model.id, patch: { render_colour: hex || "" }, journal: true });
    followIds({ [model.id]: v.id });
    recorded(hex ? `${model.name} is drawn in ${colourName(hex).toLowerCase()} now.` : `${model.name} is drawn in the usual colour again.`, v.journal, () => remakePreview(v.id || model.id));
    remakePreview(v.id || model.id); // in the background: the viewer changes at once
    return v;
  } catch (e) { toast(`Couldn't save the colour: ${e.message || e}`, 6000); return null; }
}

/** Draw the previews kept in the library for a model (or several) again, then show the cards again. */
export async function remakePreview(ids) {
  try {
    const { job } = await api("thumbs_make", { ids: [].concat(ids), force: true });
    for (let i = 0; i < 300; i++) {
      const j = await api("job", { id: job });
      if (j.done) break;
      await new Promise((r) => setTimeout(r, 200));
    }
    await loadOverview();
  } catch { /* the preview stays as it was */ }
}

/** A colour box: the usual colours and Other colour… (the colour dialog). `none`:
 *  the words for no colour, offered first (value null); `keep`: the words for
 *  leaving it as it is (value ""), for editing several models. */
export function ColourSelect({ id, value, onChange, disabled, none = null, keep = null }) {
  const known = COLOURS.some(([, h]) => h === value);
  const choice = value === "" ? "keep" : value == null ? "none" : known ? value : "other";
  return html`<span class="colour-select">${value ? html`<span class="swatch" style=${{ background: value }}></span>` : null}
    <select id=${id} value=${choice} disabled=${disabled}
      onChange=${async (e) => {
        const c = e.target.value;
        const v = c === "keep" ? "" : c === "none" ? null : c === "other" ? await pickColour(value || GRAY) : c;
        if (v !== undefined && (c !== "other" || v)) onChange(v); else e.target.value = choice;
      }}>
      ${keep ? html`<option value="keep">${keep}</option>` : null}
      ${none ? html`<option value="none">${none}</option>` : null}
      ${COLOURS.map(([name, hex]) => html`<option key=${hex} value=${hex}>${name}</option>`)}
      <option value="other">${value && !known ? `Other colour (${colourName(value)})…` : "Other colour…"}</option>
    </select></span>`;
}
