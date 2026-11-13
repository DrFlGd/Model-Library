// Dragging models (docs/PLAN.md, "C: collections in your own order"): a card or a
// row dragged onto a collection or Starred in the menu adds it there; on a
// collection's page sorted by Your order, dropped among the others it moves
// there. Pointer events, not the browser's drag and drop, which the desktop
// window keeps for files dropped from outside. A drag starts once the pointer
// has moved a few pixels, so clicks, double-clicks and ticks work as before; Esc
// stops it. Every change says what it did, with Undo.
import { ui } from "./state.js";
import { api, loadOverview, followIds, toast, undoable, setStar } from "./library.js";

const START = 6; // pixels before a press becomes a drag
const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;
const nameOf = (ms) => (ms.length === 1 ? ms[0].name : plural(ms.length, "model", "models"));

/** Add models to a list in the menu: "favs" (a star) or "coll:<id>", with Undo. */
export async function addToList(target, ms) {
  const s = ui.get();
  try {
    if (target === "favs") {
      const fresh = ms.filter((m) => !s.favs.includes(m.id));
      if (!fresh.length) { toast(ms.length === 1 ? "It's starred already." : "They're starred already."); return; }
      const done = [];
      for (const m of fresh) done.push({ ...m, id: (await setStar(m, true)).id });
      undoable(`Starred ${nameOf(done)}.`, async () => { for (const m of done) await setStar(m, false); }, 5000);
      return;
    }
    const id = target.slice(5);
    const coll = s.overview?.collections?.find((c) => c.id === id);
    const r = await api("collections_change", { changes: [{ id, add: ms.map((m) => m.id) }] });
    const map = {};
    for (const x of r.ids || []) if (x.was !== x.id) map[x.was] = x.id;
    if (Object.keys(map).length) followIds(map);
    const now = (m) => map[m.id] || m.id;
    const had = ms.filter((m) => (r.collections?.[0]?.models || []).includes(now(m)));
    await loadOverview();
    undoable(`Added ${nameOf(ms)} to ${coll?.name || "the collection"}.`, async () => {
      await api("collections_change", { changes: [{ id, remove: had.map(now) }] });
      await loadOverview();
    });
  } catch (e) { toast(e.message || String(e), 6000); }
}

/** Move models within a collection (Your order), with Undo. `before`: the id
 *  they go in front of, or null for the end. */
export async function moveInCollection(coll, ids, before) {
  try {
    const r = await api("collection_order", { id: coll.id, ids, before });
    if (JSON.stringify(r.was) === JSON.stringify(r.models)) return;
    await loadOverview();
    undoable(`Moved ${plural(ids.length, "model", "models")} in ${coll.name}.`, async () => {
      await api("collection_order", { id: coll.id, models: r.was });
      await loadOverview();
    });
  } catch (e) { toast(e.message || String(e), 6000); }
}

let drag = null;

function clearMarks() {
  for (const el of document.querySelectorAll(".drop-on, .drop-before, .drop-after")) el.classList.remove("drop-on", "drop-before", "drop-after");
}

// a picture on a card would otherwise start the browser's own drag (and cancel ours)
const noNative = (e) => e.preventDefault();

function stop() {
  if (!drag) return;
  removeEventListener("dragstart", noNative, true);
  removeEventListener("pointermove", onMove, true);
  removeEventListener("pointerup", onUp, true);
  removeEventListener("pointercancel", stop, true);
  removeEventListener("keydown", onKey, true);
  drag.ghost?.remove();
  clearMarks();
  document.documentElement.classList.remove("dragging-models");
  drag = null;
}

/** Where the pointer is: a list in the menu, or (when the page can reorder) a
 *  place among the cards. */
function targetAt(x, y) {
  const el = document.elementFromPoint(x, y);
  if (!el) return null;
  const list = el.closest("[data-drop]");
  if (list) return { kind: "list", el: list, to: list.dataset.drop };
  if (!drag.reorder) return null;
  const card = el.closest(".results [data-model]");
  if (!card) {
    // past the last card: the end
    const results = el.closest(".results");
    return results ? { kind: "order", el: null, before: null } : null;
  }
  const r = card.getBoundingClientRect();
  const grid = !card.classList.contains("row");
  const after = grid ? x > r.left + r.width / 2 : y > r.top + r.height / 2;
  const cards = [...document.querySelectorAll(".results [data-model]")];
  const next = after ? cards[cards.indexOf(card) + 1] : card;
  return { kind: "order", el: card, after, before: next ? next.dataset.model : null };
}

function onMove(e) {
  if (!drag || e.pointerId !== drag.pointerId) return;
  if (!drag.started) {
    if (Math.hypot(e.clientX - drag.x, e.clientY - drag.y) < START) return;
    drag.started = true;
    document.documentElement.classList.add("dragging-models");
    const ghost = document.createElement("div");
    ghost.className = "drag-ghost";
    ghost.id = "drag-ghost";
    ghost.textContent = nameOf(drag.models);
    document.body.appendChild(ghost);
    drag.ghost = ghost;
  }
  e.preventDefault();
  drag.ghost.style.transform = `translate(${e.clientX + 14}px, ${e.clientY + 10}px)`;
  const t = targetAt(e.clientX, e.clientY);
  // dropped among the dragged ones themselves: nothing moves
  if (t?.kind === "order" && t.el && drag.models.some((m) => m.id === t.el.dataset.model)) drag.over = null;
  else drag.over = t;
  clearMarks();
  if (drag.over?.kind === "list") drag.over.el.classList.add("drop-on");
  else if (drag.over?.el) drag.over.el.classList.add(drag.over.after ? "drop-after" : "drop-before");
}

function onUp(e) {
  if (!drag || e.pointerId !== drag.pointerId) return;
  const { started, over, models, reorder } = drag;
  stop();
  if (!started) return;
  // the click that ends a drag isn't a click on what's under it (it comes in the
  // same task as the pointerup, if at all)
  const swallow = (c) => { c.stopPropagation(); c.preventDefault(); };
  addEventListener("click", swallow, true);
  setTimeout(() => removeEventListener("click", swallow, true), 0);
  if (!over) return;
  if (over.kind === "list") addToList(over.to, models);
  else if (reorder) moveInCollection(reorder, models.map((m) => m.id), over.before);
}

function onKey(e) {
  if (e.key === "Escape" && drag) { e.preventDefault(); e.stopPropagation(); stop(); }
}

/** A card's or a row's pointerdown. `models`: the ones it carries (the selection
 *  when the card is in it); `reorder`: the collection whose order the page
 *  shows (Your order), if any. */
export function pressModel(e, models, reorder = null) {
  if (e.button !== 0 || e.ctrlKey || e.metaKey || e.shiftKey || ui.get().library?.read_only) return;
  if (e.target.closest("input, button, a, select, textarea")) return;
  stop();
  drag = { x: e.clientX, y: e.clientY, pointerId: e.pointerId, models, reorder, started: false, over: null, ghost: null };
  addEventListener("dragstart", noNative, true);
  addEventListener("pointermove", onMove, true);
  addEventListener("pointerup", onUp, true);
  addEventListener("pointercancel", stop, true);
  addEventListener("keydown", onKey, true);
}
