//! Collections (docs/PLAN.md, "Collections design"): named lists of models that
//! cut across categories. One file per collection in `_library/collections/`, so
//! the lists travel with the library and two computers syncing it rarely touch
//! the same file. A model can be in any number; its folder never moves.

use crate::config::{read_json_object, write_json};
use crate::library::{valid_id, Library, APP_DIR};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::path::PathBuf;

pub const DIR: &str = "collections";

fn dir(lib: &Library) -> PathBuf {
    lib.root().join(APP_DIR).join(DIR)
}

fn file(lib: &Library, id: &str) -> Result<PathBuf> {
    valid_id(id)?;
    Ok(dir(lib).join(format!("{id}.json")))
}

/// The collections' model ids, as written (in the order added).
pub fn members(c: &Value) -> Vec<String> {
    c["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(String::from))
        .collect()
}

/// Every collection, by name: { id, name, models, created }.
pub fn list(lib: &Library) -> Vec<Value> {
    let mut out: Vec<Value> = std::fs::read_dir(dir(lib))
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
                .map(|e| read_json_object(&e.path()))
                .filter(|v| v["id"].as_str().is_some_and(|id| valid_id(id).is_ok()) && v["name"].is_string())
                .map(|mut v| {
                    v["models"] = json!(members(&v));
                    v
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_cached_key(|c| (crate::index::fold(c["name"].as_str().unwrap_or("")), c["id"].as_str().unwrap_or("").to_string()));
    out
}

pub fn get(lib: &Library, id: &str) -> Result<Value> {
    let p = file(lib, id)?;
    if !p.is_file() {
        bail!("That collection isn't in the library any more.");
    }
    let mut v = read_json_object(&p);
    v["models"] = json!(members(&v));
    Ok(v)
}

fn clean_name(name: &str) -> Result<String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        bail!("Give the collection a name.");
    }
    if name.chars().count() > 80 {
        bail!("A collection's name can be at most 80 characters.");
    }
    Ok(name)
}

fn taken(lib: &Library, name: &str, except: Option<&str>) -> bool {
    list(lib).iter().any(|c| {
        c["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name)) && c["id"].as_str() != except
    })
}

fn save(lib: &Library, c: &Value) -> Result<()> {
    lib.writable()?;
    let id = c["id"].as_str().ok_or_else(|| anyhow!("a collection needs an id"))?;
    write_json(&file(lib, id)?, c)
}

/// A new, empty collection (or one put back by Undo: `restore` is its old file).
pub fn create(lib: &Library, name: &str, restore: Option<&Value>) -> Result<Value> {
    lib.writable()?;
    let name = clean_name(name)?;
    if taken(lib, &name, None) {
        bail!("There's already a collection called {name}.");
    }
    let c = match restore {
        Some(old) => {
            let id = old["id"].as_str().ok_or_else(|| anyhow!("a collection needs an id"))?;
            if file(lib, id)?.is_file() {
                bail!("That collection is already there.");
            }
            let mut c = old.clone();
            c["name"] = json!(name);
            c["models"] = json!(members(old));
            c
        }
        None => json!({ "id": crate::library::random_id("c"), "name": name, "models": [], "created": crate::library::now() }),
    };
    save(lib, &c)?;
    Ok(c)
}

pub fn rename(lib: &Library, id: &str, name: &str) -> Result<Value> {
    let mut c = get(lib, id)?;
    let name = clean_name(name)?;
    if taken(lib, &name, Some(id)) {
        bail!("There's already a collection called {name}.");
    }
    c["name"] = json!(name);
    save(lib, &c)?;
    Ok(c)
}

/// Remove the list (never its models); returns what it was, for Undo.
pub fn delete(lib: &Library, id: &str) -> Result<Value> {
    lib.writable()?;
    let c = get(lib, id)?;
    std::fs::remove_file(file(lib, id)?)?;
    Ok(c)
}

/// Add and take off models; returns the collection as it is now.
pub fn change(lib: &Library, id: &str, add: &[String], remove: &[String]) -> Result<Value> {
    let mut c = get(lib, id)?;
    let mut ms = members(&c);
    ms.retain(|m| !remove.contains(m));
    for a in add {
        if !ms.contains(a) {
            ms.push(a.clone());
        }
    }
    if ms != members(&c) {
        c["models"] = json!(ms);
        save(lib, &c)?;
    }
    Ok(c)
}

/// A model's id changed (its first model.json, a move): follow it in every
/// collection. Returns how many collections changed.
pub fn rename_member(lib: &Library, old: &str, new: &str) -> Result<usize> {
    if old == new || lib.writable().is_err() {
        return Ok(0);
    }
    let mut n = 0;
    for mut c in list(lib) {
        let ms = members(&c);
        if ms.iter().any(|m| m == old) {
            let mut out: Vec<String> = vec![];
            for m in ms {
                let m = if m == old { new.to_string() } else { m };
                if !out.contains(&m) {
                    out.push(m);
                }
            }
            c["models"] = json!(out);
            save(lib, &c)?;
            n += 1;
        }
    }
    Ok(n)
}

/// Take ids the library doesn't have off every collection (the health check's
/// fix); returns how many entries went.
pub fn forget(lib: &Library, gone: &[String]) -> Result<usize> {
    let mut n = 0;
    for mut c in list(lib) {
        let ms = members(&c);
        let kept: Vec<String> = ms.iter().filter(|m| !gone.contains(m)).cloned().collect();
        if kept.len() != ms.len() {
            n += ms.len() - kept.len();
            c["models"] = json!(kept);
            save(lib, &c)?;
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib() -> (PathBuf, Library) {
        let dir = std::env::temp_dir().join(format!("modlib-coll-{}", crate::library::random_id("t")));
        let lib = Library::open(&dir).unwrap();
        (dir, lib)
    }

    #[test]
    fn collections_round_trip() {
        let (dir, lib) = lib();
        let a = create(&lib, "  Print   next ", None).unwrap();
        assert_eq!(a["name"], "Print next");
        assert!(create(&lib, "print NEXT", None).is_err(), "names are unique, ignoring case");
        assert!(create(&lib, " ", None).is_err());
        let b = create(&lib, "Army list", None).unwrap();
        let (aid, bid) = (a["id"].as_str().unwrap(), b["id"].as_str().unwrap());
        assert!(dir.join("_library/collections").join(format!("{aid}.json")).is_file());
        assert_eq!(list(&lib).iter().map(|c| c["name"].clone()).collect::<Vec<_>>(), [json!("Army list"), json!("Print next")]);
        change(&lib, aid, &["m1".into(), "m2".into(), "m1".into()], &[]).unwrap();
        change(&lib, bid, &["m2".into()], &[]).unwrap();
        assert_eq!(members(&get(&lib, aid).unwrap()), ["m1", "m2"]);
        change(&lib, aid, &[], &["m1".into()]).unwrap();
        assert_eq!(members(&get(&lib, aid).unwrap()), ["m2"]);
        assert_eq!(rename_member(&lib, "m2", "m9").unwrap(), 2);
        assert_eq!(members(&get(&lib, bid).unwrap()), ["m9"]);
        assert!(rename(&lib, aid, "army LIST").is_err());
        assert_eq!(rename(&lib, aid, "To print").unwrap()["name"], "To print");
        let gone = delete(&lib, bid).unwrap();
        assert!(get(&lib, bid).is_err());
        let back = create(&lib, gone["name"].as_str().unwrap(), Some(&gone)).unwrap();
        assert_eq!(back["id"], gone["id"]);
        assert_eq!(members(&back), ["m9"]);
        assert_eq!(forget(&lib, &["m9".into()]).unwrap(), 2);
        assert!(members(&get(&lib, bid).unwrap()).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
