// The parts around the main view: the top bar's buttons and the status bar.
import { html, useEffect, useState } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, setPref, cycleTheme, resolvedTheme, nextTheme, THEMES } from "./state.js";
import { Icon } from "./icons.js";

const themeName = (t) => THEMES.find(([v]) => v === t)?.[1].replace(/ \(.*\)$/, "") || t;

export function TopButtons() {
  const theme = useStore(ui, (s) => s.theme);
  const nav = useStore(ui, (s) => ({ open: s.navOpen, collapsed: s.navCollapsed }));
  const [, force] = useState(0);
  useEffect(() => { const f = () => force((n) => n + 1); window.addEventListener("ml-theme", f); return () => window.removeEventListener("ml-theme", f); }, []);
  const cur = resolvedTheme();
  const next = nextTheme(theme);
  const icon = theme === "system" ? Icon.auto(17) : cur === "light" ? Icon.sun(17) : cur === "dark" ? Icon.moon(17) : Icon.night(17);
  const now = theme === "system" ? `Follow the system (${cur})` : themeName(theme);
  const toggleNav = () => {
    if (matchMedia("(max-width: 900px)").matches)
      ui.set({ navOpen: !ui.get().navOpen, mobilePanel: null });
    else setPref({ navCollapsed: !ui.get().navCollapsed });
  };
  return html`<button type="button" class="topicon nav-burger" aria-label=${nav.collapsed ? "Expand navigation" : "Toggle navigation"} aria-expanded=${nav.open ? "true" : "false"} title="Expand or collapse navigation" onClick=${toggleNav}>${Icon.menu(18)}</button>
    <button type="button" class="topicon" id="theme-toggle" data-theme=${theme} aria-label=${`Theme: ${now}. Switch to ${themeName(next)}`} title=${`Theme: ${now}. Click for ${themeName(next)}.`}
      onClick=${cycleTheme}>${icon}</button>`;
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
