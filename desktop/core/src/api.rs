//! The app's commands, behind one entry point ([`App::call`]) so the Tauri app
//! and `modlib-cli serve` (the tests' stand-in now, the Docker server later)
//! share them exactly.

use crate::config::{AppConfig, Prefs};
use crate::index::Index;
use crate::library::{self, Library};
use crate::{model, schema};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

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

pub struct App {
    pub paths: AppPaths,
    library: RwLock<Option<Library>>,
    /// the open library's index, built on first use (one build at a time)
    index: tokio::sync::Mutex<Option<Index>>,
}

/// What `library()` says before any library was ever opened (the page then asks where).
const NO_LIBRARY: &str = "No library is open yet.";

/// The preference key the page keeps favourites under; they're stored in the library.
const FAVS_KEY: &str = "ml-favs";

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

    pub async fn call(self: &Arc<Self>, cmd: &str, args: Value) -> Result<Reply, String> {
        let j = |v: Value| Ok(Reply::Json(v));
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
            "library_scan" => {
                let full = args["full"].as_bool() == Some(true);
                j(self.with_index(Some(full), |ix, _| json!({ "models": ix.models.len(), "read": ix.read, "ms": ix.ms as u64 })).await?)
            }
            "library_overview" => j(self.with_index(None, |ix, _| ix.overview()).await?),
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
                j(v)
            }
            "model_update" => {
                let id = arg(&args, "id").map_err(e2s)?.to_string();
                let patch = args["patch"].clone();
                let cache = self.index_cache(&self.library()?);
                let r = self
                    .with_index(None, |ix, lib| -> Result<Value> {
                        lib.writable()?;
                        let m = ix.get(&id).ok_or_else(|| anyhow!("That model isn't in the library any more."))?;
                        let dir = lib.resolve(m.rel())?;
                        // a first model.json starts from what the folder says
                        let defaults = json!({ "name": m.v["name"], "schema": m.v["schema"], "category": m.v["category"],
                            "authors": m.v["authors"].as_array().filter(|a| !a.is_empty()).map(|a| a.iter().map(|n| json!({ "name": n })).collect::<Vec<_>>()) });
                        model::update(&dir, &patch, &defaults)?;
                        let v = ix.refresh(lib, &id).ok_or_else(|| anyhow!("couldn't read the model again"))?;
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
            "schema_create" => {
                let lib = self.library()?;
                let s = schema::create(&lib, &args["schema"]).map_err(e2s)?;
                self.with_index(Some(false), |_, _| ()).await?;
                j(s.to_json())
            }
            other => Err(format!("unknown command {other}")),
        }
    }

    /// A file in the open library, by its library-relative path (the `library://` protocol).
    pub fn library_file(&self, rel: &str) -> Result<Vec<u8>> {
        let lib = self.library().map_err(|e| anyhow!(e))?;
        let rel = percent_decode(rel.trim_start_matches('/'));
        Ok(std::fs::read(lib.resolve(&rel)?)?)
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
        assert_eq!(
            side["category"],
            json!({ "game": "Warhammer 40k", "faction": "Tyranid" })
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
}
