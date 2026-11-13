// Renaming a tag or an author across the library, or merging two spellings into
// one (docs/PLAN.md, "B: rename or merge tags and authors"): from the right-click
// menu of a tag or an author wherever it's a link or a chip. One journalled
// change, so Undo is in the message and on Home.
import { html, useState, useEffect, useRef, useLayoutEffect } from "../lib/html.js";
import { ui } from "./state.js";
import { api, loadOverview, followIds, recorded } from "./library.js";
import { Dialog, close } from "./dialogs.js";
import { openMenu } from "./actions.js";

const WHAT = { tags: "tag", authors: "author" };
const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;
/** As the search writes it: tag:petg, author:"Kit Walker". */
const filterOf = (kind, v) => `${WHAT[kind]}:${/\s/.test(v) ? `"${v}"` : v}`;
const shown = (kind, v) => (kind === "tags" ? `#${v}` : v);

/** The right-click menu of a tag or an author: `first` is the link's own action
 *  ({ label, run }), then Rename… */
export function valueMenu(e, kind, value, first = null) {
  e.preventDefault();
  e.stopPropagation();
  const ro = ui.get().library?.read_only ? "The library is read-only." : false;
  openMenu(e, [
    ...(first ? [{ id: `value-show`, label: first.label, icon: "search", run: first.run }] : []),
    { id: `value-rename`, label: `Rename ${WHAT[kind]}…`, sub: "Every model that has it, or merge it into another", icon: "edit", disabled: ro,
      run: () => ui.set({ dialog: { type: "rename-value", kind, value } }) },
  ]);
}

/** Rename tag… / Rename author…: one box; the library's own spelling of the new
 *  name, if it has one, makes it a merge. */
export function RenameValueDialog({ kind, value }) {
  const what = WHAT[kind];
  const [to, setTo] = useState(value);
  const [count, setCount] = useState(null);
  const [merge, setMerge] = useState(null); // { spelt, count } when the new name is in use
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const box = useRef(null);
  useLayoutEffect(() => { box.current?.focus(); box.current?.select(); }, []);
  useEffect(() => {
    api("values_rename", { kind, from: value, to: "", dry: true }).then((r) => setCount(r.count), (e) => setError(e.message || String(e)));
  }, []);
  useEffect(() => {
    const t = setTimeout(() => {
      const name = to.trim();
      if (!name || name === value) { setMerge(null); return; }
      api("values_rename", { kind, from: value, to: name, dry: true }).then((r) => setMerge(r.spelt ? { spelt: r.spelt, count: r.merge } : null), () => setMerge(null));
    }, 200);
    return () => clearTimeout(t);
  }, [to]);
  const submit = async () => {
    setBusy(true);
    setError("");
    try {
      const r = await api("values_rename", { kind, from: value, to });
      followIds(r.ids || {});
      // a search for the old name follows it
      const s = ui.get();
      const final = r.spelt || to.trim().split(/\s+/).join(" ");
      if (s.q.includes(filterOf(kind, value))) ui.set({ q: s.q.split(filterOf(kind, value)).join(filterOf(kind, final)) });
      await loadOverview();
      if (r.errors?.length) { setError(r.errors.map((x) => `${x.name}: ${x.error}`).join(" ")); return; }
      close();
      const n = plural(r.count, "model", "models");
      recorded(r.spelt ? `Merged ${shown(kind, value)} into ${shown(kind, r.spelt)} on ${n}.` : `Renamed ${shown(kind, value)} to ${shown(kind, final)} on ${n}.`, r.journal);
    } catch (e) { setError(e.message || String(e)); } finally { setBusy(false); }
  };
  const same = to.trim().split(/\s+/).join(" ") === value;
  return html`<${Dialog} title=${`Rename ${what} ${shown(kind, value)}`} id="rename-value-dialog" onSubmit=${submit} busy=${busy || count === null || !to.trim() || same} error=${error}
      submitLabel=${merge ? "Merge" : "Rename"}>
    <p class="muted" id="rename-value-count">${count === null ? "Counting…" : `${count === 1 ? "One model has" : `${count} models have`} it. Each changes in its details; folder names stay as they are.`}</p>
    <label class="field-block"><span>New name</span><input type="text" id="rename-value-to" ref=${box} data-suggest=${kind} autocomplete="off" spellcheck="false" maxlength="120" value=${to}
      onInput=${(e) => setTo(e.target.value)} /></label>
    ${merge ? html`<p class="warn-note" id="rename-value-merge">${shown(kind, merge.spelt)} is already on ${plural(merge.count, "model", "models")}, so the two become one ${what}, ${shown(kind, merge.spelt)}. A model that has both keeps it once.</p>` : null}
  <//>`;
}
