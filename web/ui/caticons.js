// Icons for categories and subcategories (docs/PLAN.md, "Group 2 design", R4):
// the built-in set, drawn like icons.js, and a picture of your own kept in the
// library. A category file stores "builtin:<name>" or "file:_library/icons/<file>".
import { html } from "../lib/html.js";
import { Icon } from "./icons.js";
import { libraryUrl } from "./library.js";

const svg = (d, size = 16) => html`<svg width=${size} height=${size} viewBox="0 0 24 24" fill="none" stroke="currentColor"
  stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${d}</svg>`;

/** name: [label, words to search by, drawing] */
export const BUILTIN = {
  folder: ["Folder", "files", html`<path d="M3 6a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />`],
  layers: ["Layers", "category stack", html`<path d="M3 8.5l9-4.5 9 4.5-9 4.5z" /><path d="M3 12.5l9 4.5 9-4.5" /><path d="M3 16.5l9 4.5 9-4.5" />`],
  cube: ["Cube", "model box 3d", html`<path d="M3 7l9-4 9 4-9 4-9-4z" /><path d="M3 7v10l9 4 9-4V7" /><path d="M12 11v10" />`],
  house: ["House", "home building", html`<path d="M3 11l9-7 9 7" /><path d="M5 10v10h14V10" /><path d="M10 20v-6h4v6" />`],
  castle: ["Castle", "fort tower fantasy terrain", html`<path d="M4 21V9h3V6h2v3h2V6h2v3h2V6h2v3h3v12z" /><path d="M10 21v-4a2 2 0 0 1 4 0v4" />`],
  tree: ["Tree", "nature forest terrain plant", html`<path d="M12 3l6 8h-3l4 6H5l4-6H6z" /><path d="M12 17v4" />`],
  flower: ["Flower", "plant garden pot", html`<circle cx="12" cy="9" r="2.5" /><path d="M12 4.5a2.5 2.5 0 0 1 0 2M12 11.5v9M7 17c2 0 5 1 5 3.5M17 15c-2 0-5 1.5-5 5" /><path d="M9.6 7.5a2.5 2.5 0 1 1 1-3M14.4 7.5a2.5 2.5 0 1 0-1-3" />`],
  leaf: ["Leaf", "plant nature", html`<path d="M5 19c0-9 5-14 15-15-1 10-6 15-15 15z" /><path d="M5 19l8-8" />`],
  mountain: ["Mountain", "terrain landscape rock", html`<path d="M2 20l7-12 4 6 3-4 6 10z" /><path d="M7.5 10.5l1.5 1.5 1.5-1.5" />`],
  skull: ["Skull", "monster undead horror", html`<path d="M12 3a8 8 0 0 0-5 14.2V20h10v-2.8A8 8 0 0 0 12 3z" /><circle cx="9" cy="11" r="1.5" /><circle cx="15" cy="11" r="1.5" /><path d="M10 20v-2M14 20v-2" />`],
  sword: ["Sword", "weapon fantasy cosplay", html`<path d="M14 4h6v6L9 21l-2-2-2 2-2-2 2-2-2-2z" /><path d="M5 13l6 6" />`],
  shield: ["Shield", "armour defence", html`<path d="M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6z" />`],
  helmet: ["Helmet", "armour cosplay knight", html`<path d="M4 15a8 8 0 0 1 16 0v4H4z" /><path d="M9 15h10M12 7v5" />`],
  figure: ["Figure", "miniature person character", html`<circle cx="12" cy="5" r="2.5" /><path d="M8 21l1-7-3-3 3-3h6l3 3-3 3 1 7" /><path d="M9 21h6" />`],
  robot: ["Robot", "mech sci-fi", html`<rect x="5" y="8" width="14" height="11" rx="2" /><circle cx="9.5" cy="13" r="1.3" /><circle cx="14.5" cy="13" r="1.3" /><path d="M12 8V4M10 4h4M3 12v3M21 12v3" />`],
  rocket: ["Rocket", "space sci-fi", html`<path d="M12 3c3 2 5 6 4 11l-4 3-4-3c-1-5 1-9 4-11z" /><circle cx="12" cy="10" r="1.5" /><path d="M8 14l-3 3 4 1M16 14l3 3-4 1M11 20l1 2 1-2" />`],
  dragon: ["Dragon", "monster creature fantasy", html`<path d="M4 18c3 0 4-3 4-6 2 2 4 2 6 1l2 3h4l-2-5 2-2-4-1-3-4-1 3c-3 0-6 1-8 4" /><path d="M14 8l-1-3" />`],
  paw: ["Paw", "animal pet dog cat", html`<circle cx="7" cy="9" r="1.8" /><circle cx="11" cy="6" r="1.8" /><circle cx="15.5" cy="7" r="1.8" /><circle cx="18" cy="11" r="1.8" /><path d="M7 17c0-3 3-5 5-5s5 3 5 5-2 3-5 2-5 1-5-2z" />`],
  fish: ["Fish", "animal water sea", html`<path d="M3 12c3-4 8-5 12-3l5-3v12l-5-3c-4 2-9 1-12-3z" /><circle cx="8" cy="11" r=".6" />`],
  bird: ["Bird", "animal", html`<path d="M4 15c4 0 6-2 7-5a4 4 0 0 1 8-1l2 1-2 1c0 5-4 9-11 9" /><path d="M9 16l-2 4" />`],
  car: ["Car", "vehicle auto", html`<path d="M3 16v-3l2-5h14l2 5v3z" /><circle cx="7.5" cy="16.5" r="1.8" /><circle cx="16.5" cy="16.5" r="1.8" /><path d="M5 13h14" />`],
  plane: ["Plane", "aircraft vehicle", html`<path d="M21 4L3 11l7 2 2 7 3-5 4 3z" /><path d="M10 13l11-9" />`],
  boat: ["Boat", "ship sea vehicle", html`<path d="M3 15h18l-3 5H6z" /><path d="M12 15V3l6 9h-6" />`],
  train: ["Train", "rail vehicle", html`<rect x="6" y="3" width="12" height="13" rx="3" /><path d="M6 10h12M9 20l-1 1M15 20l1 1M9 16v4M15 16v4" />`],
  dice: ["Dice", "games board tabletop", html`<rect x="4" y="4" width="16" height="16" rx="3" /><circle cx="9" cy="9" r="1" /><circle cx="15" cy="15" r="1" /><circle cx="15" cy="9" r="1" /><circle cx="9" cy="15" r="1" /><circle cx="12" cy="12" r="1" />`],
  pawn: ["Pawn", "chess board games", html`<circle cx="12" cy="6" r="2.5" /><path d="M10 9h4l1 7H9z" /><path d="M7 20h10l-1-4H8z" />`],
  puzzle: ["Puzzle", "toy game", html`<path d="M4 8h4a2 2 0 1 1 4 0h4v4a2 2 0 1 1 0 4v4h-4a2 2 0 1 0-4 0H4z" />`],
  gift: ["Gift", "toy present", html`<rect x="4" y="9" width="16" height="11" rx="1" /><path d="M3 9h18v-2H3zM12 7v13" /><path d="M12 7C10 3 7 4 8 7M12 7c2-4 5-3 4 0" />`],
  wrench: ["Wrench", "tool workshop repair", html`<path d="M14.5 4a5 5 0 0 0-4.6 6.9L4 16.8 7.2 20l5.9-5.9A5 5 0 0 0 20 9.5l-3 1.5-2.5-2.5L16 5.5a5 5 0 0 0-1.5-1.5z" />`],
  hammer: ["Hammer", "tool workshop", html`<path d="M14 6l-3-3H7l-3 3 4 4 3-3 3 3 2-2z" /><path d="M13 9l7 7-2 2-7-7" />`],
  gear: ["Gear", "mechanical part", html`<circle cx="12" cy="12" r="3" /><path d="M12 2v3M12 19v3M4.9 4.9l2.1 2.1M17 17l2.1 2.1M2 12h3M19 12h3M4.9 19.1L7 17M17 7l2.1-2.1" />`],
  bolt: ["Bolt", "screw hardware fastener", html`<path d="M9 3h6l2 3-2 3H9L7 6z" /><path d="M10 9v12h4V9M10 13h4M10 17h4" />`],
  plug: ["Plug", "electronics power cable", html`<path d="M8 3v5M16 3v5M6 8h12v3a6 6 0 0 1-12 0z" /><path d="M12 17v4" />`],
  chip: ["Chip", "electronics computer", html`<rect x="6" y="6" width="12" height="12" rx="1.5" /><rect x="9.5" y="9.5" width="5" height="5" /><path d="M9 3v3M15 3v3M9 18v3M15 18v3M3 9h3M3 15h3M18 9h3M18 15h3" />`],
  phone: ["Phone", "device stand mobile", html`<rect x="7" y="2.5" width="10" height="19" rx="2" /><path d="M11 18.5h2" />`],
  cup: ["Cup", "kitchen mug drink", html`<path d="M5 8h11v7a4 4 0 0 1-4 4H9a4 4 0 0 1-4-4z" /><path d="M16 10h2a2 2 0 0 1 0 4h-2M8 3v2M12 3v2" />`],
  lamp: ["Lamp", "light desk office", html`<path d="M8 3h8l3 7H5z" /><path d="M12 10v8M8 21h8" />`],
  chair: ["Chair", "furniture living room", html`<path d="M7 3h10v8H7zM5 11h14v3H5z" /><path d="M7 14v7M17 14v7" />`],
  pencil: ["Pencil", "office desk stationery", html`<path d="M4 20h4L19 9l-4-4L4 16z" /><path d="M14 6l4 4" />`],
  heart: ["Heart", "favourite love gift", html`<path d="M12 20s-8-5-8-11a4.5 4.5 0 0 1 8-2.8A4.5 4.5 0 0 1 20 9c0 6-8 11-8 11z" />`],
  star: ["Star", "favourite", html`<path d="M12 3.5l2.6 5.3 5.9.9-4.3 4.1 1 5.8-5.2-2.7-5.2 2.7 1-5.8L3.5 9.7l5.9-.9z" />`],
  crown: ["Crown", "king royal fantasy", html`<path d="M3 8l4 4 5-7 5 7 4-4-2 11H5z" />`],
  music: ["Music", "instrument note", html`<path d="M9 18V5l11-2v13" /><circle cx="6.5" cy="18" r="2.5" /><circle cx="17.5" cy="16" r="2.5" />`],
  printer: ["Printer", "3d printer parts upgrades", html`<rect x="4" y="3" width="16" height="18" rx="1" /><path d="M4 7h16M9 7v4h6V7M8 17h8" />`],
  spool: ["Spool", "filament printer", html`<circle cx="12" cy="12" r="9" /><circle cx="12" cy="12" r="3" /><path d="M12 3v6M12 15v6" />`],
};

const builtinName = (icon) => (typeof icon === "string" && icon.startsWith("builtin:") ? icon.slice(8) : null);

/** A category's or subcategory's icon: built-in, its own picture, or `fallback` (an Icon name, or none). */
export function CatIcon({ icon, size = 15, fallback = null }) {
  const b = builtinName(icon);
  if (b && BUILTIN[b]) return html`<span class="cat-icon" data-icon=${icon}>${svg(BUILTIN[b][2], size)}</span>`;
  if (typeof icon === "string" && icon.startsWith("file:")) {
    return html`<span class="cat-icon" data-icon=${icon}><img src=${libraryUrl(icon.slice(5))} alt="" width=${size} height=${size} /></span>`;
  }
  return fallback ? html`<span class="cat-icon">${Icon[fallback](size)}</span>` : null;
}

/** The icon stored for a schema's node (path [] is the category itself). */
export const iconOf = (schema, path = []) => schema?.icons?.[path.join("/")] || null;
