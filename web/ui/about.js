// About (docs/PLAN.md, "Group 1 design"): the app's version and what it's built
// with, how a library is laid out, what's coming, the licences and the
// changelog. tools/build_desktop.py copies CHANGELOG.md and THIRD_PARTY.md next
// to the page and writes build-info.json, so this works offline.
import { html, useState, useEffect } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui } from "./state.js";
import { ctx } from "./context.js";
import { PageHead } from "./layout.js";

const OS = { windows: "Windows", linux: "Linux", macos: "macOS" };
const RELEASES = "https://github.com/DrFlGd/Model-Library/releases";

export const TREE = `_library/                  the app's own files
Unsorted/                  models with no category yet
Category/                  a category's top folder
  Subcategory/             as many as you like,
    Subcategory/           inside each other, as deep as each needs
      Model name (Author)/   one folder per model, at any level
        model.json           author, source, date…
        Model name.stl
        Parts/               parts keep their folders
        _media/              pictures, PDFs, videos
        _thumbs/             previews`;

const DONE = [
  ["Phase 0", "The app, its library folder and settings"],
  ["Phase 1", "Categories, model details and search"],
  ["Phase 2", "Importing models: files, folders and ZIPs, moved or copied into place, and moving models between categories"],
  ["Phase 3", "Viewing models: a 3D viewer, parts and variants, previews, pictures, readmes, PDFs and videos"],
  ["Phase 4", "Editing categories: a tree of subcategories, renamed, merged and moved with folders to match, and undo"],
  ["Phase 5", "Large collections: sorting a whole folder tree as it is, finding duplicates, noticing changes made outside the app"],
  ["UI pass", "The same actions, keys and undo on every page, and one page layout; a workspace for a model's files"],
];
const NEXT = [
  ["Group 1", "File actions in every view, adding a subcategory where you are, and this page", true],
  ["Group 2", "Subcategories in a panel beside the menu, a category's subcategories or its models, icons, and a style of its own"],
  ["Group 3", "Gray models with 3MF colours on request, other colours, 3MF thumbnails"],
  ["Group 4", "RAR and 7z opened and extracted; compressing selected files"],
  ["Group 5", "Several libraries open at once, and sending models between them"],
  ["Group 6", "Opening files in other programs (slicers, Meshmixer) and running tools such as MeshMill"],
  ["Group 7", "Details filled in from a Printables, Thingiverse, Thangs or MakerWorld link"],
  ["Later", "Running on a server (Docker) with sign-in, to browse the library from another computer"],
];

/** Text with `code`, **bold**, *italic* and [links](https://…), as nodes (never as HTML). */
function inline(text) {
  const out = [];
  const re = /`([^`]+)`|\*\*(.+?)\*\*|\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)|\*([^*\s][^*]*)\*/g;
  let at = 0, m;
  while ((m = re.exec(text))) {
    if (m.index > at) out.push(text.slice(at, m.index));
    if (m[1] != null) out.push(html`<code>${m[1]}</code>`);
    else if (m[2] != null) out.push(html`<strong>${inline(m[2])}</strong>`);
    else if (m[3] != null) out.push(html`<a href=${m[4]} target="_blank" rel="noopener noreferrer">${inline(m[3])}</a>`);
    else out.push(html`<em>${m[5]}</em>`);
    at = re.lastIndex;
  }
  if (at < text.length) out.push(text.slice(at));
  return out;
}

/** The Markdown this repository writes (headings, paragraphs, lists, tables), as nodes. */
export function Markdown({ text }) {
  const blocks = [];
  let para = [], list = [], table = [];
  const flush = () => {
    if (para.length) blocks.push(html`<p>${inline(para.join(" "))}</p>`);
    if (list.length) blocks.push(html`<ul>${list.map((t) => html`<li>${inline(t)}</li>`)}</ul>`);
    if (table.length) {
      const cells = (row) => row.replace(/^\||\|$/g, "").split("|").map((c) => c.trim());
      const [head, , ...rows] = table;
      blocks.push(html`<div class="about-table"><table><thead><tr>${cells(head).map((c) => html`<th>${inline(c)}</th>`)}</tr></thead>
        <tbody>${rows.map((r) => html`<tr>${cells(r).map((c) => html`<td>${inline(c)}</td>`)}</tr>`)}</tbody></table></div>`);
    }
    para = []; list = []; table = [];
  };
  for (const line of text.split(/\r?\n/)) {
    const h = /^(#{1,4})\s+(.*)$/.exec(line);
    if (h) { flush(); blocks.push(h[1].length <= 2 ? html`<h3>${inline(h[2])}</h3>` : html`<h4>${inline(h[2])}</h4>`); }
    else if (/^\s*\|/.test(line)) { if (para.length || list.length) { const t = table; flush(); table = t; } table.push(line.trim()); }
    else if (/^\s*[-*]\s+/.test(line)) { if (para.length || table.length) flush(); list.push(line.replace(/^\s*[-*]\s+/, "")); }
    else if (!line.trim() || /^-{3,}$/.test(line.trim())) flush();
    else if (list.length && /^\s+\S/.test(line)) list[list.length - 1] += " " + line.trim();
    else { if (list.length || table.length) flush(); para.push(line.trim()); }
  }
  flush();
  return html`<div class="about-md">${blocks}</div>`;
}

/** CHANGELOG.md's releases, newest first: [{title, version, body}], without the agents' notes. */
export function releases(text) {
  const parts = text.split(/^## /m).slice(1).map((p) => {
    const nl = p.indexOf("\n");
    const title = (nl < 0 ? p : p.slice(0, nl)).trim();
    return { title, version: /^\d+\.\d+\.\d+/.exec(title)?.[0] || null, body: nl < 0 ? "" : p.slice(nl + 1).trim() };
  });
  return parts.filter((p) => p.version || /^unreleased/i.test(p.title));
}

const fetchText = (path) => fetch(path, { cache: "no-store" }).then((r) => (r.ok ? r.text() : null)).catch(() => null);

export function About() {
  const lib = useStore(ui, (s) => s.library);
  const info = ctx.platform?.info || {};
  const version = info.version || "";
  const [files, setFiles] = useState(null);
  useEffect(() => {
    let live = true;
    Promise.all([fetchText("CHANGELOG.md"), fetchText("THIRD_PARTY.md"), fetchText("build-info.json")]).then(([log, third, build]) => {
      let b = null;
      try { b = build ? JSON.parse(build) : null; } catch { b = null; }
      if (live) setFiles({ log, third, build: b });
    });
    return () => { live = false; };
  }, []);
  const list = files?.log ? releases(files.log) : [];
  const released = list.some((r) => r.version === version);
  const shown = list.filter((r) => r.version || r.body).map((r) => r.version ? { ...r, label: r.title, now: r.version === version }
    : { ...r, label: released || !version ? "Coming in the next version" : `${version} (this version)`, now: !released && !!version });
  const b = files?.build;
  const rows = [
    ["Model Library", version || "unknown"],
    ["Running on", OS[info.os] || info.os || "unknown"],
    ["Library format", `${info.library_format ?? "?"} (this app writes)${lib?.format != null ? ` · the open library: ${lib.format}` : ""}`],
    ...(b ? [["Built", [b.date, b.commit].filter(Boolean).join(" · ")]] : []),
    ...(b?.parts || []).map((p) => [p.name, p.version]),
  ];
  return html`<div class="pages"><div class="library-settings about-page" id="about-page">
    <${PageHead} title="About" sub="Model Library: organise large collections of 3D model files in folders that make sense without the app." />
    <section class="ls-section" id="about-versions">
      <h2>Versions</h2>
      <dl class="about-versions">${rows.map(([k, v]) => html`<div key=${k}><dt>${k}</dt><dd>${v}</dd></div>`)}</dl>
      <p><a href=${RELEASES} target="_blank" rel="noopener noreferrer">All releases on GitHub</a></p>
    </section>
    <section class="ls-section" id="about-changelog">
      <h2>What's changed</h2>
      ${!files ? html`<p class="muted">Reading the changelog…</p>`
        : !shown.length ? html`<p class="muted">The changelog isn't included in this copy of the app (it's added when the app is built).</p>`
        : shown.map((r, i) => html`<details class=${`about-release${r.now ? " now" : ""}`} key=${r.label} open=${r.now || (!shown.some((x) => x.now) && i === 0)}>
            <summary><strong>${r.label}</strong>${r.now ? html` <span class="about-badge">This version</span>` : null}</summary>
            <${Markdown} text=${r.body} />
          </details>`)}
    </section>
    <section class="ls-section" id="about-roadmap">
      <h2>What's coming</h2>
      <ol class="phase-list">${NEXT.map(([g, what, now]) => html`<li class=${now ? "now" : ""} key=${g}><strong>${g}:</strong> ${what}${now ? " (in progress)" : ""}</li>`)}</ol>
      <details><summary>Done so far</summary><ol class="phase-list">${DONE.map(([g, what]) => html`<li key=${g}><strong>${g}:</strong> ${what}</li>`)}</ol></details>
    </section>
    <section class="ls-section" id="about-layout">
      <h2>How a library is laid out</h2>
      <p class="muted">Models go into folders laid out by your categories, so the library makes sense in any file manager, with or without this app.</p>
      <pre class="folder-tree" aria-label="Example folder layout">${TREE}</pre>
    </section>
    <section class="ls-section" id="about-licences">
      <h2>Licences and credits</h2>
      ${files?.third ? html`<${Markdown} text=${files.third.replace(/^# .*\n/, "")} />` : html`<p class="muted">The licence list isn't included in this copy of the app.</p>`}
    </section>
  </div></div>`;
}
