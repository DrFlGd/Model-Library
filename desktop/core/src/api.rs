//! The app's commands, behind one entry point ([`App::call`]) so the Tauri app
//! and `modlib-cli serve` (the tests' stand-in now, the Docker server later)
//! share them exactly.

use crate::config::{AppConfig, Prefs};
use crate::library::Library;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// Where the app keeps things on this computer.
#[derive(Clone, Debug)]
pub struct AppPaths {
    /// config.json and prefs.json
    pub config_dir: PathBuf,
    /// caches and the search index (Phase 1)
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
}

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
        Ok(Arc::new(Self { paths, library: RwLock::new(None) }))
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

    /// The open library (opening the configured one on first use).
    pub fn library(&self) -> Result<Library, String> {
        if let Some(l) = self.library.read().unwrap().as_ref() {
            return Ok(l.clone());
        }
        let path = self.config().library_path();
        self.open_library(&path).map_err(e2s)
    }

    fn open_library(&self, path: &Path) -> Result<Library> {
        let lib = Library::open(path)?;
        let mut cfg = self.config();
        if cfg.library.as_deref() != Some(path) || cfg.recent_libraries.first().map(PathBuf::as_path) != Some(path) {
            cfg.set_library(path);
            cfg.save(&self.config_path())?;
        }
        *self.library.write().unwrap() = Some(lib.clone());
        Ok(lib)
    }

    pub async fn call(self: &Arc<Self>, cmd: &str, args: Value) -> Result<Reply, String> {
        let j = |v: Value| Ok(Reply::Json(v));
        match cmd {
            "app_info" => {
                let lib = self.library();
                j(json!({
                    "version": crate::VERSION,
                    "os": std::env::consts::OS,
                    "library": lib.as_ref().ok().map(Library::info),
                    "library_error": lib.err(),
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
        let paths = AppPaths { config_dir: home.join("config"), data_dir: home.join("data"), library_url: "/library/".into() };
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
        let info = call(&app, "library_open", json!({ "path": a.display().to_string() })).await;
        assert_eq!(info["name"], "Models A");
        let info = call(&app, "library_rename", json!({ "name": "  Minis " })).await;
        assert_eq!(info["name"], "Minis");
        assert!(app.call("library_rename", json!({ "name": " " })).await.is_err());
        call(&app, "library_open", json!({ "path": home.join("Models B").display().to_string() })).await;
        let info = call(&app, "app_info", json!({})).await;
        assert_eq!(info["library"]["name"], "Models B");
        assert_eq!(info["recent_libraries"].as_array().unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn favourites_live_in_the_library() {
        let (app, home) = app("favs");
        let lib = home.join("Lib");
        call(&app, "library_open", json!({ "path": lib.display().to_string() })).await;
        call(&app, "prefs_set", json!({ "prefs": { "ml-ui": { "theme": "dark" }, "ml-favs": ["m1"] } })).await;
        let p = call(&app, "prefs_get", json!({})).await;
        assert_eq!(p["ml-ui"]["theme"], "dark");
        assert_eq!(p["ml-favs"], json!(["m1"]));
        assert!(Prefs::new(home.join("config/prefs.json")).get().get("ml-favs").is_none());
        assert_eq!(Library::open(&lib).unwrap().favourites(), json!(["m1"]));
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
