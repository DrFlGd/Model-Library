// One page layout (docs/PLAN.md, "UI pass design", step 3): the header every page
// starts with, the toolbar's view switcher and sort menu, the note that says a
// search is on, and the box the details panels edit in.
import { html, useState, useLayoutEffect } from "../lib/html.js";
import { Icon } from "./icons.js";
import { openMenu } from "./actions.js";

/** A page's header: the title and its count on the left; the page's main button
 *  (`children`) and a menu (`menu`: More ▾, or a named one such as Library ▾) on
 *  the right. `sub`: a line under the title. */
export function PageHead({ id, title, count, sub, menu, children }) {
  return html`<header class="page-head" id=${id}>
    <div class="page-title">
      <div class="page-title-line">${typeof title === "string" ? html`<h1>${title}</h1>` : title}
        ${count == null ? null : typeof count === "string" || typeof count === "number" ? html`<span class="page-count">${count}</span>` : count}</div>
      ${sub ? html`<p class="page-sub">${sub}</p>` : null}
    </div>
    ${children || menu ? html`<div class="page-actions">${children}${menu ? html`<${MenuButton} ...${menu} />` : null}</div>` : null}
  </header>`;
}

/** A button that opens a menu under it. `items`: menu items, or a function that
 *  makes them when it's pressed. */
export function MenuButton({ id, label = "More", icon = "more", title, items, disabled }) {
  return html`<button type="button" class="ghost menu-btn" id=${id} aria-haspopup="menu" title=${title || null} disabled=${disabled}
    onClick=${(e) => openMenu(e.currentTarget, typeof items === "function" ? items() : items)}>${icon && Icon[icon] ? Icon[icon](15) : null} ${label} ▾</button>`;
}

/** The view switcher: an icon and a label on each choice (the label goes in a
 *  narrow window; the tooltip stays). `views`: [[id, label, icon]]. */
export function ViewSwitch({ views, value, onChange, label = "View", id }) {
  return html`<div class="seg view-switch" role="group" aria-label=${label} id=${id}>${views.map(([k, l, icon]) =>
    html`<button type="button" key=${k} data-view=${k} aria-pressed=${value === k ? "true" : "false"} title=${l} onClick=${() => onChange(k)}>${icon && Icon[icon] ? Icon[icon](14) : null}<span class="view-label">${l}</span></button>`)}</div>`;
}

/** The sort menu: "Sort: Name ▾", with the same words everywhere. `options`:
 *  [[id, label]]; the menu's items are `sort-<id>`. */
export function SortMenu({ options, value, onChange, id }) {
  const cur = options.find(([k]) => k === value) || options[0];
  const items = () => options.map(([k, l]) => ({ id: `sort-${k}`, label: l, icon: k === cur[0] ? "check" : null, run: () => onChange(k) }));
  return html`<button type="button" class="ghost sort-btn" id=${id} data-sort=${cur[0]} aria-haspopup="menu" title="Sort"
    onClick=${(e) => openMenu(e.currentTarget, items())}>${Icon.sort(14)} Sort: ${cur[1]} ▾</button>`;
}

/** Above the results while a search is on, so a place that shows little or
 *  nothing doesn't look empty or broken: "Searching for … ×". */
export function SearchNote({ q, found, onClear, wider }) {
  if (!q) return null;
  return html`<p class="search-note" id="search-note" role="status">
    <span>Searching for <b>${q}</b>${found ? html` <span class="muted">· ${found}</span>` : null}</span>
    ${wider || null}
    <button type="button" class="ghost search-clear" id="search-clear" title="Clear the search" aria-label="Clear the search" onClick=${onClear}>${Icon.close(12)} Clear</button></p>`;
}

/** A box in a details panel, saved when you leave it (or press Enter); Esc puts
 *  it back. What's typed stays while the panel is drawn again; a new value from
 *  outside replaces it. `caption`: a label shown before it. */
export function EditBox({ id, cls = "", label, caption, title, placeholder, value, off, onSave }) {
  const [v, setV] = useState(value || "");
  // before the next paint: after it, text typed as the box appeared was put back
  useLayoutEffect(() => { setV(value || ""); }, [value]);
  const key = (e) => {
    if (e.key === "Escape") { e.stopPropagation(); e.currentTarget.value = value || ""; setV(value || ""); e.currentTarget.blur(); }
    else if (e.key === "Enter") e.currentTarget.blur();
  };
  const box = html`<input type="text" class=${`edit-box ${cls}`} id=${id} aria-label=${label} title=${title || label} placeholder=${placeholder} value=${v} disabled=${off}
    onInput=${(e) => setV(e.target.value)} onKeyDown=${key} onChange=${(e) => onSave(e.target.value.trim())} />`;
  return caption ? html`<label class="edit-row"><span class="edit-cap">${caption}</span>${box}</label>` : box;
}
