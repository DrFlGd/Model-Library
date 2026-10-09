// The same actions everywhere (docs/PLAN.md, "UI pass design"): one list for
// models, drawn as a row of buttons (the details panel, the model page) and as a
// menu (right-click, More ▾), so the names, icons, keys and order can't drift
// apart. Also the menu itself and the keys: one handler for the window, and each
// page says what its keys do.
import { html, useState, useEffect, useRef, useLayoutEffect } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui } from "./state.js";
import { routeHash } from "./context.js";
import { Icon } from "./icons.js";
import { api, isDesktop, setStar, showModelFolder, toast, undoable, undoLast, followJob, loadOverview, followIds, recorded } from "./library.js";

const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;
const nameOf = (ms) => (ms.length === 1 ? ms[0].name : plural(ms.length, "model", "models"));

/** True, or why the library can't be changed. */
export const writable = () => (ui.get().library?.read_only ? "The library is read-only." : true);

// ---------------------------------------------------------------- the menu

/** Open a menu: at a mouse event's point (or { clientX, clientY }), or under an
 *  element (a More button). `items`: [{ id, label, sub (a second line), icon, key,
 *  run, disabled (false or the reason), danger }, { sep: true } or { head: text }]. */
export function openMenu(at, items) {
  if (!items.some((it) => !it.sep && !it.head)) return;
  let x, y, from = document.activeElement;
  if (at instanceof Element) {
    const r = at.getBoundingClientRect();
    x = r.left;
    y = r.bottom + 4;
    from = at;
  } else {
    at.preventDefault?.();
    x = at.clientX;
    y = at.clientY;
  }
  ui.set({ menu: { x, y, items, from } });
}

export const closeMenu = () => {
  const m = ui.get().menu;
  if (!m) return;
  ui.set({ menu: null });
  if (m.from?.isConnected) m.from.focus?.({ preventScroll: true });
};

/** The menu that's open, drawn over everything. Arrow keys move, Enter picks, Esc closes. */
export function ContextMenu() {
  const menu = useStore(ui, (s) => s.menu);
  const ref = useRef(null);
  const [pos, setPos] = useState(null);
  useLayoutEffect(() => {
    setPos(null);
    if (!menu || !ref.current) return;
    const r = ref.current.getBoundingClientRect();
    const x = Math.max(8, Math.min(menu.x, innerWidth - r.width - 8));
    const y = menu.y + r.height > innerHeight - 8 ? Math.max(8, menu.y - r.height - 8) : menu.y;
    setPos({ x, y });
  }, [menu]);
  // once it's placed (and visible), the first item takes the keys
  useLayoutEffect(() => {
    if (pos) ref.current?.querySelector(".menu-item:not(:disabled)")?.focus({ preventScroll: true });
  }, [pos]);
  useEffect(() => {
    if (!menu) return;
    const outside = (e) => { if (!ref.current?.contains(e.target)) closeMenu(); };
    const away = () => closeMenu();
    addEventListener("mousedown", outside, true);
    addEventListener("contextmenu", outside, true);
    addEventListener("resize", away);
    addEventListener("blur", away);
    addEventListener("wheel", away, { passive: true });
    return () => {
      removeEventListener("mousedown", outside, true);
      removeEventListener("contextmenu", outside, true);
      removeEventListener("resize", away);
      removeEventListener("blur", away);
      removeEventListener("wheel", away);
    };
  }, [menu]);
  if (!menu) return null;
  const onKey = (e) => {
    const all = [...ref.current.querySelectorAll(".menu-item:not(:disabled)")];
    const i = all.indexOf(document.activeElement);
    const go = (n) => { e.preventDefault(); all[(n + all.length) % all.length]?.focus(); };
    if (e.key === "ArrowDown") go(i + 1);
    else if (e.key === "ArrowUp") go(i < 0 ? -1 : i - 1);
    else if (e.key === "Home") go(0);
    else if (e.key === "End") go(-1);
    else if (e.key === "Escape" || e.key === "Tab") { e.preventDefault(); e.stopPropagation(); closeMenu(); }
  };
  return html`<div class="menu" role="menu" id="context-menu" ref=${ref} onKeyDown=${onKey}
      style=${pos ? `left:${pos.x}px;top:${pos.y}px` : `left:${menu.x}px;top:${menu.y}px;visibility:hidden`}>
    ${menu.items.map((it, i) => it.sep ? html`<div class="menu-sep" role="separator" key=${`sep${i}`}></div>`
      : it.head ? html`<div class="menu-head" key=${`head${i}`}>${it.head}</div>`
      : html`<button type="button" role="menuitem" key=${it.id || i} data-action=${it.id} class=${`menu-item${it.danger ? " danger-text" : ""}`}
          disabled=${!!it.disabled} title=${typeof it.disabled === "string" ? it.disabled : it.title || null}
          onClick=${() => { closeMenu(); it.run(); }}>
          <span class="menu-icon">${it.icon && Icon[it.icon] ? Icon[it.icon](14, it.filled) : null}</span><span class="menu-label">${it.label}${it.sub ? html`<small class="menu-sub">${it.sub}</small>` : null}</span>${it.key ? html`<kbd>${it.key}</kbd>` : null}</button>`)}
  </div>`;
}

// ---------------------------------------------------------------- keys

let pageKeys = null;

/** What a page's keys do: `fn(e)` sees each key press that isn't typing in a box
 *  (and no dialog or menu is open). Shift+F10 and the menu key come as "ContextMenu". */
export function usePageKeys(fn) {
  const ref = useRef(fn);
  ref.current = fn;
  useLayoutEffect(() => {
    const h = (e) => ref.current(e);
    pageKeys = h;
    return () => { if (pageKeys === h) pageKeys = null; };
  }, []);
}

/** Whether a key press is typing (in a box, or on a button where Enter and Space press it). */
export const typing = (e) => !!e.target.closest?.("input, textarea, select, [contenteditable='true']");
const onControl = (e) => !!e.target.closest?.("button, a[href], summary");

/** The window's keys: Ctrl+Z undoes, "/" goes to the page's search box, the rest
 *  goes to the page. */
export function onWindowKey(e) {
  const s = ui.get();
  if (s.menu && e.key === "Escape") { closeMenu(); return; }
  if (s.dialog || s.menu || e.defaultPrevented || typing(e)) return;
  const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
  if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && key === "z") {
    e.preventDefault();
    undoLast();
    return;
  }
  if (key === "/" && !e.ctrlKey && !e.metaKey && !e.altKey) {
    const box = document.querySelector("[data-search]");
    if (box) { e.preventDefault(); box.focus(); box.select?.(); }
    return;
  }
  if ((key === "Enter" || key === " ") && onControl(e)) return;
  if ((e.shiftKey && key === "F10") || key === "ContextMenu") {
    e.preventDefault();
    pageKeys?.({ key: "ContextMenu", target: e.target, shiftKey: false, ctrlKey: false, metaKey: false, altKey: false, preventDefault() {} });
    return;
  }
  pageKeys?.(e);
}

/** The plain letter of a key press (no Ctrl, Alt or Cmd), or "". */
export const letter = (e) => (!e.ctrlKey && !e.metaKey && !e.altKey && e.key.length === 1 ? e.key.toLowerCase() : "");
/** Ctrl+A (Cmd+A). */
export const selectAllKey = (e) => (e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === "a";

// ---------------------------------------------------------------- model actions

const allStarred = (ms) => ms.length > 0 && ms.every((m) => ui.get().favs.includes(m.id));

/** Edit details: the form for one model (read in full first), or the form for several. */
export async function editDetails(ms, select = false) {
  if (ms.length > 1) return ui.set({ dialog: { type: "edit-picked", models: ms } });
  try {
    const m = await api("model_get", { id: ms[0].id });
    const schema = ui.get().overview?.schemas?.find((x) => x.id === m.schema);
    ui.set({ dialog: { type: "edit-model", model: m, schema, select } });
  } catch (e) { toast(e.message || String(e), 6000); }
}

/** Star them all, or take the star off them all, with Undo. */
async function star(ms) {
  const on = !allStarred(ms);
  const changed = ms.filter((m) => ui.get().favs.includes(m.id) !== on);
  const done = [];
  try {
    for (const m of changed) {
      const r = await setStar(m, on);
      done.push({ ...m, id: r.id });
    }
  } catch (e) { toast(`Couldn't star it: ${e.message || e}`, 6000); }
  if (!done.length) return;
  undoable(on ? `Starred ${nameOf(done)}.` : `Took the star off ${nameOf(done)}.`, async () => {
    for (const m of done) await setStar(m, !on);
  }, 5000);
}

/** Draw the models' previews again from their main 3D file. */
async function remakePreviews(ms) {
  try {
    const { job } = await api("thumbs_make", { ids: ms.map((m) => m.id), force: true });
    const done = await followJob(job, "Making previews");
    await loadOverview();
    if (done.error) throw new Error(done.error);
    const r = done.result || {};
    toast(r.made ? `Made ${plural(r.made, "new preview", "new previews")}.` : "No preview could be made: there's no 3D file this app can draw.", 5000);
  } catch (e) { toast(`Couldn't make the preview: ${e.message || e}`, 6000); }
}

/** Return to deterministic automatic composition, preserving image files and Undo. */
async function restoreAutomaticPreview(ms) {
  const m = ms[0];
  if (!m) return;
  try {
    const v = await api("model_cover", { id: m.id, automatic: true });
    followIds({ [m.id]: v.id });
    await loadOverview();
    recorded(`Restored ${m.name}'s automatic preview.`, v.journal);
  } catch (e) { toast(`Couldn't restore the automatic preview: ${e.message || e}`, 6000); }
}

async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const t = document.createElement("textarea");
    t.value = text;
    document.body.append(t);
    t.select();
    document.execCommand("copy");
    t.remove();
  }
}

/** Everything that can be done to models, in the order shown everywhere. `ctx.page`:
 *  "browse", "model" (the model's own page) or "dupes". */
export const MODEL_ACTIONS = [
  { id: "open", label: "Open", icon: "eye", key: "Enter", primary: true,
    applies: (ms, c) => ms.length === 1 && c.page !== "model",
    run: (ms) => { location.hash = routeHash(`model:${ms[0].id}`); } },
  { id: "edit", label: "Edit details…", icon: "edit", key: "E", enabled: writable, run: (ms) => editDetails(ms) },
  { id: "move", label: "Move to category…", icon: "move", key: "M", enabled: writable,
    run: (ms) => ui.set({ dialog: { type: "move-models", models: ms } }) },
  { id: "star", label: (ms) => (allStarred(ms) ? "Remove star" : "Star"), button: (ms) => (allStarred(ms) ? "Starred" : "Star"),
    pressed: allStarred, icon: "star", key: "S", enabled: writable, run: star },
  { id: "folder", label: "Show in folder", icon: "folder", applies: (ms) => ms.length === 1 && isDesktop(), run: (ms) => showModelFolder(ms[0]) },
  { id: "preview", label: "Make a new preview", icon: "image", more: true, enabled: writable, run: remakePreviews },
  { id: "auto-cover", label: "Use automatic preview", icon: "refresh", more: true, enabled: writable,
    applies: (ms) => ms.length === 1 && !!ms[0].files?.explicit_cover, run: restoreAutomaticPreview },
  { id: "copy-path", label: "Copy folder path", icon: "copy", more: true, applies: (ms) => ms.length === 1,
    run: (ms) => copyText(`${ui.get().library?.path}/${ms[0].rel}`).then(() => toast("Copied the folder's path.")) },
];

const shown = (actions, targets, c) => actions.filter((a) => !a.applies || a.applies(targets, c));
const enabledOf = (a, targets, c) => (!targets.length ? "Select something first." : a.enabled ? a.enabled(targets, c) : true);
const labelOf = (a, targets, c, as) => { const l = (as === "button" && a.button) || a.label; return typeof l === "function" ? l(targets, c) : l; };

/** Actions as menu items. */
export function menuItems(actions, targets, c = {}) {
  const list = shown(actions, targets, c);
  const main = list.filter((a) => !a.more);
  const more = list.filter((a) => a.more);
  const item = (a) => {
    const ok = enabledOf(a, targets, c);
    return { id: a.id, label: labelOf(a, targets, c), icon: a.icon, filled: a.pressed?.(targets, c), key: a.key, danger: a.danger, disabled: ok === true ? false : ok, run: () => a.run(targets, c) };
  };
  return [...main.map(item), ...(more.length && main.length ? [{ sep: true }] : []), ...more.map(item)];
}

/** Run the action a key stands for (E, M, S…), if it applies and can be used now. */
export function runKey(actions, key, targets, c = {}) {
  const a = shown(actions, targets, c).find((x) => x.key === key);
  if (!a) return false;
  const ok = enabledOf(a, targets, c);
  if (ok === true) a.run(targets, c);
  else toast(ok);
  return true;
}

/** The actions as a row of buttons (the ones under More in a menu). Ids are
 *  `<idPrefix>-<action id>`. */
export function ActionRow({ actions = MODEL_ACTIONS, targets, ctx: c = {}, idPrefix }) {
  useStore(ui, (s) => ({ favs: s.favs, ro: s.library?.read_only }));
  const list = shown(actions, targets, c);
  const main = list.filter((a) => !a.more);
  const more = list.filter((a) => a.more);
  return html`<div class="insp-actions action-row">
    ${main.map((a) => {
      const ok = enabledOf(a, targets, c);
      const label = labelOf(a, targets, c, "button");
      const pressed = a.pressed?.(targets, c);
      return html`<button type="button" key=${a.id} id=${`${idPrefix}-${a.id}`} data-action=${a.id} class=${a.primary ? "primary compact" : "ghost"}
        disabled=${ok !== true} aria-pressed=${a.pressed ? (pressed ? "true" : "false") : null}
        title=${ok !== true ? ok : a.key ? `${labelOf(a, targets, c)} (${a.key})` : null}
        onClick=${() => a.run(targets, c)}>${a.icon ? Icon[a.icon](15, pressed) : null} ${label}</button>`;
    })}
    ${more.length ? html`<button type="button" class="ghost" id=${`${idPrefix}-more`} aria-haspopup="menu" title="More actions"
      onClick=${(e) => openMenu(e.currentTarget, menuItems(more, targets, c))}>${Icon.more(15)} More ▾</button>` : null}
  </div>`;
}
