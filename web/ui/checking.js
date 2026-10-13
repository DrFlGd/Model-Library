// A review checks every file it would touch first (reading and hashing it). For
// a big folder or model that takes a while, so the check runs as a job: the
// dialog says how far it has got and has Stop, as the status bar does.
import { html, useState, useEffect, useRef } from "../lib/html.js";
import { api } from "./library.js";
import { addJob } from "./state.js";
import { size } from "./details.js";

const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;
/** "12 files · 1.4 GB read" */
export const checkedText = (p) => `${plural(p?.files || 0, "file", "files")} · ${size(p?.bytes || 0)} read`;

/** Run `cmd` (a plan or scan) with `args` as a check job. `onProgress({ files, bytes, name })`
 *  sees each poll. Returns { done, stop }: `done` resolves with the result, or null
 *  when it was stopped, and rejects with the check's error. */
export function runCheck(cmd, args, label, onProgress) {
  let id = null, stopped = false;
  const stop = () => { stopped = true; if (id) api("job_cancel", { id }).catch(() => {}); };
  const done = (async () => {
    const bar = addJob(label, stop);
    try {
      const { job } = await api(cmd, { ...args, job: true });
      id = job;
      if (stopped) stop();
      for (;;) {
        const j = await api("job", { id });
        if (j.progress?.files != null) {
          onProgress?.(j.progress);
          bar.update(`${label}: ${checkedText(j.progress)}`);
        }
        if (j.done) {
          if (j.error) throw new Error(j.error);
          // Stop wins even when the job finished before it heard: the person asked to stop.
          return stopped || j.result?.stopped ? null : j.result;
        }
        await new Promise((r) => setTimeout(r, 250));
      }
    } finally { bar(); }
  })();
  return { done, stop };
}

/** A review's check, run again (after `delay` ms) whenever `signature` changes,
 *  and stopped when it changes again or the dialog closes. `enabled: false` skips it.
 *  Returns { result, error, running, view }: `view` is the line for the dialog,
 *  progress with Stop while it runs, or Check again after a stop. */
export function useCheck(cmd, args, signature, { delay = 180, enabled = true, label = "Checking files" } = {}) {
  const blank = { signature: null, result: null, error: "", progress: null, stopped: false, running: false, started: 0 };
  const [st, setSt] = useState(blank);
  const [again, setAgain] = useState(0);
  const stopRef = useRef(null);
  useEffect(() => {
    if (!enabled) { setSt(blank); return; }
    let live = true, run = null;
    setSt({ ...blank, signature, running: true, started: Date.now() });
    const t = setTimeout(() => {
      run = runCheck(cmd, args, label, (progress) => { if (live) setSt((s) => ({ ...s, progress })); });
      stopRef.current = run.stop;
      run.done.then(
        (result) => { if (live) setSt({ ...blank, signature, result, stopped: result === null }); },
        (e) => { if (live) setSt({ ...blank, signature, error: e?.message || String(e) }); });
    }, delay);
    return () => { live = false; clearTimeout(t); run?.stop(); };
  }, [signature, enabled, again]);
  const current = st.signature === signature;
  // a check that finishes quickly shows nothing
  const slow = st.running && st.progress && Date.now() - st.started > 600;
  const view = current && slow
    ? html`<p class="check-progress" role="status"><span class="dot busy"></span>Checking files: ${checkedText(st.progress)}
        ${st.progress.name ? html` <span class="muted">${st.progress.name}</span>` : null}
        <button type="button" class="ghost" id="check-stop" onClick=${() => stopRef.current?.()}>Stop</button></p>`
    : current && st.stopped
      ? html`<p class="check-progress" role="status">Stopped before every file was checked.
          <button type="button" class="ghost" id="check-again" onClick=${() => setAgain((n) => n + 1)}>Check again</button></p>`
      : null;
  return { result: current ? st.result : null, error: current ? st.error : "", running: current && st.running, view };
}
