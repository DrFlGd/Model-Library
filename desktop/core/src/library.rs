//! The library folder: the user's models in a plain folder tree that makes sense
//! without the app, plus a small `_library/` folder of the app's own files. It can
//! be moved, copied or synced, and opened by any build of the app
//! (docs/PLAN.md, "On-disk layout").
//!
//! ```text
//! library/
//!   _library/library.json         format, id, name, favourites, library-wide settings
//!   _library/schemas/<id>.json    one file per schema (Phase 1)
//!   _library/trash/               removed models, until emptied (Phase 4)
//!   Wargames/Warhammer 40k/…      folders made by the schemas, one folder per model,
//!                                 each with its model.json (Phase 1)
//!   Unsorted/                     imports with no schema yet
//! ```
//!
//! No database lives here: the app's search index is on each computer and can be
//! rebuilt from the folders.

use crate::config::{read_json_object, write_json};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};

/// The library format this build writes. A library with a higher number opens read-only.
pub const FORMAT: u64 = 1;

/// The app's own folder inside a library.
pub const APP_DIR: &str = "_library";

const SUBDIRS: [&str; 2] = ["_library/schemas", "Unsorted"];

const README: &str = "Model Library\n\
\n\
This folder is a library for the Model Library desktop app. Your models are in\n\
the folders beside this one, one folder per model, laid out by your schemas\n\
(for example Wargames/Warhammer 40k/Tyranid/Hive Tyrant (Author)/). Each model\n\
folder has a model.json with its details, so the library can be moved, copied\n\
or synced, and opened again by any version of the app.\n\
\n\
  _library/library.json   name, favourites and library-wide settings\n\
  _library/schemas/       your categories: their subcategory folders and fields\n\
  Unsorted/               imported models that have no schema yet\n\
\n\
The app's search index and caches are kept on each computer, not here.\n";

/// Variant folder names a library starts with (Settings changes them).
pub const DEFAULT_VARIANTS: &[&str] = &[
    "Presupported",
    "Supported",
    "Unsupported",
    "No supports",
    "Sized",
    "Split",
    "FDM",
    "Resin",
];

#[derive(Clone, Debug)]
pub struct Library {
    root: PathBuf,
    /// Made by a newer app (format above [`FORMAT`]): nothing is written.
    read_only: Option<String>,
}

pub fn valid_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 96
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
        || id.starts_with('.')
    {
        bail!("invalid id {id}");
    }
    Ok(())
}

/// A library-relative path ("Wargames/Tyranid/a.stl") that stays inside the library.
pub fn rel_inside(rel: &str) -> Result<PathBuf> {
    let p = Path::new(rel.trim_start_matches('/'));
    if rel.is_empty() || p.components().any(|c| !matches!(c, Component::Normal(_))) {
        bail!("invalid library path {rel}");
    }
    Ok(p.to_path_buf())
}

/// "2026-10-05T09:48:26Z" from seconds since 1970.
pub fn iso_from_unix(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // civil_from_days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// The time now, as [`iso_from_unix`] writes it.
pub fn now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    iso_from_unix(secs as i64)
}

fn random_id(prefix: &str) -> String {
    use sha2::{Digest, Sha256};
    let seed = format!(
        "{:?}{}{:?}",
        std::time::SystemTime::now(),
        std::process::id(),
        std::thread::current().id()
    );
    format!(
        "{prefix}{}",
        &hex::encode(Sha256::digest(seed.as_bytes()))[..16]
    )
}

impl Library {
    /// Open a library folder, creating it (and `_library/`) if needed. A folder that
    /// already holds models is left as it is: only `_library/` and `Unsorted/` are added.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)
            .with_context(|| format!("couldn't create {}", root.display()))?;
        let meta_path = root.join(APP_DIR).join("library.json");
        if meta_path.is_file() {
            let format = read_json_object(&meta_path)["format"].as_u64().unwrap_or(0);
            if format > FORMAT {
                let msg = format!(
                    "This library was made by a newer version of Model Library (library format {format}; this version reads up to {FORMAT}). \
                     It's open read-only: update the app to change it."
                );
                return Ok(Self {
                    root,
                    read_only: Some(msg),
                });
            }
        }
        let lib = Self {
            root,
            read_only: None,
        };
        for d in SUBDIRS {
            let p = lib.root.join(d);
            std::fs::create_dir_all(&p)
                .with_context(|| format!("couldn't create {}", p.display()))?;
        }
        if !meta_path.is_file() {
            let name = lib
                .root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Library".into());
            write_json(
                &meta_path,
                &json!({ "format": FORMAT, "id": random_id("lib-"), "name": name, "created": now(), "favourites": [] }),
            )?;
        }
        let readme = lib.root.join(APP_DIR).join("README.txt");
        if !readme.exists() {
            let _ = std::fs::write(&readme, README);
        }
        Ok(lib)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn read_only(&self) -> Option<&str> {
        self.read_only.as_deref()
    }

    pub fn writable(&self) -> Result<()> {
        match &self.read_only {
            Some(msg) => bail!("{msg}"),
            None => Ok(()),
        }
    }

    fn meta_path(&self) -> PathBuf {
        self.root.join(APP_DIR).join("library.json")
    }

    pub fn meta(&self) -> Value {
        read_json_object(&self.meta_path())
    }

    /// Merge `patch` into library.json (top-level keys; null removes one).
    pub fn update_meta(&self, patch: Value) -> Result<Value> {
        self.writable()?;
        let mut meta = self.meta();
        for (k, v) in patch.as_object().into_iter().flatten() {
            if k == "format" || k == "id" {
                continue;
            }
            if v.is_null() {
                meta.as_object_mut().unwrap().shift_remove(k);
            } else {
                meta[k] = v.clone();
            }
        }
        write_json(&self.meta_path(), &meta)?;
        Ok(meta)
    }

    pub fn info(&self) -> Value {
        let meta = self.meta();
        json!({
            "path": self.root.display().to_string(),
            "id": meta["id"], "name": meta["name"], "format": meta["format"], "created": meta["created"],
            "read_only": self.read_only,
            "variant_folders": self.variant_folders(),
        })
    }

    /// Folder names that mark a model's variants rather than its parts
    /// (library.json `variant_folders`, else [`DEFAULT_VARIANTS`]).
    pub fn variant_folders(&self) -> Vec<String> {
        match self.meta().get("variant_folders").and_then(Value::as_array) {
            Some(a) => a
                .iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect(),
            None => DEFAULT_VARIANTS.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Set the variant folder names (trimmed, blanks and repeats dropped); None
    /// goes back to the defaults.
    pub fn set_variant_folders(&self, names: Option<&[String]>) -> Result<Vec<String>> {
        let Some(names) = names else {
            self.update_meta(json!({ "variant_folders": null }))?;
            return Ok(self.variant_folders());
        };
        let mut out: Vec<String> = vec![];
        for n in names {
            let n = n.trim();
            if n.chars().count() > 60 {
                bail!("A variant folder name can be at most 60 characters.");
            }
            if !n.is_empty() && !out.iter().any(|o| o.eq_ignore_ascii_case(n)) {
                out.push(n.to_string());
            }
        }
        self.update_meta(json!({ "variant_folders": out }))?;
        Ok(out)
    }

    // ------------------------------------------------------------ favourites

    pub fn favourites(&self) -> Value {
        match self.meta().get("favourites") {
            Some(v @ Value::Array(_)) => v.clone(),
            _ => json!([]),
        }
    }

    pub fn set_favourites(&self, favs: &Value) -> Result<()> {
        if !favs.is_array() {
            bail!("favourites must be a list");
        }
        if &self.favourites() == favs {
            return Ok(());
        }
        self.update_meta(json!({ "favourites": favs }))?;
        Ok(())
    }

    // ------------------------------------------------------------ paths

    /// The library-relative form of a path inside the library (None if outside).
    pub fn relative(&self, p: &Path) -> Option<String> {
        let rel = p.strip_prefix(&self.root).ok()?;
        Some(
            rel.components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/"),
        )
    }

    pub fn resolve(&self, rel: &str) -> Result<PathBuf> {
        Ok(self.root.join(rel_inside(rel)?))
    }
}

/// "BelfrySCAD/BOSL2" -> "belfryscad-bosl2": lowercase letters, digits and single hyphens.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-')
        .chars()
        .take(60)
        .collect::<String>()
        .trim_end_matches('-')
        .to_string()
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("modlib-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn dates() {
        assert_eq!(iso_from_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_from_unix(1_791_150_506), "2026-10-04T21:48:26Z");
        assert_eq!(iso_from_unix(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("Warhammer 40k/Tyranid"), "warhammer-40k-tyranid");
        assert_eq!(slug("  My  Model!! "), "my-model");
    }

    #[test]
    fn creates_a_library_and_keeps_its_id() {
        let dir = temp_dir("create").join("My Models");
        let lib = Library::open(&dir).unwrap();
        assert!(dir.join("_library/library.json").is_file());
        assert!(dir.join("_library/schemas").is_dir() && dir.join("Unsorted").is_dir());
        assert!(dir.join("_library/README.txt").is_file());
        let meta = lib.meta();
        assert_eq!(meta["format"], json!(FORMAT));
        assert_eq!(meta["name"], "My Models");
        lib.update_meta(json!({ "name": "Minis", "id": "changed" }))
            .unwrap();
        let again = Library::open(&dir).unwrap();
        assert_eq!(again.meta()["id"], meta["id"]);
        assert_eq!(again.info()["name"], "Minis");
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn opening_a_folder_of_models_leaves_them_alone() {
        let dir = temp_dir("existing");
        std::fs::create_dir_all(dir.join("Wargames/Tyranid")).unwrap();
        std::fs::write(dir.join("Wargames/Tyranid/hive tyrant.stl"), "solid x").unwrap();
        Library::open(&dir).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("Wargames/Tyranid/hive tyrant.stl")).unwrap(),
            "solid x"
        );
        let mut top: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        top.sort();
        assert_eq!(top, ["Unsorted", "Wargames", "_library"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn newer_format_is_read_only() {
        let dir = temp_dir("newer");
        std::fs::create_dir_all(dir.join("_library")).unwrap();
        std::fs::write(
            dir.join("_library/library.json"),
            r#"{"format": 99, "id": "x", "name": "Future"}"#,
        )
        .unwrap();
        let lib = Library::open(&dir).unwrap();
        assert!(lib.read_only().is_some());
        assert!(lib.set_favourites(&json!(["a"])).is_err());
        assert!(lib.update_meta(json!({ "name": "y" })).is_err());
        assert!(!dir.join("Unsorted").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn favourites_round_trip() {
        let dir = temp_dir("favs");
        let lib = Library::open(&dir).unwrap();
        assert_eq!(lib.favourites(), json!([]));
        lib.set_favourites(&json!(["m1", "m2"])).unwrap();
        assert_eq!(
            Library::open(&dir).unwrap().favourites(),
            json!(["m1", "m2"])
        );
        assert!(lib.set_favourites(&json!("m1")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn paths_stay_inside() {
        assert!(rel_inside("Wargames/Tyranid/a.stl").is_ok());
        assert!(rel_inside("../x").is_err());
        assert!(rel_inside("/etc/passwd").is_ok()); // leading slash is dropped: still relative
        assert!(rel_inside("a/../../b").is_err());
        assert!(valid_id("../x").is_err());
    }
}
