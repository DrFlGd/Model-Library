// The parts around the main view: the menu's head and theme button, and the status bar.
import { html, useEffect, useState } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, setPref, cycleTheme, resolvedTheme, nextTheme, THEMES } from "./state.js";
import { Icon } from "./icons.js";

const themeName = (t) => (t === "system" ? "System" : THEMES.find(([v]) => v === t)?.[1].replace(/ \(.*\)$/, "") || t);

/** The menu's head: its fold button (the drawer's close button in a narrow
 *  window) and the app's mark, a link to Home. */
export function NavHead() {
  const nav = useStore(ui, (s) => ({ open: s.navOpen, collapsed: s.navCollapsed }));
  const toggleNav = () => {
    if (matchMedia("(max-width: 900px)").matches)
      ui.set({ navOpen: !ui.get().navOpen, mobilePanel: null });
    else setPref({ navCollapsed: !ui.get().navCollapsed });
  };
  return html`<div class="nav-head-row">
    <button type="button" class="ghost topicon nav-burger" aria-label=${matchMedia("(max-width: 900px)").matches ? "Close the menu" : nav.collapsed ? "Expand navigation" : "Collapse navigation"} aria-expanded=${nav.open ? "true" : "false"} title="Expand or collapse navigation" onClick=${toggleNav}>${Icon.menu(18)}</button>
    <a class="brand" href="#/" title="Home">
      <svg class="brand-mark icon" viewBox="0 0 40 40" aria-hidden="true">
        <g data-sk="classic" transform="scale(1.6667)"><path d="M3 8.5l9-4.5 9 4.5-9 4.5z"/><path d="M3 12.5l9 4.5 9-4.5"/><path d="M3 16.5l9 4.5 9-4.5" class="accent"/></g>
        <g data-sk="drafting" fill="none"><rect x="2.5" y="2.5" width="35" height="35" stroke="var(--ink)" stroke-width="1.5"/><path d="M20 8l10 5.5v11L20 30l-10-5.5v-11z" stroke="var(--tool)" stroke-width="1.6" stroke-linejoin="miter"/><path d="M10 13.5 20 19l10-5.5M20 19v11" stroke="var(--tool)" stroke-width="1.6"/><path d="M7 34h26M7 32.5v3M33 32.5v3" stroke="var(--ink)" stroke-width="1"/></g>
        <g data-sk="partsbin" stroke="none"><rect x="3" y="5" width="34" height="30" rx="2" fill="var(--ink)"/><rect x="7" y="9" width="26" height="9" fill="var(--tool)"/><path d="M10 13.5h3M15 13.5h3M20 13.5h7" stroke="var(--tool-ink)" stroke-width="2" stroke-linecap="square"/><rect x="15" y="24" width="10" height="4" rx="1" fill="var(--white)"/></g>
        <g data-sk="layerline" stroke="none"><path d="M13 3h14l-3 8h-8z" fill="var(--tool)"/><rect x="18.5" y="11" width="3" height="4" fill="var(--tool)"/><rect x="8" y="18" width="24" height="4.5" rx="2.25" fill="var(--ink)"/><rect x="6" y="24" width="28" height="4.5" rx="2.25" fill="var(--ink)"/><rect x="6" y="30" width="28" height="4.5" rx="2.25" fill="var(--ink)"/><rect x="12" y="18" width="7" height="4.5" rx="2.25" fill="var(--tool)" opacity=".85"/></g>
      </svg>
      <span class="nav-label">Model Library</span>
    </a>
  </div>`;
}

/** The theme button at the foot of the menu: the same four choices as Settings, in turn. */
export function ThemeButton() {
  const theme = useStore(ui, (s) => s.theme);
  const [, force] = useState(0);
  useEffect(() => { const f = () => force((n) => n + 1); window.addEventListener("ml-theme", f); return () => window.removeEventListener("ml-theme", f); }, []);
  const cur = resolvedTheme();
  const next = nextTheme(theme);
  const icon = theme === "system" ? Icon.auto(15) : cur === "light" ? Icon.sun(15) : cur === "dark" ? Icon.moon(15) : Icon.night(15);
  const now = theme === "system" ? `Follow the system (${cur})` : themeName(theme);
  return html`<li class="nav-row"><span class="nav-toggle-space"></span>
    <button type="button" class="nav-link nav-theme" id="theme-toggle" data-theme=${theme} aria-label=${`Theme: ${now}. Switch to ${themeName(next)}`} title=${`Theme: ${now}. Click for ${themeName(next)}.`}
      onClick=${cycleTheme}><span class="nav-icon">${icon}</span><span class="nav-label">Theme: ${themeName(theme === "system" ? "system" : theme)}</span></button></li>`;
}

function Elapsed({ since }) {
  const [, tick] = useState(0);
  useEffect(() => { const t = setInterval(() => tick((n) => n + 1), 1000); return () => clearInterval(t); }, []);
  return html`${Math.round((Date.now() - since) / 1000)} s`;
}

export function StatusBar() {
  const s = useStore(ui, (st) => ({ jobs: st.jobs, ready: st.ready, library: st.library }));
  const [open, setOpen] = useState(false);
  if (!s.ready) return null;
  const latest = s.jobs[s.jobs.length - 1];
  return html`<div class="statusbar-inner">
    <span class="status-library" title=${s.library?.path || ""}>${s.library ? `${s.library.name} · ${s.library.path}` : "No library open"}</span>
    ${s.library?.read_only ? html`<span>Read-only</span>` : null}
    <span class="status-jobs">
      ${latest ? html`<button type="button" class="status-job" aria-expanded=${open ? "true" : "false"} title="Show what's running" onClick=${() => setOpen(!open)}>
        <span class="dot busy"></span>${latest.label}… <${Elapsed} since=${latest.started} />${s.jobs.length > 1 ? ` · ${s.jobs.length} running` : ""}</button>`
        : html`<span class="status-idle"><span class="dot"></span>Ready</span>`}
      ${open && s.jobs.length ? html`<div class="jobs-panel" role="dialog" aria-label="Background work">
        ${s.jobs.map((j) => html`<div class="job" key=${j.id}><span>${j.label}</span><span class="muted"><${Elapsed} since=${j.started} /></span>
          ${j.cancel ? html`<button type="button" class="ghost" onClick=${() => j.cancel()}>Stop</button>` : null}</div>`)}
      </div>` : null}
    </span>
  </div>`;
}
