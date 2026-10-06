// Inline stroke icons (no icon font, no network).
import { html } from "../lib/html.js";

const svg = (d, size = 16) => html`<svg width=${size} height=${size} viewBox="0 0 24 24" fill="none" stroke="currentColor"
  stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${d}</svg>`;

export const Icon = {
  search: (s) => svg(html`<circle cx="11" cy="11" r="7" /><path d="M20 20l-3.5-3.5" />`, s),
  grid: (s) => svg(html`<rect x="3" y="3" width="7" height="7" rx="1.5" /><rect x="14" y="3" width="7" height="7" rx="1.5" /><rect x="3" y="14" width="7" height="7" rx="1.5" /><rect x="14" y="14" width="7" height="7" rx="1.5" />`, s),
  list: (s) => svg(html`<rect x="3" y="4" width="5" height="4" rx="1" /><rect x="3" y="10" width="5" height="4" rx="1" /><rect x="3" y="16" width="5" height="4" rx="1" /><path d="M11 6h10M11 12h10M11 18h10" />`, s),
  table: (s) => svg(html`<rect x="3" y="4" width="18" height="16" rx="1.5" /><path d="M3 9h18M3 14h18M9 9v11" />`, s),
  grouped: (s) => svg(html`<path d="M3 5h8M3 12h8M3 19h8" /><rect x="14" y="3" width="7" height="4" rx="2" /><rect x="14" y="10" width="7" height="4" rx="2" /><rect x="14" y="17" width="7" height="4" rx="2" />`, s),
  star: (s, filled) => html`<svg width=${s || 16} height=${s || 16} viewBox="0 0 24 24" fill=${filled ? "currentColor" : "none"} stroke="currentColor" stroke-width="1.9" stroke-linejoin="round" aria-hidden="true"><path d="M12 3.5l2.6 5.3 5.9.9-4.3 4.1 1 5.8-5.2-2.7-5.2 2.7 1-5.8L3.5 9.7l5.9-.9z" /></svg>`,
  eye: (s) => svg(html`<path d="M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12z" /><circle cx="12" cy="12" r="3" />`, s),
  moon: (s) => svg(html`<path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z" />`, s),
  sun: (s) => svg(html`<circle cx="12" cy="12" r="4" /><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" />`, s),
  night: (s) => svg(html`<path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z" /><path d="M17 3l.6 1.4L19 5l-1.4.6L17 7l-.6-1.4L15 5l1.4-.6z" />`, s),
  menu: (s) => svg(html`<path d="M4 6h16M4 12h16M4 18h16" />`, s),
  close: (s) => svg(html`<path d="M6 6l12 12M18 6L6 18" />`, s),
  chevron: (s) => svg(html`<path d="M9 6l6 6-6 6" />`, s),
  book: (s) => svg(html`<path d="M4 5a2 2 0 0 1 2-2h13v16H6a2 2 0 0 0-2 2z" /><path d="M4 21V5" />`, s),
  file: (s) => svg(html`<path d="M14 3H6v18h12V7z" /><path d="M14 3v4h4" />`, s),
  chevronsLeft: (s) => svg(html`<path d="M11 17l-5-5 5-5M18 17l-5-5 5-5" />`, s),
  chevronsRight: (s) => svg(html`<path d="M13 17l5-5-5-5M6 17l5-5-5-5" />`, s),
  panel: (s) => svg(html`<rect x="3" y="4" width="18" height="16" rx="1.5" /><path d="M15 4v16" />`, s),
  home: (s) => svg(html`<path d="M3 11l9-7 9 7" /><path d="M5 10v10h14V10" />`, s),
  clock: (s) => svg(html`<circle cx="12" cy="12" r="9" /><path d="M12 7v5l3 2" />`, s),
  box: (s) => svg(html`<path d="M3 7l9-4 9 4-9 4-9-4z" /><path d="M3 7v10l9 4 9-4V7" /><path d="M12 11v10" />`, s),
  alert: (s) => svg(html`<path d="M12 3l10 18H2z" /><path d="M12 10v5M12 18v.5" />`, s),
  gear: (s) => svg(html`<circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" />`, s),
  stack: (s) => svg(html`<rect x="6" y="3" width="15" height="15" rx="2" /><path d="M3 7v12a2 2 0 0 0 2 2h12" />`, s),
  plus: (s) => svg(html`<path d="M12 5v14M5 12h14" />`, s),
  edit: (s) => svg(html`<path d="M4 20h4L19 9l-4-4L4 16z" /><path d="M14 6l4 4" />`, s),
  download: (s) => svg(html`<path d="M12 4v11M7 10l5 5 5-5M5 20h14" />`, s),
  cog: (s) => svg(html`<circle cx="12" cy="12" r="2.5" /><path d="M10.5 3h3l.5 2.6 1.9.8 2.2-1.5 2.1 2.1-1.5 2.2.8 1.9 2.5.4v3l-2.5.5-.8 1.9 1.5 2.2-2.1 2.1-2.2-1.5-1.9.8-.5 2.5h-3l-.5-2.5-1.9-.8-2.2 1.5-2.1-2.1 1.5-2.2-.8-1.9L3 13.5v-3l2.6-.5.8-1.9-1.5-2.2 2.1-2.1 2.2 1.5 1.9-.8z" />`, s),
  pin: (s) => svg(html`<path d="M9 3h6l-1 6 3 3v2H7v-2l3-3z" /><path d="M12 14v7" />`, s),
  folder: (s) => svg(html`<path d="M3 6a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />`, s),
  refresh: (s) => svg(html`<path d="M20 11a8 8 0 1 0-2.3 5.7" /><path d="M20 4v7h-7" />`, s),
  image: (s) => svg(html`<rect x="3" y="4" width="18" height="16" rx="2" /><circle cx="9" cy="10" r="2" /><path d="M21 16l-5-5-8 8" />`, s),
  layers: (s) => svg(html`<path d="M3 8.5l9-4.5 9 4.5-9 4.5z" /><path d="M3 12.5l9 4.5 9-4.5" /><path d="M3 16.5l9 4.5 9-4.5" />`, s),
  code: (s) => svg(html`<path d="M8 7l-5 5 5 5M16 7l5 5-5 5M14 4l-4 16" />`, s),
};
