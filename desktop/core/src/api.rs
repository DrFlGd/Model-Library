//! The app's commands, behind one entry point ([`App::call`]) so the Tauri app
//! and `modlib-cli serve` (the tests' stand-in now, the Docker server later)
//! share them exactly.

use crate::config::{AppConfig, Prefs};
use crate::index::Index;
use crate::library::{self, Library};
use crate::sort::Session;
use crate::{archive, dupes, import, mesh, model, relayout, schema, thumb};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

/// Where the app keeps things on this computer.
#[derive(Clone, Debug)]
pub struct AppPaths {
    /// config.json and prefs.json
    pub config_dir: PathBuf,
    /// caches and the search index (index/<library id>.json)
    pub data_dir: PathBuf,
    /// how the page reaches library files: "library://localhost/", "/library/"...
    pub library_url: String,
}

pub enum Reply {
    Json(Value),
    Bytes(Vec<u8>),
}

/// Long work (importing) runs in the background; the page polls `job`.
#[derive(Clone, Default)]
struct Job {
    id: String,
    label: String,
    done: bool,
    error: Option<String>,
    result: Value,
    progress: Value,
    started: String,
    cancel: Arc<AtomicBool>,
}

impl Job {
    fn to_json(&self) -> Value {
        json!({ "id": self.id, "label": self.label, "done": self.done, "error": self.error, "result": self.result, "progress": self.progress, "started": self.started })
    }
}

pub struct App {
    pub paths: AppPaths,
    library: RwLock<Option<Library>>,
    /// the open library's index, built on first use (one build at a time)
    index: tokio::sync::Mutex<Option<Index>>,
    jobs: Mutex<Vec<Job>>,
    job_seq: Mutex<u64>,
    /// one import at a time
    busy: tokio::sync::Mutex<()>,
    /// the sorting workspace of the open library (kept in the data folder)
    sort: Mutex<Option<Session>>,
    /// while its folders are read or its models imported, it isn't changed otherwise
    sort_busy: Arc<AtomicBool>,
    /// the workspace as it was before each of the last changes, newest last, for
    /// undo: (number, workspace); kept in memory only
    sort_undo: Mutex<Vec<(u64, Session)>>,
    sort_seq: AtomicU64,
    /// previews drawn at once
    renders: tokio::sync::Semaphore,
    /// the library folder being watched for changes made outside the app
    watched: Mutex<Option<PathBuf>>,
    watcher: Mutex<Option<crate::watch::Watcher>>,
    /// goes up each time the library is found changed (the page asks for it)
    changes: AtomicU64,
}

/// Clears the workspace's busy flag however the job ends.
struct NotBusy(Arc<AtomicBool>);
impl Drop for NotBusy {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(String::from)
        .collect()
}

/// What `library()` says before any library was ever opened (the page then asks where).
const NO_LIBRARY: &str = "No library is open yet.";

/// How many earlier states of the sorting workspace are kept for undo.
const SORT_UNDO: usize = 20;

/// The preference key the page keeps favourites under; they're stored in the library.
const FAVS_KEY: &str = "ml-favs";

/// What a model's first model.json starts from: what its folder and the index say.
fn first_sidecar(m: &crate::index::Model) -> Value {
    json!({ "name": m.v["name"], "schema": m.v["schema"], "path": if m.v["schema"].is_null() { Value::Null } else { m.v["path"].clone() },
        "authors": m.v["authors"].as_array().filter(|a| !a.is_empty()).map(|a| a.iter().map(|n| json!({ "name": n })).collect::<Vec<_>>()) })
}

/// A model's details as they are before a journalled edit: its model.json, made
/// first if it has none (so its id, and star, stay the same after an undo).
fn details_before(dir: &Path, m: &crate::index::Model) -> Result<Value> {
    model::update(dir, &json!({}), &first_sidecar(m))?;
    Ok(model::read_sidecar(dir))
}

fn e2s(e: anyhow::Error) -> String {
    format!("{e:#}")
}

fn arg<'a>(args: &'a Value, k: &str) -> Result<&'a str> {
    args[k].as_str().ok_or_else(|| anyhow!("missing {k}"))
}

impl App {
    pub fn new(paths: AppPaths) -> Result<Arc<Self>> {
        Ok(Arc::new(Self {
            paths,
            library: RwLock::new(None),
            index: tokio::sync::Mutex::new(None),
            jobs: Mutex::new(vec![]),
            job_seq: Mutex::new(0),
            busy: tokio::sync::Mutex::new(()),
            sort: Mutex::new(None),
            sort_busy: Arc::new(AtomicBool::new(false)),
            sort_undo: Mutex::new(vec![]),
            sort_seq: AtomicU64::new(0),
            renders: tokio::sync::Semaphore::new(2),
            watched: Mutex::new(None),
            watcher: Mutex::new(None),
            changes: AtomicU64::new(0),
        }))
    }

    fn config_path(&self) -> PathBuf {
        self.paths.config_dir.join("config.json")
    }
    pub fn config(&self) -> AppConfig {
        AppConfig::load(&self.config_path())
    }
    fn prefs(&self) -> Prefs {
        Prefs::new(self.paths.config_dir.join("prefs.json"))
    }

    /// The open library (opening the configured one on first use). On the very
    /// first start there's none: the page asks where it should go.
    pub fn library(&self) -> Result<Library, String> {
        if let Some(l) = self.library.read().unwrap().as_ref() {
            return Ok(l.clone());
        }
        let Some(path) = self.config().library else {
            return Err(NO_LIBRARY.into());
        };
        self.open_library(&path).map_err(e2s)
    }

    fn open_library(&self, path: &Path) -> Result<Library> {
        let lib = Library::open(path)?;
        // categories from before subcategory trees become trees (nothing moves)
        schema::upgrade_all(&lib);
        let mut cfg = self.config();
        if cfg.library.as_deref() != Some(path)
            || cfg.recent_libraries.first().map(PathBuf::as_path) != Some(path)
        {
            cfg.set_library(path);
            cfg.save(&self.config_path())?;
        }
        *self.library.write().unwrap() = Some(lib.clone());
        if let Ok(mut ix) = self.index.try_lock() {
            *ix = None;
        }
        Ok(lib)
    }

    /// Where the index of `lib` is cached on this computer (by the library's id, so a
    /// moved library finds it).
    fn index_cache(&self, lib: &Library) -> Option<PathBuf> {
        let id = lib.meta()["id"].as_str().map(String::from)?;
        library::valid_id(&id).ok()?;
        Some(self.paths.data_dir.join("index").join(format!("{id}.json")))
    }

    /// Run `f` on the open library's index, reading the library first if needed
    /// (or again, with `rescan`: Some(full)).
    async fn with_index<T>(
        &self,
        rescan: Option<bool>,
        f: impl FnOnce(&mut Index, &Library) -> T,
    ) -> Result<T, String> {
        let lib = self.library()?;
        let mut g = self.index.lock().await;
        let stale = g.as_ref().is_none_or(|ix| ix.root != lib.root());
        if stale || rescan.is_some() {
            let (l, cache, full) = (lib.clone(), self.index_cache(&lib), rescan == Some(true));
            let ix = tokio::task::spawn_blocking(move || Index::build(&l, cache.as_deref(), full))
                .await
                .map_err(|e| e.to_string())?;
            *g = Some(ix);
        }
        Ok(f(g.as_mut().unwrap(), &lib))
    }

    // ------------------------------------------------------------ changes made outside the app

    /// Watch the open library's folder (once per library); when something changes
    /// there, read it again.
    fn ensure_watch(self: &Arc<Self>) {
        let Ok(lib) = self.library() else { return };
        let root = lib.root().to_path_buf();
        {
            let mut w = self.watched.lock().unwrap();
            if w.as_ref() == Some(&root) {
                return;
            }
            *w = Some(root.clone());
        }
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let app = Arc::downgrade(self);
        // watching a big tree takes a while on some systems: not on the caller's time
        tokio::task::spawn_blocking(move || {
            let on = app.clone();
            let r = crate::watch::watch(&root, crate::watch::SETTLE, move |paths| {
                if let Some(a) = on.upgrade() {
                    handle.spawn(async move {
                        if let Err(e) = a.check_changes(paths).await {
                            eprintln!("reading changes: {e}");
                        }
                    });
                }
            });
            let Some(a) = app.upgrade() else { return };
            match r {
                // only if it's still the open library
                Ok(w) if a.watched.lock().unwrap().as_ref() == Some(&root) => {
                    *a.watcher.lock().unwrap() = Some(w);
                }
                Ok(_) => {}
                Err(e) => eprintln!("can't watch {}: {e}", root.display()),
            }
        });
    }

    /// Read the library again: only models whose folder changed (`paths`: and
    /// those holding a changed path), and tell a model.json moved by hand its new
    /// place. Waits for a running job; does nothing until the library was read once.
    async fn check_changes(&self, paths: Option<Vec<PathBuf>>) -> Result<Value, String> {
        // not while a job is moving folders (it reads what it changed itself)
        let _busy = self.busy.lock().await;
        let lib = self.library()?;
        let cache = self.index_cache(&lib);
        let mut g = self.index.lock().await;
        let Some(old) = g.as_ref().filter(|ix| ix.root == lib.root()) else {
            return Ok(json!({ "rev": self.changes.load(Ordering::Relaxed), "changed": false }));
        };
        let places = |ix: &Index| -> Vec<(String, String)> {
            let mut v: Vec<(String, String)> = ix
                .models
                .iter()
                .map(|m| (m.id().to_string(), m.rel().to_string()))
                .collect();
            v.sort();
            v
        };
        let before = places(old);
        let l = lib.clone();
        let (changed, ix) = tokio::task::spawn_blocking(move || {
            let mut ix = Index::build(&l, cache.as_deref(), false);
            let mut touched: Vec<String> = vec![];
            if let Some(paths) = &paths {
                let by_rel: std::collections::HashMap<&str, &str> =
                    ix.models.iter().map(|m| (m.rel(), m.id())).collect();
                let mut ids = std::collections::HashSet::new();
                for p in paths {
                    let Some(rel) = l.relative(p) else { continue };
                    let mut r = rel.as_str();
                    loop {
                        if let Some(id) = by_rel.get(r) {
                            ids.insert(id.to_string());
                            break;
                        }
                        match r.rsplit_once('/') {
                            Some((up, _)) => r = up,
                            None => break,
                        }
                    }
                }
                touched.extend(ids);
            }
            if l.writable().is_ok() {
                for m in ix.models.iter().filter(|m| m.v["moved"] == true) {
                    if let Ok(dir) = l.resolve(m.rel()) {
                        if model::set_place(&dir, m.v["schema"].as_str(), &strings(&m.v["path"]))
                            .is_ok()
                        {
                            touched.push(m.id().to_string());
                        }
                    }
                }
            }
            for id in &touched {
                ix.refresh(&l, id);
            }
            if let (Some(c), false) = (&cache, touched.is_empty()) {
                let _ = ix.save(c);
            }
            (!touched.is_empty() || ix.read > 0, ix)
        })
        .await
        .map_err(|e| e.to_string())?;
        let changed = changed || places(&ix) != before;
        *g = Some(ix);
        if changed {
            self.changes.fetch_add(1, Ordering::Relaxed);
        }
        Ok(json!({ "rev": self.changes.load(Ordering::Relaxed), "changed": changed }))
    }

    // ------------------------------------------------------------ jobs

    fn start_job(&self, label: &str) -> Job {
        let mut seq = self.job_seq.lock().unwrap();
        *seq += 1;
        let job = Job {
            id: format!("job{}", *seq),
            label: label.into(),
            started: library::now(),
            ..Default::default()
        };
        let mut jobs = self.jobs.lock().unwrap();
        // keep running jobs and the last few finished ones
        let finished = jobs.iter().filter(|j| j.done).count();
        let mut drop_n = finished.saturating_sub(20);
        jobs.retain(|j| {
            let gone = j.done && drop_n > 0;
            if gone {
                drop_n -= 1;
            }
            !gone
        });
        jobs.push(job.clone());
        job
    }

    fn job_progress(&self, id: &str, progress: Value) {
        if let Some(j) = self.jobs.lock().unwrap().iter_mut().find(|j| j.id == id) {
            j.progress = progress;
        }
    }

    fn job_done(&self, id: &str, r: Result<Value, String>) {
        if let Some(j) = self.jobs.lock().unwrap().iter_mut().find(|j| j.id == id) {
            j.done = true;
            match r {
                Ok(v) => j.result = v,
                Err(e) => j.error = Some(e),
            }
        }
    }

    /// Run `work` on a blocking thread as a job, then read the library again.
    fn spawn_job<F>(self: &Arc<Self>, label: &str, work: F) -> Value
    where
        F: FnOnce(Arc<Self>, String, Arc<AtomicBool>) -> Result<Value> + Send + 'static,
    {
        self.spawn_job_with(label, true, work)
    }

    /// As `spawn_job`; `reread`: read the library again after it.
    fn spawn_job_with<F>(self: &Arc<Self>, label: &str, reread: bool, work: F) -> Value
    where
        F: FnOnce(Arc<Self>, String, Arc<AtomicBool>) -> Result<Value> + Send + 'static,
    {
        let job = self.start_job(label);
        let (app, id, cancel) = (self.clone(), job.id.clone(), job.cancel.clone());
        tokio::spawn(async move {
            let _g = app.busy.lock().await;
            let (a, jid) = (app.clone(), id.clone());
            let r = tokio::task::spawn_blocking(move || work(a, jid, cancel)).await;
            let r = match r {
                Ok(r) => r.map_err(e2s),
                Err(e) => Err(e.to_string()),
            };
            if reread {
                let _ = app.with_index(Some(false), |_, _| ()).await;
            }
            // models a job changed inside their folders: read them again (a folder's
            // time doesn't always change on Windows, so the cache can't tell)
            if let Ok(v) = &r {
                let ids: Vec<String> = v["refresh"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect();
                if !ids.is_empty() {
                    app.refresh_models(&ids).await;
                }
            }
            app.job_done(&id, r);
        });
        json!({ "job": job.id })
    }

    /// Read some models again from their folders, and save the cache.
    async fn refresh_models(&self, ids: &[String]) {
        let Ok(lib) = self.library() else { return };
        let cache = self.index_cache(&lib);
        let _ = self
            .with_index(None, |ix, lib| {
                for id in ids {
                    ix.refresh(lib, id);
                }
                if let Some(c) = &cache {
                    let _ = ix.save(c);
                }
            })
            .await;
    }

    /// Import planned items one by one (a failed one doesn't stop the rest).
    #[allow(clippy::too_many_arguments)]
    fn run_import(
        &self,
        jid: &str,
        cancel: &AtomicBool,
        lib: &Library,
        items: &[Value],
        dests: &[PathBuf],
        schemas: &std::collections::HashMap<String, schema::Schema>,
        ids: &std::collections::HashSet<String>,
        mv: bool,
        force_copy: bool,
    ) -> Vec<Value> {
        let total_bytes: u64 = items
            .iter()
            .map(
                |it| match it["files"].as_array().filter(|f| !f.is_empty()) {
                    Some(fs) => fs
                        .iter()
                        .filter_map(Value::as_str)
                        .map(|f| import::tree_bytes(Path::new(f)))
                        .sum(),
                    None => import::tree_bytes(Path::new(it["source"].as_str().unwrap_or(""))),
                },
            )
            .sum();
        let done_bytes = std::sync::atomic::AtomicU64::new(0);
        let mut results = vec![];
        for (i, (it, dest)) in items.iter().zip(dests).enumerate() {
            let name = it["name"].as_str().unwrap_or("").to_string();
            let report = |bytes: u64| {
                self.job_progress(jid, json!({ "item": i, "items": items.len(), "name": name, "bytes": bytes, "total_bytes": total_bytes }));
            };
            report(done_bytes.load(Ordering::Relaxed));
            if cancel.load(Ordering::Relaxed) {
                results.push(json!({ "source": it["source"], "name": name, "error": "Stopped before this one." }));
                continue;
            }
            let on_bytes = |n: u64| report(done_bytes.fetch_add(n, Ordering::Relaxed) + n);
            let p = import::Progress {
                cancel,
                on_bytes: &on_bytes,
            };
            let schema = it["schema"].as_str().and_then(|s| schemas.get(s));
            // what the folder had before, so undoing the import can put it back as it was
            let src = PathBuf::from(it["source"].as_str().unwrap_or(""));
            let loose = it["files"].as_array().is_some_and(|f| !f.is_empty());
            let side = src.join(model::SIDECAR);
            let before = json!({
                "sidecar_before": if !loose && side.is_file() { model::read_sidecar(&src) } else { Value::Null },
                "thumb_before": !loose && thumb::has(&src),
            });
            match import::commit_one(lib, it, dest, schema, mv, force_copy, ids, &p) {
                Ok(mut v) => {
                    v["source"] = it["source"].clone();
                    v["name"] = json!(name);
                    v["before"] = before;
                    results.push(v);
                }
                Err(e) => {
                    results.push(json!({ "source": it["source"], "name": name, "error": e2s(e) }))
                }
            }
        }
        // previews for the new models
        let made: Vec<String> = results
            .iter()
            .filter_map(|r| r["dest"].as_str().map(String::from))
            .collect();
        for (i, d) in made.iter().enumerate() {
            self.job_progress(jid, json!({ "item": i, "items": made.len(), "name": "Making previews", "bytes": total_bytes, "total_bytes": total_bytes }));
            let dir = PathBuf::from(d);
            if !thumb::has(&dir) {
                if let Err(e) = thumb::make(&dir) {
                    eprintln!("preview of {d}: {e:#}");
                }
            }
        }
        self.job_progress(jid, json!({ "item": items.len(), "items": items.len(), "bytes": total_bytes, "total_bytes": total_bytes }));
        results
    }

    /// A model's folder and index entry, by id.
    async fn model_dir(&self, args: &Value) -> Result<(PathBuf, Value), String> {
        let id = arg(args, "id").map_err(e2s)?.to_string();
        let found = self
            .with_index(None, |ix, lib| {
                ix.get(&id).map(|m| (lib.resolve(m.rel()), m.v.clone()))
            })
            .await?;
        let (dir, v) = found.ok_or("That model isn't in the library any more.")?;
        Ok((dir.map_err(e2s)?, v))
    }

    /// A file shown on a model page or in the workspace: { id, file } (a library
    /// model's) or { sort, file } (a workspace model's).
    async fn file_path(&self, args: &Value) -> Result<PathBuf, String> {
        let file = arg(args, "file").map_err(e2s)?;
        if let Some(sid) = args["sort"].as_str() {
            return self.with_sort(false, |se| se.file_at(sid, file));
        }
        let (dir, _) = self.model_dir(args).await?;
        Ok(dir.join(crate::library::rel_inside(file).map_err(e2s)?))
    }

    // ------------------------------------------------------------ duplicates

    /// Where the open library's last search for duplicates is kept, and the hashes
    /// of its models that have no model.json.
    fn dupes_files(&self, lib: &Library) -> Result<(PathBuf, PathBuf), String> {
        let id = lib.meta()["id"].as_str().unwrap_or("").to_string();
        library::valid_id(&id).map_err(e2s)?;
        let dir = self.paths.data_dir.join("duplicates");
        Ok((
            dir.join(format!("{id}.json")),
            dir.join(format!("{id}-hashes.json")),
        ))
    }

    async fn dupes_call(self: &Arc<Self>, cmd: &str, args: Value) -> Result<Reply, String> {
        let j = |v: Value| Ok(Reply::Json(v));
        let lib = self.library()?;
        let (saved, cache) = self.dupes_files(&lib)?;
        match cmd {
            "dupes_get" => {
                let found = crate::config::read_json_object(&saved);
                j(self
                    .with_index(None, |ix, lib| dupes::view(lib, ix, &found))
                    .await?)
            }
            "dupes_find" => {
                let models = self.with_index(None, |ix, _| ix.models.clone()).await?;
                j(
                    self.spawn_job("Looking for duplicates", move |app, jid, cancel| {
                        let r = dupes::find(&lib, &models, &cache, &cancel, &|p| {
                            app.job_progress(&jid, p)
                        })?;
                        crate::config::write_json(&saved, &r)?;
                        Ok(json!({
                            "groups": r["groups"].as_array().map_or(0, Vec::len),
                            "shared": r["shared_count"], "wasted": r["wasted"],
                            // model.json files that got hashes: read them again
                            "refresh": r["refresh"],
                        }))
                    }),
                )
            }
            "dupes_set_aside" => {
                lib.writable().map_err(e2s)?;
                let ids = strings(&args["ids"]);
                let plan = self
                    .with_index(None, |ix, lib| dupes::set_aside_plan(lib, ix, &ids))
                    .await?
                    .map_err(e2s)?;
                let id = relayout::start(&lib, &plan).map_err(e2s)?;
                j(self.spawn_job(
                    plan["label"].as_str().unwrap_or("Setting copies aside"),
                    move |app, jid, cancel| {
                        relayout::apply(&lib, &id, &cancel, &|i, n, name| {
                            app.job_progress(&jid, json!({ "item": i, "items": n, "name": name }))
                        })
                    },
                ))
            }
            "dupes_empty" => {
                let _g = self
                    .busy
                    .try_lock()
                    .map_err(|_| "Wait until the job that's running has finished.".to_string())?;
                j(dupes::empty_set_aside(&lib).map_err(e2s)?)
            }
            _ => Err(format!("unknown command {cmd}")),
        }
    }

    // ------------------------------------------------------------ the sorting workspace

    /// Where the open library's workspace is kept: (library id, file).
    fn sort_file(&self, lib: &Library) -> Result<(String, PathBuf), String> {
        let id = lib.meta()["id"].as_str().unwrap_or("").to_string();
        library::valid_id(&id).map_err(e2s)?;
        let path = self
            .paths
            .data_dir
            .join("sorting")
            .join(format!("{id}.json"));
        Ok((id, path))
    }

    /// Run `f` on the workspace (`mutate`: and keep what it changed).
    fn with_sort<T>(
        &self,
        mutate: bool,
        f: impl FnOnce(&mut Session) -> Result<T>,
    ) -> Result<T, String> {
        if mutate && self.sort_busy.load(Ordering::Relaxed) {
            return Err(
                "The workspace is busy: wait until its folders are read or its models imported."
                    .into(),
            );
        }
        let lib = self.library()?;
        let (id, path) = self.sort_file(&lib)?;
        let mut g = self.sort.lock().unwrap();
        if g.as_ref().is_none_or(|se| se.library != id) {
            *g = Some(Session::load(&path, &id));
            self.sort_undo.lock().unwrap().clear();
        }
        let se = g.as_mut().unwrap();
        let r = f(se).map_err(e2s)?;
        if mutate {
            se.save(&path).map_err(e2s)?;
        }
        Ok(r)
    }

    fn sort_view(&self) -> Result<Value, String> {
        let busy = self.sort_busy.load(Ordering::Relaxed);
        let v = self.with_sort(false, |se| {
            let mut v = se.to_json();
            v["busy"] = json!(busy);
            Ok(v)
        });
        v.map(|mut v| {
            // the change the page can undo next
            v["undo"] = json!(self.sort_undo.lock().unwrap().last().map(|u| u.0));
            v
        })
    }

    /// Keep the workspace as it was before a change, for undo.
    fn sort_keep(&self, before: Session) -> u64 {
        let n = self.sort_seq.fetch_add(1, Ordering::Relaxed) + 1;
        let mut u = self.sort_undo.lock().unwrap();
        u.push((n, before));
        let extra = u.len().saturating_sub(SORT_UNDO);
        u.drain(..extra);
        n
    }

    /// A change to the workspace that can be undone (`sort_undo`).
    fn sort_change<T>(&self, f: impl FnOnce(&mut Session) -> Result<T>) -> Result<T, String> {
        let mut before = None;
        let r = self.with_sort(true, |se| {
            let copy = se.clone();
            match f(se) {
                Ok(r) => {
                    before = Some(copy);
                    Ok(r)
                }
                Err(e) => {
                    *se = copy;
                    Err(e)
                }
            }
        })?;
        if let Some(se) = before {
            self.sort_keep(se);
        }
        Ok(r)
    }

    /// Put a workspace a job made in place of the one kept (the one before can be
    /// put back with `sort_undo`).
    fn sort_replace(&self, se: Session) -> Result<()> {
        let lib = self.library().map_err(|e| anyhow!(e))?;
        let (id, path) = self.sort_file(&lib).map_err(|e| anyhow!(e))?;
        if se.library != id {
            anyhow::bail!("Another library was opened meanwhile.");
        }
        se.save(&path)?;
        let before = self.sort.lock().unwrap().replace(se);
        if let Some(before) = before.filter(|b| b.library == id) {
            self.sort_keep(before);
        }
        Ok(())
    }

    async fn sort_ctx(&self) -> Result<import::Ctx, String> {
        self.with_index(None, |ix, lib| import::Ctx::new(lib, ix))
            .await
    }

    /// Draw a preview of one 3D file (kept in the data folder): its URL.
    async fn preview_of(&self, path: PathBuf, entry: Option<String>) -> Result<String, String> {
        let _permit = self.renders.acquire().await.map_err(|e| e.to_string())?;
        let cache = self.paths.data_dir.join("previews");
        let name = tokio::task::spawn_blocking(move || {
            thumb::file_preview(&cache, &path, entry.as_deref())
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(e2s)?;
        Ok(format!("~preview/{name}"))
    }

    async fn sort_call(self: &Arc<Self>, cmd: &str, args: Value) -> Result<Reply, String> {
        let j = |v: Value| Ok(Reply::Json(v));
        let ids = strings(&args["ids"]);
        let folders = strings(&args["folders"]);
        match cmd {
            "sort_get" => j(self.sort_view()?),
            "sort_add" | "sort_rescan" => {
                let paths: Vec<PathBuf> = strings(&args["paths"])
                    .into_iter()
                    .map(PathBuf::from)
                    .collect();
                let rescan = cmd == "sort_rescan";
                if !rescan && paths.is_empty() {
                    return Err("Choose a folder or file.".into());
                }
                let contents = args["contents"].as_bool() != Some(false);
                let ctx = self.sort_ctx().await?;
                let mut se = self.with_sort(true, |se| Ok(se.clone()))?;
                if self.sort_busy.swap(true, Ordering::Relaxed) {
                    return Err("The workspace is busy: wait until its folders are read or its models imported.".into());
                }
                let guard = NotBusy(self.sort_busy.clone());
                let label = if rescan {
                    "Reading the folders again".to_string()
                } else {
                    format!(
                        "Reading {}",
                        paths
                            .iter()
                            .map(|p| import::file_name(p))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                j(self.spawn_job_with(&label, false, move |app, jid, cancel| {
                    let _guard = guard;
                    let progress = |n: usize, name: &str| {
                        app.job_progress(&jid, json!({ "read": n, "name": name }))
                    };
                    if rescan {
                        se.rescan(&ctx, &cancel, &progress)?;
                    } else {
                        se.add(&ctx, &paths, contents, &cancel, &progress)?;
                    }
                    let n = se.items.len();
                    app.sort_replace(se)?;
                    Ok(json!({ "items": n }))
                }))
            }
            "sort_undo" => {
                // put back the workspace as it was before its last change; `rev`:
                // only if that change is still the last one
                let prev = {
                    let mut u = self.sort_undo.lock().unwrap();
                    match (u.last(), args["rev"].as_u64()) {
                        (None, _) => return Err("There's nothing to undo in the workspace.".into()),
                        (Some((n, _)), Some(rev)) if *n != rev => {
                            return Err(
                                "That can't be undone any more: the workspace has changed since."
                                    .into(),
                            )
                        }
                        _ => u.pop().unwrap(),
                    }
                };
                let r = self.with_sort(true, |se| {
                    if se.library != prev.1.library {
                        anyhow::bail!("Another library was opened meanwhile.");
                    }
                    *se = prev.1.clone();
                    Ok(())
                });
                if let Err(e) = r {
                    self.sort_undo.lock().unwrap().push(prev);
                    return Err(e);
                }
                j(self.sort_view()?)
            }
            "sort_send" => {
                let schema = args["schema"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(String::from);
                if let Some(id) = &schema {
                    let ok = self
                        .with_index(None, |ix, _| ix.schema(id).is_some())
                        .await?;
                    if !ok {
                        return Err(format!("There's no category {id} any more."));
                    }
                }
                let values = strings(&args["values"]);
                let (keep, keep_self) = (
                    args["keep"] == json!(true),
                    args["keep_self"] != json!(false),
                );
                let n = self.sort_change(|se| {
                    se.send(&ids, &folders, schema.as_deref(), &values, keep, keep_self)
                })?;
                let mut v = self.sort_view()?;
                v["changed"] = json!(n);
                j(v)
            }
            "sort_update" => {
                self.sort_change(|se| se.update(&ids, &folders, &args["patch"]))?;
                j(self.sort_view()?)
            }
            "sort_join" => {
                // a folder, several (`folders`), or every folder at a folder's level (`level`)
                let ctx = self.sort_ctx().await?;
                let level = args["level"] == json!(true);
                let one = args["folder"].as_str().map(String::from);
                let (ids, left) = self.sort_change(|se| {
                    let dirs = match (&one, level) {
                        (Some(d), true) => se.level_of(d),
                        (Some(d), false) => vec![d.clone()],
                        (None, _) => folders.clone(),
                    };
                    if dirs.is_empty() {
                        anyhow::bail!("Choose a folder.");
                    }
                    se.join_many(&ctx, &dirs)
                })?;
                let mut v = self.sort_view()?;
                v["id"] = json!(ids.first());
                v["ids"] = json!(ids);
                v["left"] = json!(left);
                j(v)
            }
            "sort_group" | "sort_split" => {
                let ctx = self.sort_ctx().await?;
                let id = self.sort_change(|se| match cmd {
                    "sort_group" => se.group(
                        &ctx,
                        &ids,
                        &folders,
                        &strings(&args["files"]),
                        args["name"].as_str(),
                    ),
                    _ => se.split(&ctx, arg(&args, "id")?).map(|_| String::new()),
                })?;
                let mut v = self.sort_view()?;
                v["id"] = json!(id);
                j(v)
            }
            "sort_clear" => {
                let imported = args["imported"] == json!(true);
                self.sort_change(|se| {
                    se.clear(imported);
                    Ok(())
                })?;
                j(self.sort_view()?)
            }
            "sort_files" => {
                let id = arg(&args, "id").map_err(e2s)?.to_string();
                let files = self.with_sort(false, |se| se.files(&id))?;
                let main = tokio::task::spawn_blocking({
                    let files = files.clone();
                    move || thumb::pick_main_in(&files)
                })
                .await
                .map_err(|e| e.to_string())?;
                j(json!({
                    "files": files.iter().map(|(rel, size, _)| json!({ "rel": rel, "size": size, "kind": model::file_kind(rel) })).collect::<Vec<_>>(),
                    "main": main.map(|(file, entry)| json!({ "file": file, "entry": entry })),
                }))
            }
            "sort_preview" => {
                // the main 3D file drawn, else a picture
                let id = arg(&args, "id").map_err(e2s)?.to_string();
                let files = self.with_sort(false, |se| se.files(&id))?;
                let pick = {
                    let files = files.clone();
                    tokio::task::spawn_blocking(move || thumb::pick_main_in(&files))
                        .await
                        .map_err(|e| e.to_string())?
                };
                if let Some((rel, entry)) = pick {
                    if let Some((_, _, at)) = files.iter().find(|f| f.0 == rel) {
                        if let Ok(url) = self.preview_of(at.clone(), entry).await {
                            return j(json!({ "url": url }));
                        }
                    }
                }
                let picture = files
                    .iter()
                    .filter(|(r, _, _)| model::file_kind(r) == "image")
                    .min_by_key(|(r, _, _)| (!r.starts_with("_media/"), r.matches('/').count()));
                j(match picture {
                    Some((r, _, _)) => json!({ "url": format!("~sort/{id}/{r}"), "picture": true }),
                    None => json!({}),
                })
            }
            "file_preview" => {
                let path = self.file_path(&args).await?;
                let entry = args["entry"].as_str().map(String::from);
                j(json!({ "url": self.preview_of(path, entry).await? }))
            }
            "sort_commit" => {
                let only: Option<Vec<String>> = args["ids"].as_array().map(|_| ids.clone());
                let mv = args["mode"].as_str() != Some("copy");
                let force_copy = args["force_copy"].as_bool() == Some(true);
                // dropped on the window and added straight to a category: off the list once in
                let forget = args["forget"] == json!(true);
                let lib = self.library()?;
                lib.writable().map_err(e2s)?;
                let ready = self.with_sort(true, |se| Ok(se.ready(only.as_deref())))?;
                if ready.is_empty() {
                    return Err("Nothing is sorted yet: send models to a category first.".into());
                }
                let items: Vec<Value> = ready.iter().map(|(_, v)| v.clone()).collect();
                let (plans, schemas, used) = self
                    .with_index(None, |ix, lib| {
                        (
                            import::plan(lib, ix, &items),
                            import::schemas_by_id(ix),
                            import::ids_in_use(ix),
                        )
                    })
                    .await?;
                // one that can't go where it was sent says why, and the rest go
                let (mut go, mut bad) = (vec![], vec![]);
                for ((sid, it), p) in ready.into_iter().zip(plans) {
                    match p["error"].as_str() {
                        Some(e) => bad.push((sid, json!({ "error": e }))),
                        None => go.push((sid, it, PathBuf::from(p["dest"].as_str().unwrap_or("")))),
                    }
                }
                self.with_sort(true, |se| {
                    se.mark(&bad, false);
                    Ok(())
                })?;
                if go.is_empty() {
                    return Err(bad[0].1["error"]
                        .as_str()
                        .unwrap_or("Nothing can be imported.")
                        .to_string());
                }
                if self.sort_busy.swap(true, Ordering::Relaxed) {
                    return Err("The workspace is busy: wait until its folders are read or its models imported.".into());
                }
                let guard = NotBusy(self.sort_busy.clone());
                let n = go.len();
                let label = format!(
                    "{} {n} {}",
                    if mv { "Moving" } else { "Copying" },
                    if n == 1 { "model" } else { "models" }
                );
                j(self.spawn_job(&label, move |app, jid, cancel| {
                    let _guard = guard;
                    let lib = app.library().map_err(|e| anyhow!(e))?;
                    let sids: Vec<String> = go.iter().map(|g| g.0.clone()).collect();
                    let items: Vec<Value> = go.iter().map(|g| g.1.clone()).collect();
                    let dests: Vec<PathBuf> = go.into_iter().map(|g| g.2).collect();
                    // subcategories the import makes (undoing it takes them away again)
                    let mut added: Vec<Value> = vec![];
                    for (it, dest) in items.iter().zip(&dests) {
                        let Some(s) = it["schema"].as_str().and_then(|s| schemas.get(s)) else { continue };
                        let path = import::place_of(&lib, Some(s), dest);
                        let tree = schema::subcategories(&schema::as_tree(&lib, s));
                        if let Some(k) = (1..=path.len()).find(|&k| !schema::in_tree(&tree, &path[..k])) {
                            let a = json!({ "schema": s.id, "path": path[..k] });
                            if !added.contains(&a) {
                                added.push(a);
                            }
                        }
                    }
                    let mut results = app.run_import(&jid, &cancel, &lib, &items, &dests, &schemas, &used, mv, force_copy);
                    let mut moves = vec![];
                    for ((r, it), sid) in results.iter_mut().zip(&items).zip(&sids) {
                        let before = r.as_object_mut().and_then(|o| o.shift_remove("before"));
                        if r["error"].is_string() {
                            continue;
                        }
                        let before = before.unwrap_or_default();
                        moves.push(json!({ "name": r["name"], "id": r["id"], "to": r["rel"], "source": it["source"], "files": it["files"],
                            "sort": sid, "sidecar_before": before["sidecar_before"], "thumb_before": before["thumb_before"] }));
                    }
                    let journal = if moves.is_empty() {
                        Value::Null
                    } else {
                        let what = match moves.as_slice() {
                            [one] => one["name"].as_str().unwrap_or("a model").to_string(),
                            all => format!("{} models", all.len()),
                        };
                        let id = relayout::new_id(&lib)?;
                        relayout::record(&lib, &id, &json!({ "kind": "import", "mode": if mv { "move" } else { "copy" },
                            "label": format!("Imported {what}{}", if mv { "" } else { " (copies)" }), "added": added, "moves": moves }))?;
                        json!(id)
                    };
                    let marks: Vec<(String, Value)> = sids.into_iter().zip(results.iter().cloned()).collect();
                    drop(_guard);
                    app.with_sort(true, |se| {
                        se.mark(&marks, mv);
                        if forget {
                            se.forget_done(&marks.iter().map(|m| m.0.clone()).collect::<Vec<_>>());
                        }
                        Ok(())
                    })
                    .map_err(|e| anyhow!(e))?;
                    // what was decided before can't be put back: the files have moved
                    app.sort_undo.lock().unwrap().clear();
                    let failed = results.iter().filter(|r| r["error"].is_string()).count();
                    Ok(json!({ "results": results, "imported": results.len() - failed, "failed": failed + bad.len(), "mode": if mv { "move" } else { "copy" }, "journal": journal }))
                }))
            }
            "sort_paths" => {
                // what was dropped on the window: folders or files, and their names
                let paths = strings(&args["paths"]);
                j(json!(paths
                    .iter()
                    .map(|p| {
                        let pb = PathBuf::from(p);
                        json!({ "path": p, "name": import::file_name(&pb), "dir": pb.is_dir(), "there": pb.exists() })
                    })
                    .collect::<Vec<_>>()))
            }
            other => Err(format!("unknown command {other}")),
        }
    }

    pub async fn call(self: &Arc<Self>, cmd: &str, args: Value) -> Result<Reply, String> {
        let j = |v: Value| Ok(Reply::Json(v));
        self.ensure_watch();
        if cmd.starts_with("sort_") || cmd == "file_preview" {
            return self.sort_call(cmd, args).await;
        }
        if cmd.starts_with("dupes_") {
            return self.dupes_call(cmd, args).await;
        }
        match cmd {
            "app_info" => {
                let lib = self.library();
                let first_run = lib.as_ref().err().is_some_and(|e| e == NO_LIBRARY);
                j(json!({
                    "version": crate::VERSION,
                    "os": std::env::consts::OS,
                    "library": lib.as_ref().ok().map(Library::info),
                    "library_error": lib.err().filter(|_| !first_run),
                    "first_run": first_run,
                    "default_library": crate::config::default_library_path(),
                    "library_url": self.paths.library_url,
                    "recent_libraries": self.config().recent_libraries,
                }))
            }
            "prefs_get" => {
                let mut p = self.prefs().get();
                if let Ok(lib) = self.library() {
                    p[FAVS_KEY] = lib.favourites();
                }
                j(p)
            }
            "prefs_set" => {
                let mut p = args["prefs"].clone();
                if let Some(favs) = p.as_object_mut().and_then(|o| o.shift_remove(FAVS_KEY)) {
                    if let Ok(lib) = self.library() {
                        if lib.read_only().is_none() {
                            lib.set_favourites(&favs).map_err(e2s)?;
                        }
                    }
                }
                self.prefs().set(&p).map_err(e2s)?;
                j(Value::Null)
            }
            "library_open" => {
                let path = PathBuf::from(arg(&args, "path").map_err(e2s)?);
                let lib = self.open_library(&path).map_err(e2s)?;
                j(lib.info())
            }
            "library_rename" => {
                let name = arg(&args, "name").map_err(e2s)?.trim();
                if name.is_empty() {
                    return Err("A library needs a name.".into());
                }
                let lib = self.library()?;
                lib.update_meta(json!({ "name": name })).map_err(e2s)?;
                j(lib.info())
            }
            "library_variants" => {
                let lib = self.library()?;
                let names: Option<Vec<String>> = args["names"].as_array().map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(String::from)
                        .collect()
                });
                lib.set_variant_folders(names.as_deref()).map_err(e2s)?;
                j(lib.info())
            }
            "library_changes" => {
                // `check`: look now (the window came back to the front, or a while passed)
                if args["check"].as_bool() == Some(true) {
                    j(self.check_changes(None).await?)
                } else {
                    j(json!({ "rev": self.changes.load(Ordering::Relaxed) }))
                }
            }
            "library_scan" => {
                let full = args["full"].as_bool() == Some(true);
                if args["job"].as_bool() != Some(true) {
                    return j(self.with_index(Some(full), |ix, _| json!({ "models": ix.models.len(), "read": ix.read, "ms": ix.ms as u64 })).await?);
                }
                // Read again from Home: a job that can be stopped (the library stays as read before)
                let lib = self.library()?;
                let cache = self.index_cache(&lib);
                j(self.spawn_job_with("Reading the library again", false, move |app, jid, cancel| {
                    let report = |i: usize, n: usize| app.job_progress(&jid, json!({ "item": i, "items": n, "name": "" }));
                    match Index::build_with(&lib, cache.as_deref(), full, &cancel, &report) {
                        Some(ix) => {
                            let r = json!({ "models": ix.models.len(), "read": ix.read, "ms": ix.ms as u64 });
                            *app.index.blocking_lock() = Some(ix);
                            Ok(r)
                        }
                        None => Ok(json!({ "stopped": true })),
                    }
                }))
            }
            "library_overview" => j(self
                .with_index(None, |ix, lib| {
                    let mut v = ix.overview();
                    v["loose"] = json!(import::loose_folders(lib, ix));
                    let favs = strings(&lib.favourites());
                    v["starred"] = json!(ix
                        .models
                        .iter()
                        .filter(|m| favs.iter().any(|f| f == m.id()))
                        .count());
                    v
                })
                .await?),
            "import_scan" => {
                let paths: Vec<PathBuf> = args["paths"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(PathBuf::from)
                    .collect();
                if paths.is_empty() {
                    return Err("Nothing to import.".into());
                }
                let contents = args["contents"].as_bool() == Some(true);
                let r = self
                    .with_index(None, |ix, lib| import::scan(lib, ix, &paths, contents))
                    .await?;
                j(r.map_err(e2s)?)
            }
            "import_plan" => {
                let items = args["items"].as_array().cloned().unwrap_or_default();
                j(json!(
                    self.with_index(None, |ix, lib| import::plan(lib, ix, &items))
                        .await?
                ))
            }
            "import_commit" => {
                let items: Vec<Value> = args["items"].as_array().cloned().unwrap_or_default();
                if items.is_empty() {
                    return Err("Nothing to import.".into());
                }
                let mv = args["mode"].as_str() != Some("copy");
                let force_copy = args["force_copy"].as_bool() == Some(true);
                self.library()?.writable().map_err(e2s)?;
                let (plans, schemas, ids) = self
                    .with_index(None, |ix, lib| {
                        (
                            import::plan(lib, ix, &items),
                            import::schemas_by_id(ix),
                            import::ids_in_use(ix),
                        )
                    })
                    .await?;
                if let Some((it, e)) = items
                    .iter()
                    .zip(&plans)
                    .find_map(|(it, p)| p["error"].as_str().map(|e| (it, e)))
                {
                    return Err(format!("{}: {e}", it["name"].as_str().unwrap_or("?")));
                }
                let dests: Vec<PathBuf> = plans
                    .iter()
                    .map(|p| PathBuf::from(p["dest"].as_str().unwrap_or("")))
                    .collect();
                let n = items.len();
                let label = format!(
                    "{} {n} {}",
                    if mv { "Moving" } else { "Copying" },
                    if n == 1 { "model" } else { "models" }
                );
                j(self.spawn_job(&label, move |app, jid, cancel| {
                    let lib = app.library().map_err(|e| anyhow!(e))?;
                    let results = app.run_import(
                        &jid, &cancel, &lib, &items, &dests, &schemas, &ids, mv, force_copy,
                    );
                    Ok(json!({ "results": results, "mode": if mv { "move" } else { "copy" } }))
                }))
            }
            "model_zip" => {
                let path = self.file_path(&args).await?;
                j(json!(archive::list(&path).map_err(e2s)?))
            }
            "model_mesh" => {
                let path = self.file_path(&args).await?;
                let (file, entry) = (
                    arg(&args, "file").map_err(e2s)?.to_string(),
                    args["entry"].as_str().map(String::from),
                );
                let stl = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
                    let bytes = thumb::bytes_at(&path, entry.as_deref())?;
                    Ok(mesh::to_stl(&mesh::read(
                        entry.as_deref().unwrap_or(&file),
                        &bytes,
                    )?))
                })
                .await
                .map_err(|e| e.to_string())?
                .map_err(e2s)?;
                Ok(Reply::Bytes(stl))
            }
            "model_entry" => {
                let path = self.file_path(&args).await?;
                let entry = arg(&args, "entry").map_err(e2s)?;
                Ok(Reply::Bytes(
                    thumb::bytes_at(&path, Some(entry)).map_err(e2s)?,
                ))
            }
            "model_doc" => {
                let path = self.file_path(&args).await?;
                let file = arg(&args, "file").map_err(e2s)?;
                let bytes = thumb::bytes_at(&path, args["entry"].as_str()).map_err(e2s)?;
                j(json!({ "html": crate::docs::to_html(file, &bytes) }))
            }
            "model_cover" => {
                // journalled: undo puts the old cover back (a picture it replaced is kept)
                let (dir, v) = self.model_dir(&args).await?;
                let lib = self.library()?;
                lib.writable().map_err(e2s)?;
                let jid = relayout::new_id(&lib).map_err(e2s)?;
                let mut files = vec![];
                let cover = match args["snapshot"].as_str() {
                    Some(data) => {
                        use base64::Engine;
                        let b64 = data.split_once(',').map(|(_, b)| b).unwrap_or(data);
                        let png = base64::engine::general_purpose::STANDARD
                            .decode(b64.trim())
                            .map_err(|e| format!("not a picture: {e}"))?;
                        if !png.starts_with(b"\x89PNG") {
                            return Err("not a PNG".into());
                        }
                        let file = dir.join("_media/cover.png");
                        let kept = if file.is_file() {
                            let k = relayout::kept_dir(&lib, &jid);
                            std::fs::create_dir_all(&k).map_err(|e| e.to_string())?;
                            std::fs::copy(&file, k.join("0.png")).map_err(|e| e.to_string())?;
                            json!("0.png")
                        } else {
                            Value::Null
                        };
                        files.push(json!({ "rel": lib.relative(&file), "kept": kept }));
                        crate::config::write_atomic(&file, &png).map_err(e2s)?;
                        json!("_media/cover.png")
                    }
                    None => args["file"].clone(),
                };
                let name = v["name"].as_str().unwrap_or("");
                Box::pin(self.call(
                    "model_update",
                    json!({ "id": v["id"], "patch": { "cover": cover }, "journal": jid, "files": files,
                        "label": if args["snapshot"].is_string() { format!("Used the 3D view as {name}'s cover") } else { format!("Changed {name}'s cover") } }),
                ))
                .await
            }
            "thumbs_make" => {
                let force = args["force"].as_bool() == Some(true);
                let only: Option<Vec<String>> = args["ids"].as_array().map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(String::from)
                        .collect()
                });
                self.library()?.writable().map_err(e2s)?;
                let dirs: Vec<(String, String, PathBuf)> = self
                    .with_index(None, |ix, lib| {
                        ix.models
                            .iter()
                            .filter(|m| {
                                only.as_ref()
                                    .is_none_or(|o| o.iter().any(|id| id == m.id()))
                            })
                            .filter(|m| force || m.v["files"]["cover"].is_null())
                            .filter_map(|m| {
                                Some((
                                    m.id().to_string(),
                                    m.v["name"].as_str().unwrap_or("").to_string(),
                                    lib.resolve(m.rel()).ok()?,
                                ))
                            })
                            .collect()
                    })
                    .await?;
                let n = dirs.len();
                j(self.spawn_job("Making previews", move |app, jid, cancel| {
                    let (mut made, mut none, mut failed, mut ids) = (0, 0, vec![], vec![]);
                    for (i, (id, name, dir)) in dirs.iter().enumerate() {
                        if cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        app.job_progress(&jid, json!({ "item": i, "items": n, "name": name }));
                        match thumb::make(dir) {
                            Ok(true) => {
                                made += 1;
                                ids.push(id.clone());
                            }
                            Ok(false) => none += 1,
                            Err(e) => failed.push(json!({ "name": name, "error": e2s(e) })),
                        }
                    }
                    Ok(json!({ "made": made, "no_3d": none, "failed": failed, "refresh": ids }))
                }))
            }
            "job" => {
                let id = arg(&args, "id").map_err(e2s)?;
                let jobs = self.jobs.lock().unwrap();
                j(jobs
                    .iter()
                    .find(|x| x.id == id)
                    .map(Job::to_json)
                    .ok_or("No such job.")?)
            }
            "jobs" => j(json!(self
                .jobs
                .lock()
                .unwrap()
                .iter()
                .map(Job::to_json)
                .collect::<Vec<_>>())),
            "job_cancel" => {
                let id = arg(&args, "id").map_err(e2s)?;
                if let Some(x) = self.jobs.lock().unwrap().iter().find(|x| x.id == id) {
                    x.cancel.store(true, Ordering::Relaxed);
                }
                j(Value::Null)
            }
            "models_move" => {
                let ids: Vec<String> = args["ids"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect();
                let schema_id = args["schema"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(String::from);
                let values: Vec<String> = args["values"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|v| v.as_str().unwrap_or("").to_string())
                    .collect();
                let lib = self.library()?;
                lib.writable().map_err(e2s)?;
                let r = self
                    .with_index(None, |ix, lib| -> Result<Value> {
                        let schema = match &schema_id {
                            Some(id) => Some(ix.schema(id).cloned().ok_or_else(|| anyhow!("There's no category {id} any more."))?),
                            None => None,
                        };
                        let mut favs: Vec<Value> = lib.favourites().as_array().cloned().unwrap_or_default();
                        let mut moved = vec![];
                        let mut errors = vec![];
                        for id in &ids {
                            match import::move_model(lib, ix, id, schema.as_ref(), &values) {
                                Ok(dest) => {
                                    let new_id = model::read_sidecar(&dest)["id"].clone();
                                    for f in favs.iter_mut().filter(|f| f.as_str() == Some(id)) {
                                        *f = new_id.clone();
                                    }
                                    moved.push(json!({ "id": id, "new_id": new_id, "rel": lib.relative(&dest) }));
                                }
                                Err(e) => errors.push(json!({ "id": id, "error": e2s(e) })),
                            }
                        }
                        lib.set_favourites(&json!(favs))?;
                        Ok(json!({ "moved": moved, "errors": errors, "favourites": favs }))
                    })
                    .await?
                    .map_err(e2s)?;
                self.with_index(Some(false), |_, _| ()).await?;
                j(r)
            }
            "models_query" => {
                let favs: Vec<String> =
                    serde_json::from_value(self.library()?.favourites()).unwrap_or_default();
                let (scope, q, sort) = (
                    args["scope"].as_str().unwrap_or("all"),
                    args["q"].as_str().unwrap_or(""),
                    args["sort"].as_str().unwrap_or("name"),
                );
                let (offset, limit) = (
                    args["offset"].as_u64().unwrap_or(0) as usize,
                    args["limit"].as_u64().unwrap_or(200).min(2000) as usize,
                );
                j(self
                    .with_index(None, |ix, _| ix.query(scope, q, sort, offset, limit, &favs))
                    .await?)
            }
            "model_get" => {
                let id = arg(&args, "id").map_err(e2s)?.to_string();
                let found = self
                    .with_index(None, |ix, lib| {
                        ix.get(&id).map(|m| (m.v.clone(), lib.resolve(m.rel())))
                    })
                    .await?;
                let (mut v, dir) = found.ok_or("That model isn't in the library any more.")?;
                let dir = dir.map_err(e2s)?;
                v["files_list"] = json!(model::list_files(&dir).into_iter().map(|(rel, size)| json!({ "rel": rel, "size": size, "kind": model::file_kind(&rel) })).collect::<Vec<_>>());
                v["details"] = model::read_sidecar(&dir);
                // the 3D file shown first (the one its thumbnail is drawn from)
                v["main"] = json!(thumb::pick_main(&dir)
                    .map(|(file, entry)| json!({ "file": file, "entry": entry })));
                v["has_thumb"] = json!(thumb::has(&dir));
                j(v)
            }
            "model_update" => {
                // `journal`: true (or an id made for it) records the edit, so it can be
                // undone (`label` names it; `files`: ones it replaced, kept in the
                // journal's folder)
                let id = arg(&args, "id").map_err(e2s)?.to_string();
                let patch = args["patch"].clone();
                let journal = match &args["journal"] {
                    Value::String(j) => Some(j.clone()),
                    Value::Bool(true) => Some(String::new()),
                    _ => None,
                };
                let cache = self.index_cache(&self.library()?);
                let r = self
                    .with_index(None, |ix, lib| -> Result<Value> {
                        lib.writable()?;
                        let m = ix.get(&id).ok_or_else(|| anyhow!("That model isn't in the library any more."))?;
                        let dir = lib.resolve(m.rel())?;
                        let rel = m.rel().to_string();
                        let before = match &journal {
                            Some(_) => Some(details_before(&dir, m)?),
                            None => None,
                        };
                        // a first model.json starts from what the folder says
                        model::update(&dir, &patch, &first_sidecar(m))?;
                        let mut v = ix.refresh(lib, &id).ok_or_else(|| anyhow!("couldn't read the model again"))?;
                        if let (Some(jid), Some(before)) = (journal, before) {
                            let jid = if jid.is_empty() { relayout::new_id(lib)? } else { jid };
                            let name = v["name"].as_str().unwrap_or("").to_string();
                            let label = args["label"].as_str().map(String::from).unwrap_or_else(|| format!("Edited {name}'s details"));
                            relayout::record(lib, &jid, &json!({ "kind": "details", "label": label,
                                "models": [{ "id": v["id"], "name": name, "rel": rel, "before": before, "after": model::read_sidecar(&dir) }],
                                "files": args["files"].as_array().cloned().unwrap_or_default() }))?;
                            v["journal"] = json!(jid);
                        }
                        if let Some(c) = &cache {
                            ix.save(c)?;
                        }
                        // a favourite keeps its star when the model gets its real id
                        let new_id = v["id"].as_str().unwrap_or("").to_string();
                        if new_id != id {
                            let favs = lib.favourites();
                            if favs.as_array().is_some_and(|f| f.iter().any(|x| x == &json!(id))) {
                                let favs: Vec<Value> = favs.as_array().unwrap().iter().map(|x| if x == &json!(id) { json!(new_id) } else { x.clone() }).collect();
                                lib.set_favourites(&json!(favs))?;
                            }
                        }
                        Ok(v)
                    })
                    .await?;
                j(r.map_err(e2s)?)
            }
            "model_star" => {
                // a star needs a lasting id: the model gets its model.json first
                let id = arg(&args, "id").map_err(e2s)?.to_string();
                let on = args["on"].as_bool() != Some(false);
                let mut id = id;
                if on {
                    if let Reply::Json(v) =
                        Box::pin(self.call("model_update", json!({ "id": id, "patch": {} })))
                            .await?
                    {
                        id = v["id"].as_str().unwrap_or(&id).to_string();
                    }
                }
                let lib = self.library()?;
                let mut favs: Vec<Value> = lib.favourites().as_array().cloned().unwrap_or_default();
                favs.retain(|f| f != &json!(id));
                if on {
                    favs.push(json!(id));
                }
                lib.set_favourites(&json!(favs)).map_err(e2s)?;
                j(json!({ "id": id, "favourites": favs }))
            }
            "relayout_plan" => {
                let change = args["change"].clone();
                let plan = self
                    .with_index(None, |ix, lib| relayout::plan(lib, ix, &change))
                    .await?
                    .map_err(e2s)?;
                j(relayout::summary(&plan))
            }
            "relayout_apply" => {
                let change = args["change"].clone();
                let lib = self.library()?;
                lib.writable().map_err(e2s)?;
                let plan = self
                    .with_index(None, |ix, lib| relayout::plan(lib, ix, &change))
                    .await?
                    .map_err(e2s)?;
                let id = relayout::start(&lib, &plan).map_err(e2s)?;
                j(self.spawn_job(
                    plan["label"].as_str().unwrap_or("Moving folders"),
                    move |app, jid, cancel| {
                        relayout::apply(&lib, &id, &cancel, &|i, n, name| {
                            app.job_progress(&jid, json!({ "item": i, "items": n, "name": name }))
                        })
                    },
                ))
            }
            "journals" => j(json!(relayout::briefs(&self.library()?))),
            "journal_undo" | "journal_finish" => {
                let lib = self.library()?;
                lib.writable().map_err(e2s)?;
                let id = arg(&args, "id").map_err(e2s)?.to_string();
                let undo = cmd == "journal_undo";
                let label = relayout::read(&lib, &id).map_err(e2s)?["label"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                j(self.spawn_job(
                    &format!("{}: {label}", if undo { "Undoing" } else { "Finishing" }),
                    move |app, jid, cancel| {
                        let report = |i: usize, n: usize, name: &str| {
                            app.job_progress(&jid, json!({ "item": i, "items": n, "name": name }))
                        };
                        if !undo {
                            return relayout::apply(&lib, &id, &cancel, &report);
                        }
                        let r = relayout::undo(&lib, &id, &cancel, &report)?;
                        // imported models put back where they came from: not imported in the workspace
                        let back = strings(&r["sort"]);
                        if !back.is_empty() {
                            let n = app.with_sort(true, |se| Ok(se.unmark(&back))).unwrap_or(0);
                            app.sort_undo.lock().unwrap().clear();
                            let mut r = r;
                            r["unmarked"] = json!(n);
                            return Ok(r);
                        }
                        Ok(r)
                    },
                ))
            }
            "models_update" => {
                // several models at once: tags added or removed, authors, licence, fields
                let ids: Vec<String> = args["ids"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect();
                let edit = args["patch"].clone();
                let cache = self.index_cache(&self.library()?);
                let r = self
                    .with_index(None, |ix, lib| -> Result<Value> {
                        lib.writable()?;
                        let list = |k: &str| -> Vec<String> {
                            match &edit[k] {
                                Value::String(s) => s.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect(),
                                Value::Array(a) => a.iter().filter_map(Value::as_str).map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect(),
                                _ => vec![],
                            }
                        };
                        let (add, remove) = (list("tags_add"), list("tags_remove"));
                        let (mut saved, mut errors, mut changed) = (0, vec![], vec![]);
                        // a model given its first model.json gets a lasting id
                        let mut renamed = serde_json::Map::new();
                        for id in &ids {
                            let Some(m) = ix.get(id) else {
                                errors.push(json!({ "id": id, "error": "That model isn't in the library any more." }));
                                continue;
                            };
                            let dir = lib.resolve(m.rel())?;
                            let mut patch = json!({});
                            if !add.is_empty() || !remove.is_empty() {
                                let mut tags: Vec<String> = m.v["tags"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
                                tags.retain(|t| !remove.iter().any(|r| r.eq_ignore_ascii_case(t)));
                                for a in &add {
                                    if !tags.iter().any(|t| t.eq_ignore_ascii_case(a)) {
                                        tags.push(a.clone());
                                    }
                                }
                                patch["tags"] = json!(tags);
                            }
                            for k in ["authors", "license"] {
                                if edit[k].as_str().is_some_and(|v| !v.trim().is_empty()) {
                                    patch[k] = edit[k].clone();
                                }
                            }
                            if let Some(f) = edit["fields"].as_object().filter(|f| !f.is_empty()) {
                                patch["fields"] = Value::Object(f.clone());
                            }
                            let (rel, name) = (m.rel().to_string(), m.v["name"].clone());
                            let r = details_before(&dir, m).and_then(|before| {
                                model::update(&dir, &patch, &first_sidecar(m))?;
                                Ok(before)
                            });
                            match r {
                                Ok(before) => {
                                    saved += 1;
                                    let after = model::read_sidecar(&dir);
                                    ix.refresh(lib, id);
                                    renamed.insert(id.clone(), after["id"].clone());
                                    changed.push(json!({ "id": after["id"], "name": name, "rel": rel, "before": before, "after": after }));
                                }
                                Err(e) => errors.push(json!({ "id": id, "error": e2s(e) })),
                            }
                        }
                        if let Some(c) = &cache {
                            ix.save(c)?;
                        }
                        let journal = if changed.is_empty() {
                            Value::Null
                        } else {
                            let jid = relayout::new_id(lib)?;
                            let label = match changed.as_slice() {
                                [one] => format!("Edited {}'s details", one["name"].as_str().unwrap_or("")),
                                all => format!("Edited {} models' details", all.len()),
                            };
                            relayout::record(lib, &jid, &json!({ "kind": "details", "label": label, "models": changed }))?;
                            json!(jid)
                        };
                        Ok(json!({ "saved": saved, "errors": errors, "journal": journal, "ids": renamed }))
                    })
                    .await?
                    .map_err(e2s)?;
                j(r)
            }
            "subcategory_add" | "subcategory_remove" => {
                let lib = self.library()?;
                let id = arg(&args, "schema").map_err(e2s)?.to_string();
                let path: Vec<String> = args["path"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect();
                if cmd == "subcategory_add" {
                    let made = schema::add_subcategory(
                        &lib,
                        &id,
                        &path,
                        args["name"].as_str().unwrap_or(""),
                    )
                    .map_err(e2s)?;
                    self.with_index(Some(false), |_, _| ()).await?;
                    return j(json!({ "path": made }));
                }
                if path.is_empty() {
                    return Err("Choose a subcategory to remove.".into());
                }
                let n = self
                    .with_index(None, |ix, _| {
                        ix.models
                            .iter()
                            .filter(|m| m.v["schema"] == json!(id))
                            .filter(|m| {
                                let p: Vec<String> =
                                    serde_json::from_value(m.v["path"].clone()).unwrap_or_default();
                                p.len() >= path.len()
                                    && p.iter().zip(&path).all(|(a, b)| a.eq_ignore_ascii_case(b))
                            })
                            .count()
                    })
                    .await?;
                if n > 0 {
                    return Err(format!(
                        "{} has {n} {} in it: move or merge them first.",
                        path.join(" › "),
                        if n == 1 { "model" } else { "models" }
                    ));
                }
                schema::remove_subcategory(&lib, &id, &path).map_err(e2s)?;
                self.with_index(Some(false), |_, _| ()).await?;
                j(json!({ "removed": path }))
            }
            "schema_create" => {
                let lib = self.library()?;
                let s = schema::create(&lib, &args["schema"]).map_err(e2s)?;
                self.with_index(Some(false), |_, _| ()).await?;
                j(s.to_json())
            }
            other => Err(format!("unknown command {other}")),
        }
    }

    /// Part of a file in the open library for an HTTP `Range` header ("bytes=a-b"):
    /// (status, bytes, Content-Range). Without one, the whole file (200).
    pub fn library_range(
        &self,
        rel: &str,
        range: Option<&str>,
    ) -> Result<(u16, Vec<u8>, Option<String>)> {
        use std::io::{Read, Seek, SeekFrom};
        let path = self.served_path(rel)?;
        let Some(r) = range.and_then(|r| r.trim().strip_prefix("bytes=")) else {
            return Ok((200, std::fs::read(&path)?, None));
        };
        let total = std::fs::metadata(&path)?.len();
        let (a, b) = r.split_once('-').unwrap_or((r, ""));
        let (start, end) = match (a.trim().parse::<u64>().ok(), b.trim().parse::<u64>().ok()) {
            (Some(s), Some(e)) => (s, e.min(total.saturating_sub(1))),
            (Some(s), None) => (s, (s + (8 << 20)).min(total).saturating_sub(1)), // 8 MB at a time
            (None, Some(n)) => (total.saturating_sub(n), total.saturating_sub(1)),
            _ => return Ok((200, std::fs::read(&path)?, None)),
        };
        if start >= total || end < start {
            return Ok((416, vec![], Some(format!("bytes */{total}"))));
        }
        let mut f = std::fs::File::open(&path)?;
        f.seek(SeekFrom::Start(start))?;
        let mut buf = vec![0; (end - start + 1) as usize];
        f.read_exact(&mut buf)?;
        Ok((206, buf, Some(format!("bytes {start}-{end}/{total}"))))
    }

    /// A file in the open library, by its library-relative path (the `library://` protocol).
    pub fn library_file(&self, rel: &str) -> Result<Vec<u8>> {
        Ok(std::fs::read(self.served_path(rel)?)?)
    }

    /// What a `library://` path is: a file in the library; `~preview/<name>`, a
    /// preview drawn in the data folder; `~sort/<item>/<file>`, a file of a model
    /// in the sorting workspace.
    fn served_path(&self, rel: &str) -> Result<PathBuf> {
        let rel = percent_decode(rel.trim_start_matches('/'));
        if let Some(name) = rel.strip_prefix("~preview/") {
            let (stem, ext) = name.split_once('.').unwrap_or((name, ""));
            if ext != "png" || stem.is_empty() || !stem.chars().all(|c| c.is_ascii_hexdigit()) {
                anyhow::bail!("no such preview");
            }
            return Ok(self.paths.data_dir.join("previews").join(name));
        }
        if let Some(r) = rel.strip_prefix("~sort/") {
            let (id, file) = r.split_once('/').ok_or_else(|| anyhow!("no such file"))?;
            return self
                .with_sort(false, |se| se.file_at(id, file))
                .map_err(|e| anyhow!(e));
        }
        let lib = self.library().map_err(|e| anyhow!(e))?;
        lib.resolve(&rel)
    }
}

/// "%20" -> " " (paths in library URLs and file names sent in headers).
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str) -> (Arc<App>, PathBuf) {
        let home = std::env::temp_dir().join(format!("modlib-api-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let paths = AppPaths {
            config_dir: home.join("config"),
            data_dir: home.join("data"),
            library_url: "/library/".into(),
        };
        (App::new(paths).unwrap(), home)
    }

    async fn call(app: &Arc<App>, cmd: &str, args: Value) -> Value {
        match app.call(cmd, args).await.unwrap() {
            Reply::Json(v) => v,
            Reply::Bytes(_) => panic!("bytes"),
        }
    }

    #[tokio::test]
    async fn opens_renames_and_remembers_libraries() {
        let (app, home) = app("open");
        let a = home.join("Models A");
        let info = call(
            &app,
            "library_open",
            json!({ "path": a.display().to_string() }),
        )
        .await;
        assert_eq!(info["name"], "Models A");
        let info = call(&app, "library_rename", json!({ "name": "  Minis " })).await;
        assert_eq!(info["name"], "Minis");
        assert_eq!(info["variant_folders"][4], "Sized");
        let info = call(
            &app,
            "library_variants",
            json!({ "names": ["Resin", " FDM ", "resin", ""] }),
        )
        .await;
        assert_eq!(info["variant_folders"], json!(["Resin", "FDM"]));
        let info = call(&app, "library_variants", json!({ "names": null })).await;
        assert_eq!(info["variant_folders"].as_array().unwrap().len(), 8);
        assert!(app
            .call("library_rename", json!({ "name": " " }))
            .await
            .is_err());
        call(
            &app,
            "library_open",
            json!({ "path": home.join("Models B").display().to_string() }),
        )
        .await;
        let info = call(&app, "app_info", json!({})).await;
        assert_eq!(info["library"]["name"], "Models B");
        assert_eq!(info["recent_libraries"].as_array().unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn favourites_live_in_the_library() {
        let (app, home) = app("favs");
        let lib = home.join("Lib");
        call(
            &app,
            "library_open",
            json!({ "path": lib.display().to_string() }),
        )
        .await;
        call(
            &app,
            "prefs_set",
            json!({ "prefs": { "ml-ui": { "theme": "dark" }, "ml-favs": ["m1"] } }),
        )
        .await;
        let p = call(&app, "prefs_get", json!({})).await;
        assert_eq!(p["ml-ui"]["theme"], "dark");
        assert_eq!(p["ml-favs"], json!(["m1"]));
        assert!(Prefs::new(home.join("config/prefs.json"))
            .get()
            .get("ml-favs")
            .is_none());
        assert_eq!(Library::open(&lib).unwrap().favourites(), json!(["m1"]));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn first_start_asks_where_the_library_goes() {
        let (app, home) = app("first");
        let info = call(&app, "app_info", json!({})).await;
        assert_eq!(info["first_run"], true);
        assert!(info["library"].is_null() && info["library_error"].is_null());
        assert!(app.call("library_overview", json!({})).await.is_err());
        call(
            &app,
            "library_open",
            json!({ "path": home.join("Lib").display().to_string() }),
        )
        .await;
        assert_eq!(call(&app, "app_info", json!({})).await["first_run"], false);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn schemas_models_details_and_stars() {
        let (app, home) = app("models");
        let lib = home.join("Lib");
        call(
            &app,
            "library_open",
            json!({ "path": lib.display().to_string() }),
        )
        .await;
        let s = call(&app, "schema_create", json!({ "schema": { "name": "Wargames", "levels": [{ "label": "Game" }, { "label": "Faction" }] } })).await;
        assert_eq!(s["id"], "wargames");
        let d = lib.join("Wargames/Warhammer 40k/Tyranid/Hive Tyrant (Jo Smith)");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("body.stl"), "x").unwrap();
        call(&app, "library_scan", json!({})).await;
        let ov = call(&app, "library_overview", json!({})).await;
        assert_eq!(ov["schemas"][0]["count"], 1);
        let q = call(
            &app,
            "models_query",
            json!({ "scope": "schema:wargames/Warhammer 40k", "q": "hive" }),
        )
        .await;
        assert_eq!(q["total"], 1);
        let id = q["items"][0]["id"].as_str().unwrap().to_string();
        let m = call(&app, "model_get", json!({ "id": id })).await;
        assert_eq!(m["files_list"][0]["rel"], "body.stl");
        // starring gives the model its model.json, and the star its lasting id
        let star = call(&app, "model_star", json!({ "id": id, "on": true })).await;
        let real = star["id"].as_str().unwrap().to_string();
        assert_ne!(real, id);
        assert_eq!(star["favourites"], json!([real]));
        let side: Value =
            serde_json::from_slice(&std::fs::read(d.join("model.json")).unwrap()).unwrap();
        assert_eq!(side["name"], "Hive Tyrant");
        assert_eq!(side["authors"], json!([{ "name": "Jo Smith" }]));
        assert_eq!(side["path"], json!(["Warhammer 40k", "Tyranid"]));
        // opening the library again turns the fixed levels into a tree; nothing moves
        call(
            &app,
            "library_open",
            json!({ "path": lib.display().to_string() }),
        )
        .await;
        let sc: Value = serde_json::from_slice(
            &std::fs::read(lib.join("_library/schemas/wargames.json")).unwrap(),
        )
        .unwrap();
        assert!(sc.get("levels").is_none(), "{sc}");
        assert_eq!(
            sc["subcategories"],
            json!([{ "name": "Warhammer 40k", "subcategories": [{ "name": "Tyranid" }] }])
        );
        call(&app, "library_scan", json!({ "full": true })).await;
        let q = call(
            &app,
            "models_query",
            json!({ "scope": "all", "q": "in:tyranid" }),
        )
        .await;
        assert_eq!(
            q["items"][0]["rel"],
            "Wargames/Warhammer 40k/Tyranid/Hive Tyrant (Jo Smith)"
        );
        let v = call(
            &app,
            "model_update",
            json!({ "id": real, "patch": { "tags": "monster", "fields": { "scale": "32mm" } } }),
        )
        .await;
        assert_eq!(v["tags"], json!(["monster"]));
        let q = call(
            &app,
            "models_query",
            json!({ "scope": "favs", "q": "tag:monster scale:32" }),
        )
        .await;
        assert_eq!(q["total"], 1);
        // reopening reads the cache: nothing changed, nothing reread
        let again = App::new(app.paths.clone()).unwrap();
        let r = call(&again, "library_scan", json!({})).await;
        assert_eq!(
            (r["models"].as_u64(), r["read"].as_u64()),
            (Some(1), Some(0))
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn library_files_stay_inside() {
        let (app, home) = app("files");
        let lib = home.join("Lib");
        app.open_library(&lib).unwrap();
        std::fs::create_dir_all(lib.join("Unsorted/A b")).unwrap();
        std::fs::write(lib.join("Unsorted/A b/x.txt"), "hi").unwrap();
        assert_eq!(app.library_file("/Unsorted/A%20b/x.txt").unwrap(), b"hi");
        assert!(app.library_file("../config/config.json").is_err());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn imports_as_a_job_and_moves_between_categories() {
        let (app, home) = app("import");
        let lib = home.join("Lib");
        call(
            &app,
            "library_open",
            json!({ "path": lib.display().to_string() }),
        )
        .await;
        call(&app, "schema_create", json!({ "schema": { "name": "Wargames", "levels": [{ "label": "Game" }, { "label": "Faction" }] } })).await;
        let src = home.join("Downloads");
        std::fs::create_dir_all(src.join("Tyrant (Jo)")).unwrap();
        std::fs::write(src.join("Tyrant (Jo)/t.stl"), "solid").unwrap();
        std::fs::write(src.join("benchy.stl"), "b").unwrap();
        let scan = call(
            &app,
            "import_scan",
            json!({ "paths": [src.display().to_string()], "contents": true }),
        )
        .await;
        let mut items: Vec<Value> = scan["items"].as_array().unwrap().clone();
        assert_eq!(items.len(), 2);
        items[0]["schema"] = json!("wargames");
        items[0]["values"] = json!(["40k", "Tyranid"]);
        let plan = call(&app, "import_plan", json!({ "items": items })).await;
        assert_eq!(plan[0]["rel"], "Wargames/40k/Tyranid/Tyrant (Jo)");
        assert_eq!(plan[1]["rel"], "Unsorted/benchy");
        let job = call(
            &app,
            "import_commit",
            json!({ "items": items, "mode": "move", "force_copy": true }),
        )
        .await;
        let id = job["job"].as_str().unwrap().to_string();
        let mut done = Value::Null;
        for _ in 0..200 {
            done = call(&app, "job", json!({ "id": id })).await;
            if done["done"] == true {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert_eq!(done["error"], Value::Null, "{done}");
        assert_eq!(done["result"]["results"].as_array().unwrap().len(), 2);
        assert!(
            lib.join("Wargames/40k/Tyranid/Tyrant (Jo)/t.stl").is_file()
                && !src.join("benchy.stl").exists()
        );
        let ov = call(&app, "library_overview", json!({})).await;
        assert_eq!(
            (ov["all"].clone(), ov["unsorted"].clone()),
            (json!(2), json!(1))
        );
        // star the Unsorted one, then sort it into a category: the star follows
        let q = call(&app, "models_query", json!({ "scope": "unsorted" })).await;
        let bid = q["items"][0]["id"].as_str().unwrap().to_string();
        let star = call(&app, "model_star", json!({ "id": bid, "on": true })).await;
        let bid = star["id"].as_str().unwrap().to_string();
        let r = call(
            &app,
            "models_move",
            json!({ "ids": [bid], "schema": "wargames", "values": ["40k", "Tyranid"] }),
        )
        .await;
        assert_eq!(r["moved"][0]["rel"], "Wargames/40k/Tyranid/benchy");
        assert_eq!(r["favourites"], json!([bid]));
        let q = call(
            &app,
            "models_query",
            json!({ "scope": "schema:wargames/40k/Tyranid" }),
        )
        .await;
        assert_eq!(q["total"], 2);
        let _ = std::fs::remove_dir_all(&home);
    }

    async fn bytes(app: &Arc<App>, cmd: &str, args: Value) -> Vec<u8> {
        match app.call(cmd, args).await.unwrap() {
            Reply::Bytes(b) => b,
            Reply::Json(v) => panic!("json {v}"),
        }
    }

    async fn wait(app: &Arc<App>, job: &Value) -> Value {
        let id = job["job"].as_str().unwrap().to_string();
        for _ in 0..500 {
            let done = call(app, "job", json!({ "id": id })).await;
            if done["done"] == true {
                return done;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("job didn't finish")
    }

    #[tokio::test]
    async fn shows_meshes_zips_readmes_and_previews() {
        use std::io::Write;
        let (app, home) = app("view");
        let lib = home.join("Lib");
        call(
            &app,
            "library_open",
            json!({ "path": lib.display().to_string() }),
        )
        .await;
        let dir = lib.join("Unsorted/Armour (Jo)");
        std::fs::create_dir_all(dir.join("Presupported/Helmet")).unwrap();
        std::fs::create_dir_all(dir.join("Unsupported")).unwrap();
        std::fs::write(
            dir.join("Presupported/Helmet/helmet.stl"),
            mesh::tests::cube_stl_text(),
        )
        .unwrap();
        std::fs::write(
            dir.join("Unsupported/body.stl"),
            mesh::tests::cube_stl_text().repeat(3),
        )
        .unwrap();
        std::fs::write(
            dir.join("README.md"),
            "# Armour\n\n<script>alert(1)</script>\n\n[site](https://example.com) [bad](javascript:x)",
        )
        .unwrap();
        let mut z = zip::ZipWriter::new(std::fs::File::create(dir.join("extra.zip")).unwrap());
        z.start_file::<_, ()>("parts/arm.stl", Default::default())
            .unwrap();
        z.write_all(mesh::tests::cube_stl_text().as_bytes())
            .unwrap();
        z.start_file::<_, ()>("__MACOSX/._arm.stl", Default::default())
            .unwrap();
        z.finish().unwrap();
        call(&app, "library_scan", json!({ "full": true })).await;
        let q = call(&app, "models_query", json!({ "scope": "all" })).await;
        let id = q["items"][0]["id"].as_str().unwrap().to_string();
        // the main file prefers the supported variant
        let m = call(&app, "model_get", json!({ "id": id })).await;
        assert_eq!(m["main"]["file"], "Presupported/Helmet/helmet.stl", "{m}");
        assert_eq!(m["has_thumb"], false);
        let stl = bytes(
            &app,
            "model_mesh",
            json!({ "id": id, "file": "Unsupported/body.stl" }),
        )
        .await;
        assert_eq!(u32::from_le_bytes(stl[80..84].try_into().unwrap()), 9);
        let entries = call(&app, "model_zip", json!({ "id": id, "file": "extra.zip" })).await;
        assert_eq!(entries.as_array().unwrap().len(), 1, "{entries}");
        assert_eq!(entries[0]["name"], "parts/arm.stl");
        let stl = bytes(
            &app,
            "model_mesh",
            json!({ "id": id, "file": "extra.zip", "entry": "parts/arm.stl" }),
        )
        .await;
        assert_eq!(stl.len(), 84 + 50 * 3);
        assert!(app
            .call("model_mesh", json!({ "id": id, "file": "../../x.stl" }))
            .await
            .is_err());
        let doc = call(&app, "model_doc", json!({ "id": id, "file": "README.md" })).await;
        let html = doc["html"].as_str().unwrap();
        assert!(
            html.contains("<h1>Armour</h1>")
                && !html.contains("<script")
                && html.contains("https://example.com")
                && !html.contains("href=\"javascript"),
            "{html}"
        );
        // previews: made for models without a cover, then used as the cover
        let done = wait(&app, &call(&app, "thumbs_make", json!({})).await).await;
        assert_eq!(done["result"]["made"], 1, "{done}");
        assert!(dir.join(thumb::THUMB).is_file());
        let q = call(&app, "models_query", json!({ "scope": "all" })).await;
        assert_eq!(q["items"][0]["files"]["cover"], thumb::THUMB);
        let done = wait(&app, &call(&app, "thumbs_make", json!({})).await).await;
        assert_eq!(done["result"]["made"], 0);
        // ranges, for videos
        let rel = "Unsorted/Armour%20(Jo)/README.md";
        let all = app.library_range(rel, None).unwrap();
        assert_eq!(all.0, 200);
        let (code, part, cr) = app.library_range(rel, Some("bytes=2-7")).unwrap();
        assert_eq!((code, part.as_slice()), (206, &all.1[2..8]));
        assert_eq!(cr.unwrap(), format!("bytes 2-7/{}", all.1.len()));
        assert_eq!(app.library_range(rel, Some("bytes=99999-")).unwrap().0, 416);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn renames_a_category_undoes_it_and_edits_several_models() {
        let (app, home) = app("relayout");
        let lib = home.join("Lib");
        call(
            &app,
            "library_open",
            json!({ "path": lib.display().to_string() }),
        )
        .await;
        call(&app, "schema_create", json!({ "schema": { "name": "Wargames", "levels": [{ "label": "Game" }, { "label": "Faction" }] } })).await;
        for m in ["Tyrant", "Lictor"] {
            std::fs::create_dir_all(lib.join(format!("Wargames/40k/Tyranid/{m}"))).unwrap();
            std::fs::write(lib.join(format!("Wargames/40k/Tyranid/{m}/x.stl")), "solid").unwrap();
        }
        call(&app, "library_scan", json!({ "full": true })).await;
        let change = json!({ "kind": "category", "schema": "wargames", "from": ["40k", "Tyranid"], "to": ["40k", "Tyranids"] });
        let s = call(&app, "relayout_plan", json!({ "change": change })).await;
        assert_eq!(
            (s["moving"].clone(), s["label"].clone()),
            (json!(2), json!("Renamed Tyranid to Tyranids"))
        );
        let done = wait(
            &app,
            &call(&app, "relayout_apply", json!({ "change": change })).await,
        )
        .await;
        assert_eq!(done["result"]["state"], "done", "{done}");
        let q = call(
            &app,
            "models_query",
            json!({ "scope": "schema:wargames/40k/Tyranids" }),
        )
        .await;
        assert_eq!(q["total"], 2);
        let js = call(&app, "journals", json!({})).await;
        assert_eq!(js[0]["state"], "done");
        let done = wait(
            &app,
            &call(&app, "journal_undo", json!({ "id": js[0]["id"] })).await,
        )
        .await;
        assert_eq!(done["result"]["state"], "undone", "{done}");
        assert!(
            lib.join("Wargames/40k/Tyranid/Tyrant/x.stl").is_file()
                && !lib.join("Wargames/40k/Tyranids").exists()
        );
        // several models at once
        let q = call(&app, "models_query", json!({ "scope": "all" })).await;
        let ids: Vec<Value> = q["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].clone())
            .collect();
        let r = call(
            &app,
            "models_update",
            json!({ "ids": ids, "patch": { "tags_add": "big, monster", "authors": "Jo Smith" } }),
        )
        .await;
        assert_eq!(r["saved"], 2, "{r}");
        let q = call(
            &app,
            "models_query",
            json!({ "scope": "all", "q": "tag:monster author:jo" }),
        )
        .await;
        assert_eq!(q["total"], 2);
        let ids: Vec<Value> = q["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].clone())
            .collect();
        call(
            &app,
            "models_update",
            json!({ "ids": ids, "patch": { "tags_remove": ["big"] } }),
        )
        .await;
        let q = call(
            &app,
            "models_query",
            json!({ "scope": "all", "q": "tag:big" }),
        )
        .await;
        assert_eq!(q["total"], 0);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn subcategories_are_made_kept_renamed_and_removed() {
        let (app, home) = app("subcats");
        let lib = home.join("Lib");
        call(
            &app,
            "library_open",
            json!({ "path": lib.display().to_string() }),
        )
        .await;
        call(
            &app,
            "schema_create",
            json!({ "schema": { "name": "Prints", "subcategories": [{ "name": "1" }] } }),
        )
        .await;
        for (path, name) in [
            (json!([]), "2"),
            (json!(["2"]), "A"),
            (json!(["2"]), "B"),
            (json!(["2"]), "C"),
            (json!(["2", "A"]), "1"),
            (json!(["2", "A"]), "2"),
        ] {
            call(
                &app,
                "subcategory_add",
                json!({ "schema": "prints", "path": path, "name": name }),
            )
            .await;
        }
        // any depth, on any branch
        call(
            &app,
            "subcategory_add",
            json!({ "schema": "prints", "path": ["2", "A", "1"], "name": "x" }),
        )
        .await;
        assert!(app
            .call(
                "subcategory_add",
                json!({ "schema": "prints", "path": ["2"], "name": "_x" })
            )
            .await
            .is_err());
        std::fs::create_dir_all(lib.join("Prints/2/Bench")).unwrap();
        std::fs::write(lib.join("Prints/2/Bench/b.stl"), "solid").unwrap();
        assert!(app
            .call(
                "subcategory_add",
                json!({ "schema": "prints", "path": ["2", "Bench"], "name": "x" })
            )
            .await
            .is_err());
        std::fs::remove_dir_all(lib.join("Prints/2/Bench")).unwrap();
        assert!(lib.join("Prints/2/A/2").is_dir() && lib.join("Prints/1").is_dir());
        assert!(lib.join("Prints/2/A/1/x").is_dir());
        let ov = call(&app, "library_overview", json!({})).await;
        let tree = &ov["schemas"][0]["tree"];
        assert_eq!(tree[1]["value"], "2");
        assert_eq!(tree[1]["children"].as_array().unwrap().len(), 3);
        assert_eq!(tree[1]["children"][0]["children"][1]["value"], "2");
        // a model moved out of a subcategory leaves its folder in place
        std::fs::create_dir_all(lib.join("Prints/2/A/1/Thing")).unwrap();
        std::fs::write(lib.join("Prints/2/A/1/Thing/t.stl"), "solid").unwrap();
        call(&app, "library_scan", json!({ "full": true })).await;
        let q = call(&app, "models_query", json!({ "scope": "all" })).await;
        let id = q["items"][0]["id"].clone();
        call(
            &app,
            "models_move",
            json!({ "ids": [id], "schema": "prints", "values": ["2", "B", "x"] }),
        )
        .await;
        assert!(
            lib.join("Prints/2/A/1").is_dir() && lib.join("Prints/2/B/x/Thing/t.stl").is_file()
        );
        // a non-empty one can't be removed; renaming an empty one renames its folder
        assert!(app
            .call(
                "subcategory_remove",
                json!({ "schema": "prints", "path": ["2", "B"] })
            )
            .await
            .is_err());
        let change =
            json!({ "kind": "category", "schema": "prints", "from": ["2", "A"], "to": ["2", "D"] });
        let done = wait(
            &app,
            &call(&app, "relayout_apply", json!({ "change": change })).await,
        )
        .await;
        assert_eq!(done["result"]["state"], "done", "{done}");
        assert!(lib.join("Prints/2/D/1").is_dir() && !lib.join("Prints/2/A").exists());
        call(
            &app,
            "subcategory_remove",
            json!({ "schema": "prints", "path": ["2", "D"] }),
        )
        .await;
        assert!(!lib.join("Prints/2/D").exists());
        let ov = call(&app, "library_overview", json!({})).await;
        let kids: Vec<Value> = ov["schemas"][0]["tree"][1]["children"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["value"].clone())
            .collect();
        assert_eq!(kids, vec![json!("B"), json!("C")]);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn sorts_a_folder_into_the_library_over_two_sittings() {
        let (app, home) = app("sort");
        let lib = home.join("Lib");
        call(
            &app,
            "library_open",
            json!({ "path": lib.display().to_string() }),
        )
        .await;
        call(
            &app,
            "schema_create",
            json!({ "schema": { "name": "Home items" } }),
        )
        .await;
        let stl = "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid t\n";
        let src = home.join("Share");
        for (p, body) in [
            ("Kitchen/Spoon rest/spoon.stl", stl),
            ("Kitchen/Spoon rest/photo.jpg", "jpeg"),
            ("Office/Lamp/lamp.stl", stl),
        ] {
            std::fs::create_dir_all(src.join(p).parent().unwrap()).unwrap();
            std::fs::write(src.join(p), body).unwrap();
        }
        let job = call(
            &app,
            "sort_add",
            json!({ "paths": [src.display().to_string()] }),
        )
        .await;
        assert_eq!(wait(&app, &job).await["result"]["items"], 2);
        let se = call(&app, "sort_get", json!({})).await;
        let spoon = se["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["name"] == "Spoon rest")
            .unwrap()
            .clone();
        let sid = spoon["id"].as_str().unwrap().to_string();
        // its main 3D file drawn, its files, its picture and its mesh
        let pv = call(&app, "sort_preview", json!({ "id": sid })).await;
        let url = pv["url"].as_str().unwrap();
        assert!(
            url.starts_with("~preview/") && app.library_file(url).unwrap().starts_with(b"\x89PNG")
        );
        let files = call(&app, "sort_files", json!({ "id": sid })).await;
        assert_eq!(files["files"].as_array().unwrap().len(), 2);
        assert_eq!(files["main"]["file"], "spoon.stl");
        assert_eq!(
            app.library_file(&format!("~sort/{sid}/photo.jpg")).unwrap(),
            b"jpeg"
        );
        assert!(app
            .library_file(&format!("~sort/{sid}/../Lamp/lamp.stl"))
            .is_err());
        assert!(app
            .library_file("~preview/../../config/config.json")
            .is_err());
        assert!(!bytes(
            &app,
            "model_mesh",
            json!({ "sort": sid, "file": "spoon.stl" })
        )
        .await
        .is_empty());
        // the Kitchen folder goes to Home items, keeping its name as a subcategory
        let kitchen = src.join("Kitchen").display().to_string();
        let se = call(
            &app,
            "sort_send",
            json!({ "folders": [kitchen], "schema": "home-items", "values": [], "keep": true }),
        )
        .await;
        assert_eq!(se["changed"], 1);
        // a change is undone (only while it's the last one): the workspace is as before
        let office = src.join("Office").display().to_string();
        let se = call(
            &app,
            "sort_send",
            json!({ "folders": [office], "schema": "home-items", "values": ["Desk"] }),
        )
        .await;
        let rev = se["undo"].as_u64().unwrap();
        assert!(app
            .call("sort_undo", json!({ "rev": rev + 1 }))
            .await
            .is_err());
        let se = call(&app, "sort_undo", json!({ "rev": rev })).await;
        let placed = |se: &Value, name: &str| {
            se["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| i["name"] == name && i["placed"] == true)
        };
        assert!(!placed(&se, "Lamp") && placed(&se, "Spoon rest"));
        let job = call(&app, "sort_commit", json!({ "mode": "move" })).await;
        let done = wait(&app, &job).await;
        assert_eq!(done["result"]["imported"], 1, "{done}");
        // once imported, what was decided before can't be put back
        assert!(app.call("sort_undo", json!({})).await.is_err());
        assert!(lib
            .join("Home items/Kitchen/Spoon rest/spoon.stl")
            .is_file());
        assert!(!src.join("Kitchen").exists(), "the emptied folder goes");
        let q = call(&app, "models_query", json!({ "q": "spoon" })).await;
        assert_eq!(q["total"], 1);
        // another sitting: the workspace is as it was left
        let app2 = App::new(app.paths.clone()).unwrap();
        let se = call(&app2, "sort_get", json!({})).await;
        let items = se["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert!(items
            .iter()
            .any(|i| i["name"] == "Spoon rest"
                && i["done"]["rel"] == "Home items/Kitchen/Spoon rest"));
        assert!(items
            .iter()
            .any(|i| i["name"] == "Lamp" && i["placed"] == false));
        let se = call(&app2, "sort_clear", json!({ "imported": true })).await;
        assert_eq!(se["items"].as_array().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn moves_imports_and_details_are_journalled_and_undone() {
        let (app, home) = app("journals");
        let lib = home.join("Lib");
        call(
            &app,
            "library_open",
            json!({ "path": lib.display().to_string() }),
        )
        .await;
        call(
            &app,
            "schema_create",
            json!({ "schema": { "name": "Home items" } }),
        )
        .await;
        for p in ["Unsorted/Lamp/lamp.stl", "Unsorted/Hook/hook.stl"] {
            std::fs::create_dir_all(lib.join(p).parent().unwrap()).unwrap();
            std::fs::write(lib.join(p), "solid x").unwrap();
        }
        call(&app, "library_scan", json!({ "full": true })).await;
        let id_of = |q: Value, name: &str| {
            q["items"]
                .as_array()
                .unwrap()
                .iter()
                .find(|m| m["name"] == name)
                .unwrap()["id"]
                .as_str()
                .unwrap()
                .to_string()
        };
        let q = call(&app, "models_query", json!({})).await;
        let (lamp, hook) = (id_of(q.clone(), "Lamp"), id_of(q, "Hook"));
        let tree = |app: Arc<App>| async move {
            let ov = call(&app, "library_overview", json!({})).await;
            ov["schemas"][0]["tree"].to_string()
        };
        // Move to category: planned (with the folders that move), run, journalled
        let change = json!({ "kind": "move", "ids": [lamp], "schema": "home-items", "values": ["Office", "Desk"] });
        let plan = call(&app, "relayout_plan", json!({ "change": change })).await;
        assert_eq!(
            (plan["moving"].clone(), plan["label"].clone()),
            (json!(1), json!("Moved Lamp to Home items › Office › Desk")),
            "{plan}"
        );
        let done = wait(
            &app,
            &call(&app, "relayout_apply", json!({ "change": change })).await,
        )
        .await;
        let moved = done["result"]["journal"].as_str().unwrap().to_string();
        assert!(
            lib.join("Home items/Office/Desk/Lamp/lamp.stl").is_file(),
            "{done}"
        );
        assert!(tree(app.clone()).await.contains("Desk"));
        // the model kept its id
        let q = call(&app, "models_query", json!({ "q": "lamp" })).await;
        assert_eq!(q["items"][0]["id"], json!(lamp));
        // Edit details: journalled; it doesn't stop the move being undone (another model)
        let v = call(
            &app,
            "model_update",
            json!({ "id": hook, "patch": { "name": "Coat hook" }, "journal": true }),
        )
        .await;
        let edited = v["journal"].as_str().unwrap().to_string();
        let hook = v["id"].as_str().unwrap().to_string(); // its model.json gives it a lasting id
        let js = call(&app, "journals", json!({})).await;
        assert_eq!(js[0]["kind"], "details");
        assert_eq!(
            (js[0]["undo"].clone(), js[1]["undo"].clone()),
            (json!(true), json!(true)),
            "{js}"
        );
        // editing the lamp's details does
        let v = call(
            &app,
            "model_update",
            json!({ "id": lamp, "patch": { "tags": "light" }, "journal": true }),
        )
        .await;
        let tagged = v["journal"].as_str().unwrap().to_string();
        let js = call(&app, "journals", json!({})).await;
        assert!(
            js[2]["undo"]
                .as_str()
                .unwrap()
                .starts_with("Undo the newer change first"),
            "{js}"
        );
        let d = wait(
            &app,
            &call(&app, "journal_undo", json!({ "id": moved })).await,
        )
        .await;
        assert!(
            d["error"]
                .as_str()
                .unwrap()
                .starts_with("Undo the newer change first"),
            "{d}"
        );
        wait(
            &app,
            &call(&app, "journal_undo", json!({ "id": tagged })).await,
        )
        .await;
        let done = wait(
            &app,
            &call(&app, "journal_undo", json!({ "id": moved })).await,
        )
        .await;
        assert_eq!(done["result"]["state"], "undone", "{done}");
        assert!(
            lib.join("Unsorted/Lamp/lamp.stl").is_file() && !lib.join("Home items/Office").exists()
        );
        assert!(
            !tree(app.clone()).await.contains("Office"),
            "the subcategory made on the way goes"
        );
        let q = call(&app, "models_query", json!({ "q": "lamp" })).await;
        assert_eq!(q["items"][0]["id"], json!(lamp));
        let done = wait(
            &app,
            &call(&app, "journal_undo", json!({ "id": edited })).await,
        )
        .await;
        assert_eq!(done["result"]["state"], "undone", "{done}");
        assert_eq!(
            model::read_sidecar(&lib.join("Unsorted/Hook"))["name"],
            "Hook"
        );
        // Use as cover: the picture it replaced comes back
        use base64::Engine;
        let png = |c: &[u8]| {
            format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD
                    .encode([b"\x89PNG\r\n\x1a\n".as_slice(), c].concat())
            )
        };
        call(
            &app,
            "model_cover",
            json!({ "id": hook, "snapshot": png(b"one") }),
        )
        .await;
        let v = call(
            &app,
            "model_cover",
            json!({ "id": hook, "snapshot": png(b"two") }),
        )
        .await;
        let cover = lib.join("Unsorted/Hook/_media/cover.png");
        assert!(std::fs::read(&cover).unwrap().ends_with(b"two"));
        wait(
            &app,
            &call(&app, "journal_undo", json!({ "id": v["journal"] })).await,
        )
        .await;
        assert!(std::fs::read(&cover).unwrap().ends_with(b"one"));
        // Import: undone, the folder goes back as it was and the workspace has it again
        let src = home.join("Share");
        std::fs::create_dir_all(src.join("Bench")).unwrap();
        std::fs::write(src.join("Bench/bench.stl"), "solid b").unwrap();
        wait(
            &app,
            &call(
                &app,
                "sort_add",
                json!({ "paths": [src.display().to_string()] }),
            )
            .await,
        )
        .await;
        let se = call(&app, "sort_get", json!({})).await;
        let bench = se["items"][0]["id"].clone();
        let se = call(
            &app,
            "sort_send",
            json!({ "ids": [bench], "schema": "home-items", "values": ["Garage"] }),
        )
        .await;
        assert_eq!(se["changed"], 1, "{se}");
        let done = wait(
            &app,
            &call(&app, "sort_commit", json!({ "mode": "move" })).await,
        )
        .await;
        let imported = done["result"]["journal"].as_str().unwrap().to_string();
        assert!(
            lib.join("Home items/Garage/Bench/bench.stl").is_file() && !src.join("Bench").exists()
        );
        let done = wait(
            &app,
            &call(&app, "journal_undo", json!({ "id": imported })).await,
        )
        .await;
        assert_eq!(
            (
                done["result"]["state"].clone(),
                done["result"]["unmarked"].clone()
            ),
            (json!("undone"), json!(1)),
            "{done}"
        );
        assert!(
            src.join("Bench/bench.stl").is_file()
                && !src.join("Bench/model.json").exists()
                && !src.join("Bench/_thumbs").exists()
        );
        assert!(
            !lib.join("Home items/Garage").exists() && !tree(app.clone()).await.contains("Garage")
        );
        let se = call(&app, "sort_get", json!({})).await;
        assert!(
            se["items"][0]["done"].is_null() && se["items"][0]["placed"] == true,
            "{se}"
        );
        // copied in, then undone: the copy goes and the original stays
        let done = wait(
            &app,
            &call(&app, "sort_commit", json!({ "mode": "copy" })).await,
        )
        .await;
        let copied = done["result"]["journal"].as_str().unwrap().to_string();
        assert!(
            lib.join("Home items/Garage/Bench/bench.stl").is_file()
                && src.join("Bench/bench.stl").is_file()
        );
        wait(
            &app,
            &call(&app, "journal_undo", json!({ "id": copied })).await,
        )
        .await;
        assert!(!lib.join("Home items/Garage").exists() && src.join("Bench/bench.stl").is_file());
        // Read again as a job
        let done = wait(
            &app,
            &call(&app, "library_scan", json!({ "full": true, "job": true })).await,
        )
        .await;
        assert_eq!(done["result"]["models"], 2, "{done}");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn finds_duplicates_and_sets_copies_aside() {
        let (app, home) = app("dupes");
        let lib = home.join("Lib");
        call(
            &app,
            "library_open",
            json!({ "path": lib.display().to_string() }),
        )
        .await;
        for p in [
            "Unsorted/Hook/hook.stl",
            "Unsorted/Hook (2)/hook.stl",
            "Unsorted/Other/other.stl",
        ] {
            std::fs::create_dir_all(lib.join(p).parent().unwrap()).unwrap();
            std::fs::write(
                lib.join(p),
                if p.contains("Other") {
                    "solid o"
                } else {
                    "solid h"
                },
            )
            .unwrap();
        }
        call(&app, "library_scan", json!({ "full": true })).await;
        let none = call(&app, "dupes_get", json!({})).await;
        assert!(none["found"].is_null() && none["groups"].as_array().unwrap().is_empty());
        let done = wait(&app, &call(&app, "dupes_find", json!({})).await).await;
        assert_eq!(done["result"]["groups"], 1, "{done}");
        let found = call(&app, "dupes_get", json!({})).await;
        let g = &found["groups"][0];
        assert_eq!(g["models"].as_array().unwrap().len(), 2);
        let extra: Vec<Value> = g["models"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["id"] != g["keep"])
            .map(|m| m["id"].clone())
            .collect();
        let done = wait(
            &app,
            &call(&app, "dupes_set_aside", json!({ "ids": extra })).await,
        )
        .await;
        assert_eq!(done["result"]["state"], "done", "{done}");
        let found = call(&app, "dupes_get", json!({})).await;
        assert!(found["groups"].as_array().unwrap().is_empty());
        assert_eq!(found["set_aside"]["count"], 1);
        assert_eq!(call(&app, "models_query", json!({})).await["total"], 2);
        // Home lists it with the other changes, and can undo it
        let js = call(&app, "journals", json!({})).await;
        assert_eq!(js[0]["kind"], "set-aside");
        wait(
            &app,
            &call(&app, "journal_undo", json!({ "id": js[0]["id"] })).await,
        )
        .await;
        assert_eq!(call(&app, "models_query", json!({})).await["total"], 3);
        // set aside again and delete the copies
        let found = call(&app, "dupes_get", json!({})).await;
        assert_eq!(found["groups"].as_array().unwrap().len(), 1);
        let g = &found["groups"][0];
        let extra: Vec<Value> = g["models"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["id"] != g["keep"])
            .map(|m| m["id"].clone())
            .collect();
        wait(
            &app,
            &call(&app, "dupes_set_aside", json!({ "ids": extra })).await,
        )
        .await;
        let gone = call(&app, "dupes_empty", json!({})).await;
        assert_eq!(gone["count"], 1);
        assert_eq!(
            call(&app, "journals", json!({})).await[0]["state"],
            "emptied"
        );
        assert_eq!(
            call(&app, "dupes_get", json!({})).await["set_aside"]["count"],
            0
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn notices_folders_changed_outside_the_app() {
        let (app, home) = app("watch");
        let lib = home.join("Lib");
        call(
            &app,
            "library_open",
            json!({ "path": lib.display().to_string() }),
        )
        .await;
        call(&app, "schema_create", json!({ "schema": { "name": "Home items", "subcategories": [{ "name": "Kitchen" }, { "name": "Garage" }] } })).await;
        let put = |p: &str| {
            std::fs::create_dir_all(lib.join(p).parent().unwrap()).unwrap();
            std::fs::write(lib.join(p), "solid x").unwrap();
        };
        let t = std::time::Instant::now();
        while app.watcher.lock().unwrap().is_none() {
            assert!(t.elapsed().as_secs() < 10, "the library is watched");
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        let changed = |want: u64| {
            let app = app.clone();
            async move {
                let t = std::time::Instant::now();
                loop {
                    let r = call(&app, "library_changes", json!({})).await["rev"]
                        .as_u64()
                        .unwrap();
                    if r >= want {
                        return r;
                    }
                    assert!(t.elapsed().as_secs() < 15, "the change was noticed");
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
        };
        let rev = call(&app, "library_changes", json!({})).await["rev"]
            .as_u64()
            .unwrap();
        put("Home items/Kitchen/Jug/jug.stl");
        put("Home items/Kitchen/Jug/Parts/lid.stl");
        let rev = changed(rev + 1).await;
        assert_eq!(call(&app, "models_query", json!({})).await["total"], 1);
        // a star gives the jug its model.json
        let jug = call(&app, "models_query", json!({})).await["items"][0]["id"]
            .as_str()
            .unwrap()
            .to_string();
        let jug = call(&app, "model_star", json!({ "id": jug, "on": true })).await["id"]
            .as_str()
            .unwrap()
            .to_string();
        // a folder dropped in by hand, and a part added deep inside a model
        put("Unsorted/Hook/hook.stl");
        put("Home items/Kitchen/Jug/Parts/Handle/handle.stl");
        let rev = changed(rev + 1).await;
        let q = call(&app, "models_query", json!({})).await;
        assert_eq!(q["total"], 2, "{q}");
        let m = call(&app, "model_get", json!({ "id": jug })).await;
        assert_eq!(m["files"]["count"], 3, "{m}");
        // the jug moved by hand to Garage keeps its id, star and details; its model.json follows
        std::fs::rename(
            lib.join("Home items/Kitchen/Jug"),
            lib.join("Home items/Garage/Jug"),
        )
        .unwrap();
        changed(rev + 1).await;
        let m = call(&app, "model_get", json!({ "id": jug })).await;
        assert_eq!(m["rel"], "Home items/Garage/Jug", "{m}");
        let side = model::read_sidecar(&lib.join("Home items/Garage/Jug"));
        assert_eq!(side["path"], json!(["Garage"]));
        assert_eq!(
            call(&app, "prefs_get", json!({})).await[FAVS_KEY],
            json!([jug])
        );
        // the page's own check, when the window comes back to the front
        let r = call(&app, "library_changes", json!({ "check": true })).await;
        assert_eq!(r["changed"], false);
        drop(app);
        let _ = std::fs::remove_dir_all(&home);
    }
}
