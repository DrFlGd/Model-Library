// Suggestions as you type an author, a tag or a licence: the library's own values,
// most used first, so "kit walker" offers Kit Walker instead of making a second
// author. Any box with data-suggest="authors", "tags" or "licences" gets them;
// authors and tags are lists separated by commas, so only the part being typed
// is matched. data-values (a JSON list) gives a box its own values instead.
// ↓ and ↑ move through them, Enter or a click takes one, Esc closes the list.
import { html, useState, useEffect } from "../lib/html.js";
import { ui } from "./state.js";
import { api } from "./library.js";

const MULTI = { authors: true, tags: true, licences: false };
const MAX = 8;

let cache = { key: null, values: null };
/** The library's authors, tags and licences, asked for again when models change. */
function libraryValues() {
  const s = ui.get();
  const key = `${s.library?.path}|${s.catalogRev}`;
  if (cache.key !== key) cache = { key, values: api("library_values", {}).catch(() => ({})) };
  return cache.values;
}

export const fold = (s) => String(s).normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();

/** The part of a box's text being typed: [start, end, text]. */
function part(box, multi) {
  const v = box.value;
  if (!multi) return [0, v.length, v.trim()];
  const at = box.selectionStart ?? v.length;
  const start = v.lastIndexOf(",", at - 1) + 1;
  let end = v.indexOf(",", at);
  if (end < 0) end = v.length;
  return [start, end, v.slice(start, end).trim()];
}

/** The values to offer for `text`: those starting with it, then those with a word starting with it. */
export function pick(values, text, taken = []) {
  const t = fold(text);
  if (!t) return [];
  const skip = new Set(taken.map(fold));
  const first = [], then = [];
  for (const v of values || []) {
    if (v === text.trim() || skip.has(fold(v))) continue;
    const f = fold(v);
    if (f.startsWith(t)) first.push(v);
    else if (f.split(/[^\p{L}\p{N}]+/u).some((w) => w.startsWith(t))) then.push(v);
    if (first.length >= MAX) break;
  }
  return [...first, ...then].slice(0, MAX);
}

let show = () => {};
let current = null; // { box, kind, items, at }
let quiet = false;  // the input event a pick sends doesn't ask again

async function update(box) {
  const kind = box.dataset.suggest;
  if (!(kind in MULTI) || box.disabled || box.readOnly) return close();
  const multi = MULTI[kind];
  const [start, end, text] = part(box, multi);
  const own = box.dataset.values ? JSON.parse(box.dataset.values) : null;
  const values = own || (await libraryValues())[kind] || [];
  if (document.activeElement !== box) return;
  const taken = multi ? (box.value.slice(0, start) + box.value.slice(end)).split(",").map((x) => x.trim()).filter(Boolean) : [];
  const items = pick(values, text, taken);
  if (!items.length) return close();
  current = { box, kind, items, at: -1 };
  show({ ...current, rect: box.getBoundingClientRect() });
}

function close() {
  if (!current) return;
  current = null;
  show(null);
}

function take(value) {
  const c = current;
  if (!c) return;
  const box = c.box, multi = MULTI[c.kind];
  const [start, end] = part(box, multi);
  const before = box.value.slice(0, start), after = box.value.slice(end);
  const lead = start > 0 ? " " : "";
  box.value = `${before}${lead}${value}${after}`;
  const caret = before.length + lead.length + value.length;
  box.setSelectionRange(caret, caret);
  close();
  quiet = true;
  box.dispatchEvent(new Event("input", { bubbles: true }));
  quiet = false;
}

const suggesting = (t) => t instanceof HTMLInputElement && t.dataset.suggest;

function onInput(e) {
  if (!quiet && suggesting(e.target)) update(e.target);
}

function onKey(e) {
  const c = current;
  if (!c || e.target !== c.box) return;
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    e.stopPropagation();
    const n = c.items.length;
    c.at = e.key === "ArrowDown" ? (c.at + 1) % n : c.at <= 0 ? n - 1 : c.at - 1;
    show({ ...c, rect: c.box.getBoundingClientRect() });
  } else if (e.key === "Enter" && c.at >= 0) {
    // the highlighted value, not the box's Enter (save, or the dialog's button)
    e.preventDefault();
    e.stopPropagation();
    take(c.items[c.at]);
  } else if (e.key === "Escape") {
    // only the list closes; the box and the dialog stay as they are
    e.preventDefault();
    e.stopPropagation();
    close();
  } else if (e.key === "Tab") {
    close();
  }
}

function onBlur(e) {
  if (current && e.target === current.box) close();
}

function onScroll(e) {
  if (current && !document.getElementById("suggest-list")?.contains(e.target)) close();
}

let listening = false;
function listen() {
  if (listening) return;
  listening = true;
  document.addEventListener("input", onInput, true);
  document.addEventListener("keydown", onKey, true);
  document.addEventListener("focusout", onBlur, true);
  document.addEventListener("scroll", onScroll, true);
  addEventListener("resize", close);
}

/** The list under the box being typed in, drawn over everything. */
export function Suggestions() {
  const [state, setState] = useState(null);
  useEffect(() => {
    show = setState;
    listen();
    return () => { show = () => {}; };
  }, []);
  if (!state) return null;
  const { rect, items, at } = state;
  const below = innerHeight - rect.bottom > Math.min(items.length * 32 + 16, 280) || rect.top < innerHeight / 2;
  const style = { left: `${Math.max(8, Math.min(rect.left, innerWidth - Math.max(rect.width, 200) - 8))}px`, minWidth: `${Math.min(rect.width, innerWidth - 16)}px`,
    ...(below ? { top: `${rect.bottom + 3}px` } : { bottom: `${innerHeight - rect.top + 3}px` }) };
  return html`<ul class="suggest-list menu" id="suggest-list" role="listbox" aria-label="Suggestions" style=${style}>
    ${items.map((v, i) => html`<li key=${v} role="option" class=${`menu-item suggest-item${i === at ? " on" : ""}`} aria-selected=${i === at ? "true" : "false"}
      onMouseDown=${(e) => { e.preventDefault(); take(v); }}>${v}</li>`)}
  </ul>`;
}
