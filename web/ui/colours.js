// The colours 3D files are drawn in (docs/PLAN.md, "Group 3 design", R9 and R10):
// gray unless you choose another, on this computer (`viewColour`) or for one model
// (`render_colour` in its model.json); a 3MF's own colours when `fileColours` is on.
import { ui, setPref } from "./state.js";
import { api, followIds, loadOverview, recorded, toast } from "./library.js";

export const GRAY = "#a3a9ae";

/** The colours offered everywhere: [name, "#rrggbb"]. */
export const COLOURS = [
  ["Gray", GRAY], ["White", "#f2f2ef"], ["Black", "#2e3135"], ["Red", "#c8352b"],
  ["Blue", "#2f6fd0"], ["Green", "#3a9a4a"], ["Orange", "#e8781e"], ["Yellow", "#f2b705"],
];

export const colourName = (hex) => COLOURS.find(([, h]) => h === hex)?.[0] || hex;

/** Ask for a colour with the system's colour picker; resolves with "#rrggbb" or null. */
export function pickColour(start = GRAY) {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "color";
    input.value = start;
    input.style.cssText = "position:fixed;left:-100px;top:0;opacity:0";
    document.body.append(input);
    let done = false;
    const finish = (v) => { if (done) return; done = true; input.remove(); resolve(v); };
    input.addEventListener("change", () => finish(input.value.toLowerCase()));
    input.addEventListener("blur", () => setTimeout(() => finish(null), 300));
    input.click();
  });
}

/** Menu items to choose one of the colours: `current` is ticked; `choose(hex)`. */
export function colourItems(current, choose, idPrefix = "colour") {
  return [
    ...COLOURS.map(([name, hex]) => ({ id: `${idPrefix}-${name.toLowerCase()}`, label: name, swatch: hex, icon: hex === current ? "check" : null, run: () => choose(hex) })),
    { id: `${idPrefix}-other`, label: "Other colour…", swatch: COLOURS.some(([, h]) => h === current) ? null : current, icon: current && !COLOURS.some(([, h]) => h === current) ? "check" : null,
      run: async () => { const v = await pickColour(current || GRAY); if (v) choose(v); } },
  ];
}

/** This computer's colour for 3D files ("#rrggbb"). */
export const viewColour = () => ui.get().viewColour || GRAY;
export const setViewColour = (hex) => setPref({ viewColour: hex === GRAY ? null : hex });

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

async function remakePreview(id) {
  try {
    const { job } = await api("thumbs_make", { ids: [id], force: true });
    for (let i = 0; i < 300; i++) {
      const j = await api("job", { id: job });
      if (j.done) break;
      await new Promise((r) => setTimeout(r, 200));
    }
    await loadOverview();
  } catch { /* the preview stays as it was */ }
}
