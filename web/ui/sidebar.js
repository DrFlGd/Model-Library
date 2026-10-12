// The menu on the left: the library's places, each schema with its category
// tree, and settings. On narrow windows it's a drawer. Right-click a category
// for the same Category menu as its page. The category or subcategory being
// shown has an "Add subcategory" row under it (docs/PLAN.md, "Group 1 design").
import { html, useState, useRef, useLayoutEffect } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, setPref } from "./state.js";
import { routeHash, schemaScope } from "./context.js";
import { Icon } from "./icons.js";
import { openMenu } from "./actions.js";
import { categoryItems, addSubcategory } from "./categories.js";
import { toast } from "./library.js";

function NavLink({ route, label, icon, count, current, depth = 0, toggle = null, menu = null }) {
  const on = current === route;
  return html`<li class="nav-row" style=${depth ? `--depth: ${depth}` : null}>
    ${toggle || html`<span class="nav-toggle-space"></span>`}
    <a class=${`nav-link${on ? " active" : ""}`} href=${routeHash(route)} title=${label} aria-label=${label} aria-current=${on ? "page" : null}
      onClick=${() => ui.set({ navOpen: false })} onContextMenu=${menu ? (e) => openMenu(e, menu()) : null}
      onKeyDown=${menu ? (e) => { if ((e.shiftKey && e.key === "F10") || e.key === "ContextMenu") { e.preventDefault(); e.stopPropagation(); openMenu(e.currentTarget, menu()); } } : null}>
      ${icon ? html`<span class="nav-icon">${Icon[icon](15)}</span>` : null}<span class="nav-label">${label}</span>
      ${count != null ? html`<span class="nav-count">${count}</span>` : null}</a></li>`;
}

/** "+ Add subcategory" under the category or subcategory shown: a row that
 *  becomes a name box; Enter adds it (with Undo), Esc or nothing typed cancels. */
function AddRow({ schemaId, path, depth, openKey }) {
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const input = useRef(null);
  useLayoutEffect(() => { if (editing) input.current?.focus(); }, [editing]);
  const stop = () => { setEditing(false); setName(""); };
  const submit = async () => {
    if (!name.trim()) return stop();
    setBusy(true);
    try {
      await addSubcategory(schemaId, path, name);
      setPref({ open: { ...ui.get().open, [openKey]: true } });
      stop();
    } catch (e) { toast(e.message || String(e), 6000); }
    finally { setBusy(false); }
  };
  return html`<li class="nav-row nav-add-row" style=${`--depth: ${depth}`}>
    ${editing ? html`<input ref=${input} id="nav-subcategory-name" class="nav-add-input" type="text" maxlength="80" placeholder="Subcategory name"
        aria-label="New subcategory name" value=${name} disabled=${busy} onInput=${(e) => setName(e.target.value)}
        onKeyDown=${(e) => { if (e.key === "Enter") { e.preventDefault(); submit(); } else if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); stop(); } }}
        onBlur=${() => { if (!busy && !name.trim()) stop(); }} />`
      : html`<button type="button" class="nav-link nav-add-sub" id="nav-add-subcategory" title="Add a subcategory here" onClick=${() => setEditing(true)}>
        <span class="nav-icon">${Icon.plus(13)}</span><span class="nav-label">Add subcategory</span></button>`}
  </li>`;
}

/** A schema's category values, nested, each unfolding to the next level. */
function TreeRows({ schema, nodes, values, depth, current, open, canAdd }) {
  return nodes.map((n) => {
    const path = [...values, n.value];
    const key = [schema.id, ...path].join("/");
    const unfolded = !!open[key];
    const toggle = n.children.length ? html`<button type="button" class="nav-toggle" aria-expanded=${unfolded ? "true" : "false"} aria-label=${`${unfolded ? "Fold" : "Unfold"} ${n.value}`}
      onClick=${() => setPref({ open: { ...open, [key]: !unfolded } })}>${Icon.chevron(14)}</button>` : null;
    const route = `browse:${schemaScope(schema.id, path)}`;
    return html`<${NavLink} key=${key} route=${route} label=${n.value} count=${n.count} depth=${depth} current=${current} toggle=${toggle}
        menu=${() => categoryItems(schema.id, path)} />
      ${unfolded ? html`<${TreeRows} schema=${schema} nodes=${n.children} values=${path} depth=${depth + 1} current=${current} open=${open} canAdd=${canAdd} />` : null}
      ${canAdd && current === route ? html`<${AddRow} key=${`${key}+`} schemaId=${schema.id} path=${path} depth=${depth + 1} openKey=${key} />` : null}`;
  });
}

export function Sidebar() {
  const s = useStore(ui, (st) => ({ route: st.route, navOpen: st.navOpen, overview: st.overview, open: st.open, library: st.library, navCollapsed: st.navCollapsed }));
  const ov = s.overview;
  const canAdd = !!s.library && !s.library.read_only;
  return html`${s.navOpen ? html`<div class="nav-backdrop" onClick=${() => ui.set({ navOpen: false })}></div>` : null}
    <nav class=${`sidebar${s.navOpen ? " open" : ""}${s.navCollapsed ? " collapsed" : ""}`} aria-label="Library">
      <ul class="nav-list">
        <${NavLink} route="home" label="Home" icon="home" current=${s.route} />
        <${NavLink} route="import" label="Import" icon="inbox" current=${s.route} />
        <${NavLink} route="browse:all" label="All models" icon="grid" count=${ov?.all} current=${s.route} />
        <${NavLink} route="browse:unsorted" label="Unsorted" icon="box" count=${ov?.unsorted} current=${s.route} />
        <${NavLink} route="browse:favs" label="Starred" icon="star" count=${ov?.starred} current=${s.route} />
      </ul>
      <p class="nav-head">Categories</p>
      ${ov?.schemas?.length ? html`<ul class="nav-list" id="schema-tree">${ov.schemas.map((sc) => {
        const key = sc.id;
        const unfolded = s.open[key] !== false; // schemas start unfolded
        const toggle = sc.tree.length ? html`<button type="button" class="nav-toggle" aria-expanded=${unfolded ? "true" : "false"} aria-label=${`${unfolded ? "Fold" : "Unfold"} ${sc.name}`}
          onClick=${() => setPref({ open: { ...s.open, [key]: !unfolded } })}>${Icon.chevron(14)}</button>` : null;
        const route = `browse:${schemaScope(sc.id)}`;
        return html`<${NavLink} key=${key} route=${route} label=${sc.name} icon="layers" count=${sc.count} current=${s.route} toggle=${toggle}
            menu=${() => categoryItems(sc.id, [])} />
          ${unfolded ? html`<${TreeRows} schema=${sc} nodes=${sc.tree} values=${[]} depth=${1} current=${s.route} open=${s.open} canAdd=${canAdd} />` : null}
          ${canAdd && s.route === route ? html`<${AddRow} key=${`${key}+`} schemaId=${sc.id} path=${[]} depth=${1} openKey=${key} />` : null}`;
      })}</ul>` : html`<p class="muted nav-note">None yet.</p>`}
      ${s.library && !s.library.read_only ? html`<button type="button" class="ghost nav-add" id="new-schema" onClick=${() => ui.set({ dialog: { type: "new-schema" }, navOpen: false })}>${Icon.plus(14)} New category…</button>` : null}
      <ul class="nav-list nav-foot">
        <${NavLink} route="duplicates" label="Duplicates" icon="stack" current=${s.route} />
        <${NavLink} route="settings" label="Settings" icon="cog" current=${s.route} />
        <${NavLink} route="about" label="About" icon="info" current=${s.route} />
      </ul>
    </nav>`;
}
