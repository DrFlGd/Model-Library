// Health check (docs/PLAN.md, "Health check design"): what in the library needs
// looking at, most serious first, each with the fix the app can make. The quick
// check reads folders and model.json files; "Also read every file" reads every
// 3D, slicer and archive file whole, to find damaged and cut-short ones. Click a
// model to see it in the details panel (double-click or Enter opens it);
// right-click for the same actions as everywhere.
import { html, useState, useEffect } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, setPref } from "./state.js";
import { routeHash } from "./context.js";
import { api, followJob, loadOverview, toast, recorded } from "./library.js";
import { size, ModelDetails } from "./details.js";
import { CommandBar } from "./layout.js";
import { Inspector } from "./inspector.js";
import { Icon } from "./icons.js";
import { MODEL_ACTIONS, menuItems, openMenu, usePageKeys, runKey, letter } from "./actions.js";

const CTX = { page: "health" };
const SHOWN = 40;
const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;
const when = (t) => (t || "").replace("T", " ").slice(0, 16);
const openModel = (id) => { location.hash = routeHash(`model:${id}`); };

/** Each kind: its heading, what it means and what to do, and its fix (if the app
 *  has one): { label (with the count), ask (asked first), done (what was done). */
const KINDS = {
  damaged: { title: "Damaged files", tone: "bad",
    text: "These files have the same size and date as when they were last read, but different contents. A disk, a network share or a copy has usually damaged them: put them back from a backup. If you changed them on purpose, mark them as fine.",
    fix: { label: (n) => `Mark ${n === 1 ? "it" : `all ${n}`} as fine`, done: (n) => `Marked ${plural(n, "file", "files")} as fine.` } },
  gone: { title: "Files that have gone", tone: "bad",
    text: "Each model's model.json remembers these files, but they aren't in its folder any more. They may have been deleted, renamed or moved outside the app, or a sync hasn't finished yet.",
    fix: { label: () => "Forget them", done: (n) => `Forgot the files of ${plural(n, "model", "models")}.` } },
  unreadable: { title: "Files that can't be read", tone: "bad",
    text: "These files can't be opened, are empty, or are cut short (a download or copy that didn't finish). Open the model to replace or delete them." },
  broken_json: { title: "model.json that can't be read", tone: "bad",
    text: "The file is damaged or was only half written, so these models show their folder name and lose their details, stars and collections. A new model.json can be started from the folder; the old one is kept beside it as model.json.broken.",
    fix: { label: (n) => `Start ${n === 1 ? "a new one" : `${n} new ones`}`, done: (n) => `Started a new model.json for ${plural(n, "model", "models")}.` } },
  same_id: { title: "Models sharing an id", tone: "warn",
    text: "A model folder was copied outside the app, so two folders have the same id and stars, collections and Undo can't tell them apart. The copy marked Keep keeps the id; the others get their own.",
    fix: { label: () => "Give the copies their own id", done: (n) => `Gave ${plural(n, "copied model", "copied models")} their own id.` } },
  empty: { title: "Empty models", tone: "warn",
    text: "These model folders have no files, only their model.json." ,
    fix: { label: (n) => `Delete ${n === 1 ? "it" : `all ${n}`}…`, del: true } },
  cover: { title: "Cover picture missing", tone: "warn",
    text: "The picture chosen as the cover isn't in the model's folder any more, so the card shows nothing chosen.",
    fix: { label: () => "Use the automatic previews", done: (n) => `Gave ${plural(n, "model", "models")} the automatic preview.` } },
  lists: { title: "Stars and collections naming models that have gone", tone: "warn",
    text: "These models aren't in the library any more: deleted, or moved out of it outside the app. If they're coming back (a drive that isn't connected), leave them.",
    fix: { label: () => "Take them off the lists", ask: (n) => ({ title: "Take them off the lists", text: `${plural(n, "entry", "entries")} for models the library doesn't have will be taken off Starred and the collections.`, button: "Take them off" }),
      done: (n) => `Took ${plural(n, "model", "models")} that have gone off the lists.` } },
  no_sidecar: { title: "Models without model.json", tone: "info",
    text: "Their name and author come from the folder name, and nothing about them travels with the folder. Writing model.json keeps their details if the library moves, and lets the next full check tell damaged files from changed ones.",
    fix: { label: (n) => `Write ${n === 1 ? "its" : "their"} model.json`, done: (n) => `Wrote model.json for ${plural(n, "model", "models")}.` } },
  long_path: { title: "Paths too long for Windows", tone: "info",
    text: "Windows can't always open a path longer than 250 characters, counting the library folder. Shorten the model's folder or file names, or keep the library in a shorter place." },
  bad_name: { title: "Names Windows can't use", tone: "info",
    text: "These names have characters such as : ? * \" | < >, end with a space or a dot, or are names like CON or NUL. Windows can't open them: rename them on the computer or NAS they came from." },
  leftovers: { title: "Leftovers from stopped copies", tone: "info",
    text: "Hidden files and folders left when a copy, import or archive change was stopped or the app closed. They can hold the only copy of a file, so they're never deleted here: they can be moved to Unsorted, where you can look at them and delete what you don't need.",
    fix: { label: () => "Move them to Unsorted", ask: (n) => ({ title: "Move leftovers to Unsorted", text: `${plural(n, "leftover", "leftovers")} will be moved into Unsorted › Leftovers from stopped copies, where you can look at them and delete what you don't need.`, button: "Move to Unsorted" }),
      done: (n) => `Moved ${plural(n, "leftover", "leftovers")} to Unsorted.` } },
};

/** What a running check is doing, in words. */
function progressText(p) {
  if (!p?.step) return "Starting…";
  if (p.step === "models") return `Reading model folders: ${Math.min(p.item + 1, p.items)} of ${p.items}`;
  if (p.step === "leftovers") return "Looking for leftovers…";
  return `Reading files: ${size(p.bytes)} of ${size(p.total_bytes)}`;
}

/** One line of a kind: the model (or file) and what's wrong. */
function Item({ kind, it, picked, pick }) {
  if (kind === "lists") return html`<li class="health-item" data-key=${it.key}>
    <span class="health-what">${it.coll ? Icon.bookmark(13) : Icon.star(13)} <b>${it.list}</b></span><span class="muted health-detail">a model that has gone (${it.model})</span></li>`;
  if (kind === "leftovers") return html`<li class="health-item" data-key=${it.key}>
    <span class="health-what">${it.folder ? Icon.folder(13) : Icon.file(13)} <span class="health-rel">${it.rel}</span></span><span class="muted health-detail">${size(it.bytes || 0)}</span></li>`;
  if (kind === "same_id") return html`<li class="health-item health-set" data-key=${it.key}>
    <ul class="health-copies">${it.models.map((m) => html`<${ModelLine} key=${m.rel} m=${m} picked=${picked} pick=${pick}
      detail=${m.keep ? html`<b>Keep</b>` : "Gets its own id"} />`)}</ul></li>`;
  const detail = kind === "damaged" || kind === "unreadable" ? html`<span class="health-file">${it.file}</span>${it.why ? ` ${it.why}` : ""}`
    : kind === "gone" ? (it.files.length === 1 ? it.files[0] : `${it.files[0]} and ${plural(it.files.length - 1, "other", "others")}`)
    : kind === "cover" ? `${it.cover || "picture"} isn't there`
    : kind === "long_path" ? `${it.file} (${it.length} characters${it.files > 1 ? `; ${it.files} files too long` : ""})`
    : kind === "bad_name" ? `${it.file}${it.files > 1 ? ` and ${plural(it.files - 1, "other", "others")}` : ""}`
    : "";
  return html`<${ModelLine} m=${it} picked=${picked} pick=${pick} detail=${detail} />`;
}

function ModelLine({ m, picked, pick, detail }) {
  return html`<li class="health-item health-model" data-model=${m.id} aria-selected=${picked === m.id ? "true" : "false"}
    onClick=${() => pick(m.id)} onDblClick=${() => openModel(m.id)}
    onContextMenu=${(e) => { pick(m.id); openMenu(e, menuItems(MODEL_ACTIONS, [m], CTX)); }}>
    <span class="health-what"><span class="health-name">${m.name}</span><span class="muted health-rel">${m.rel}</span></span>
    <span class="muted health-detail">${detail}</span>
  </li>`;
}

function Group({ g, writable, busy, fix, picked, pick }) {
  const k = KINDS[g.kind] || { title: g.kind, text: "" };
  const [more, setMore] = useState(SHOWN);
  const n = g.items.length;
  return html`<section class=${`health-group tone-${k.tone || "info"}`} id=${`health-${g.kind}`} data-kind=${g.kind}>
    <div class="health-group-head">
      <h2>${k.title} <span class="muted">(${g.count})</span></h2>
      ${k.fix && writable ? html`<button type="button" class="ghost health-fix" id=${`health-fix-${g.kind}`} disabled=${busy} onClick=${() => fix(g, k)}>${k.fix.label(n)}</button>` : null}
    </div>
    <p class="muted">${k.text}</p>
    <ul class="health-items">${g.items.slice(0, more).map((it) => html`<${Item} key=${it.key} kind=${g.kind} it=${it} picked=${picked} pick=${pick} />`)}</ul>
    ${n > more ? html`<button type="button" class="ghost" onClick=${() => setMore(more + 100)}>Show more (${n - more} left)</button>` : null}
    ${g.count > n ? html`<p class="muted">Showing the first ${n}; check again once these are fixed to see the rest.</p>` : null}
  </section>`;
}

export function HealthPage() {
  const lib = useStore(ui, (s) => s.library);
  const deep = useStore(ui, (s) => !!s.healthDeep);
  const rev = useStore(ui, (s) => s.catalogRev);
  const [data, setData] = useState(null);
  const [error, setError] = useState(null);
  const [job, setJob] = useState(null);
  const [picked, setPicked] = useState(null);
  const load = () => api("health_get", {}).then((d) => { setData(d); setError(null); }, (e) => setError(e.message || String(e)));
  useEffect(() => { if (lib) load(); }, [lib?.path, rev]); // again after a change elsewhere (a delete)
  const groups = data?.groups || [];
  const models = groups.flatMap((g) => g.kind === "same_id" ? g.items.flatMap((it) => it.models) : g.items.filter((it) => it.id && it.name));
  const pickedModel = picked ? models.find((m) => m.id === picked) : null;
  usePageKeys((e) => {
    const l = letter(e);
    if (e.key === "Escape" && picked) setPicked(null);
    else if (e.key === "Enter" && pickedModel) { e.preventDefault(); openModel(pickedModel.id); }
    else if ((l === "e" || l === "m" || l === "s" || l === "c") && pickedModel) { e.preventDefault(); runKey(MODEL_ACTIONS, l.toUpperCase(), [pickedModel], CTX); }
  });
  if (!lib) return html`<div class="pages"><h1>Health check</h1><p>Open a library first.</p></div>`;
  const writable = data ? data.writable !== false : !lib.read_only;
  const busy = !!job;

  const check = async () => {
    try {
      const { job: id } = await api("health_check", { deep });
      setJob({ id, progress: null });
      const done = await followJob(id, deep ? "Checking the library and reading its files" : "Checking the library", (j) => setJob({ id, progress: j.progress }));
      if (done.error) throw new Error(done.error);
      const n = done.result?.problems || 0;
      toast(n ? `Found ${plural(n, "thing", "things")} to look at.` : "Nothing needs looking at.", 5000);
    } catch (e) { toast(e.message === "Stopped." ? "Stopped checking." : `Couldn't check the library: ${e.message || e}`, 6000); }
    finally { setJob(null); await load(); }
  };
  const stop = () => job && api("job_cancel", { id: job.id });

  const fix = (g, k) => {
    const keys = g.items.map((it) => it.key);
    if (k.fix.del) {
      ui.set({ dialog: { type: "delete-models", models: g.items.map((it) => ({ id: it.id, name: it.name, rel: it.rel })) } });
      return;
    }
    const run = async () => {
      setJob({ id: null, progress: null, fixing: true });
      try {
        const r = await api("health_fix", { kind: g.kind, keys });
        const said = k.fix.done(r.fixed);
        const failed = r.failed?.length ? ` ${plural(r.failed.length, "couldn't be fixed", "couldn't be fixed")}: ${r.failed[0].error}` : "";
        if (r.journal) recorded(said + failed, r.journal, load);
        else toast(said + failed, failed ? 8000 : 5000);
      } catch (e) { toast(e.message || String(e), 6000); }
      finally { setJob(null); await loadOverview(); await load(); }
    };
    if (k.fix.ask) confirmLike(k.fix.ask(keys.length), run);
    else run();
  };

  const total = groups.reduce((n, g) => n + g.count, 0);
  const count = data?.checked ? (total ? plural(total, "thing to look at", "things to look at") : "All well") : null;
  const lead = html`<p class="page-lead">What in the library needs looking at, most serious first, with the fix the app can make. Nothing is checked until you choose Check now.</p>`;
  return html`<${CommandBar} id="health-head" title="Health check" count=${count}
      primary=${html`<button type="button" class=${data?.checked ? "ghost" : "primary compact"} id="health-check" disabled=${busy} onClick=${check}>${Icon.pulse(15)} Check now</button>`}
      selection=${pickedModel ? { count: `1 model selected: ${pickedModel.name}`, onClear: () => setPicked(null), items: menuItems(MODEL_ACTIONS, [pickedModel], CTX), buttons: 3 } : null} />
  <div class="browser dupes-layout" id="health-page">
   <div class="pages dupes-page health-page">
    ${lead}
    <label class="check-row" id="health-deep-row"><input type="checkbox" id="health-deep" checked=${deep} disabled=${busy} onChange=${(e) => setPref({ healthDeep: e.currentTarget.checked })} />
      <span>Also read every 3D, slicer and archive file <span class="muted">(slow on a big library: finds damaged and cut-short files)</span></span></label>
    ${error ? html`<p class="warn-note" role="alert">${error}</p>` : null}
    ${job && !job.fixing ? html`<div class="home-actions"><span class="dupes-progress" id="health-progress" role="status">${progressText(job.progress)}</span>
      <button type="button" class="ghost" id="health-stop" onClick=${stop}>Stop</button></div>` : null}
    ${data?.checked ? html`<p id="health-summary" data-checked=${data.checked}>Checked on ${when(data.checked)}${data.deep ? ", reading every file," : ""} through ${plural(data.models || 0, "model", "models")} and ${plural(data.files || 0, "file", "files")}: ${total ? html`<b>${plural(total, "thing needs", "things need")} looking at.</b>` : "nothing needs looking at."}</p>
        ${!data.deep ? html`<p class="muted" id="health-quick-note">That was a quick check. Reading every file finds damaged and cut-short ones; the first time also records what each file holds, so later checks can tell damage from a change.</p>` : null}`
      : html`<p id="health-summary" class="muted">Not checked yet.</p>`}
    ${data?.checked && !writable ? html`<p class="warn-note">This library is read-only, so nothing can be fixed from here.</p>` : null}
    ${groups.map((g) => html`<${Group} key=${g.kind} g=${g} writable=${writable} busy=${busy} fix=${fix} picked=${picked} pick=${setPicked} />`)}
    ${data?.checked && !groups.length ? html`<p class="health-all-well" id="health-none">${Icon.check(16)} All well: nothing needs looking at.</p>` : null}
   </div>
   <${Inspector} page="health" open=${!matchMedia("(max-width: 1250px)").matches} tabs=${[{ id: "details", label: "Details", icon: "info", body: html`<${ModelDetails} id=${pickedModel?.id || null} ctx=${CTX} empty=${html`<div class="place-summary">
     <h2 class="insp-title">Health check</h2>
     ${data?.checked ? html`<p>${total ? plural(total, "thing needs", "things need") + " looking at." : "Nothing needs looking at."}</p>` : html`<p>Not checked yet.</p>`}
     <p class="muted insp-later">Click a model to see its details and files here; double-click or Enter opens it.</p></div>`} />` }]} />
  </div>`;
}

/** Ask first (a fix that can't be undone, but deletes nothing): the plain dialog. */
function confirmLike(ask, run) {
  ui.set({ dialog: { type: "ask", title: ask.title, text: ask.text, button: ask.button, cancel: "Cancel", run } });
}
