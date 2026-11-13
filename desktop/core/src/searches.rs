//! Saved searches (docs/PLAN.md, "D: saved searches"): a search kept under a name
//! and listed with the collections; it's run again whenever it's opened, so it
//! keeps itself up to date. One file each in `_library/searches/`, apart from the
//! collections, so an older version of the app doesn't take them for empty lists.

use crate::config::{read_json_object, write_json};
use crate::library::{valid_id, Library, APP_DIR};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::path::PathBuf;

pub const DIR: &str = "searches";

fn dir(lib: &Library) -> PathBuf {
    lib.root().join(APP_DIR).join(DIR)
}

fn file(lib: &Library, id: &str) -> Result<PathBuf> {
    valid_id(id)?;
    Ok(dir(lib).join(format!("{id}.json")))
}

/// Every saved search, by name: { id, name, q, scope, created }.
pub fn list(lib: &Library) -> Vec<Value> {
    let mut out: Vec<Value> = std::fs::read_dir(dir(lib))
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
                .map(|e| read_json_object(&e.path()))
                .filter(|v| v["id"].as_str().is_some_and(|id| valid_id(id).is_ok()) && v["name"].is_string() && v["q"].is_string())
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_cached_key(|c| (crate::index::fold(c["name"].as_str().unwrap_or("")), c["id"].as_str().unwrap_or("").to_string()));
    out
}

pub fn get(lib: &Library, id: &str) -> Result<Value> {
    let p = file(lib, id)?;
    if !p.is_file() {
        bail!("That saved search isn't in the library any more.");
    }
    Ok(read_json_object(&p))
}

fn clean_name(name: &str) -> Result<String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        bail!("Give the search a name.");
    }
    if name.chars().count() > 80 {
        bail!("A saved search's name can be at most 80 characters.");
    }
    Ok(name)
}

fn clean_q(q: &str) -> Result<String> {
    let q = q.trim().to_string();
    if q.is_empty() {
        bail!("Type what to search for.");
    }
    Ok(q)
}

/// Where a saved search looks: a place's scope, never another saved search.
fn clean_scope(scope: &str) -> String {
    let scope = scope.trim();
    if scope.is_empty() || scope.starts_with("search:") {
        "all".into()
    } else {
        scope.into()
    }
}

fn taken(lib: &Library, name: &str, except: Option<&str>) -> bool {
    list(lib).iter().any(|c| {
        c["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name)) && c["id"].as_str() != except
    })
}

fn save(lib: &Library, c: &Value) -> Result<()> {
    lib.writable()?;
    let id = c["id"].as_str().ok_or_else(|| anyhow!("a saved search needs an id"))?;
    write_json(&file(lib, id)?, c)
}

/// A new saved search (or one put back by Undo: `restore` is its old file).
pub fn create(lib: &Library, name: &str, q: &str, scope: &str, restore: Option<&Value>) -> Result<Value> {
    lib.writable()?;
    let name = clean_name(name)?;
    if taken(lib, &name, None) {
        bail!("There's already a saved search called {name}.");
    }
    let c = match restore {
        Some(old) => {
            let id = old["id"].as_str().ok_or_else(|| anyhow!("a saved search needs an id"))?;
            if file(lib, id)?.is_file() {
                bail!("That saved search is already there.");
            }
            let mut c = old.clone();
            c["name"] = json!(name);
            c
        }
        None => json!({ "id": crate::library::random_id("s"), "name": name, "q": clean_q(q)?,
            "scope": clean_scope(scope), "created": crate::library::now() }),
    };
    save(lib, &c)?;
    Ok(c)
}

/// Change a saved search's name, search text or place (only those given).
pub fn update(lib: &Library, id: &str, patch: &Value) -> Result<Value> {
    let mut c = get(lib, id)?;
    if let Some(name) = patch["name"].as_str() {
        let name = clean_name(name)?;
        if taken(lib, &name, Some(id)) {
            bail!("There's already a saved search called {name}.");
        }
        c["name"] = json!(name);
    }
    if let Some(q) = patch["q"].as_str() {
        c["q"] = json!(clean_q(q)?);
    }
    if let Some(scope) = patch["scope"].as_str() {
        c["scope"] = json!(clean_scope(scope));
    }
    save(lib, &c)?;
    Ok(c)
}

/// Remove it; returns what it was, for Undo.
pub fn delete(lib: &Library, id: &str) -> Result<Value> {
    lib.writable()?;
    let c = get(lib, id)?;
    std::fs::remove_file(file(lib, id)?)?;
    Ok(c)
}

/// What a page with scope `search:<id>` and `typed` in its search box looks for:
/// (scope, search text). A saved search that's gone finds nothing.
pub fn resolve(lib: &Library, id: &str, typed: &str) -> Option<(String, String)> {
    let s = get(lib, id).ok()?;
    let q = [s["q"].as_str().unwrap_or(""), typed.trim()].iter().filter(|x| !x.is_empty()).cloned().collect::<Vec<_>>().join(" ");
    Some((clean_scope(s["scope"].as_str().unwrap_or("all")), q))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_searches_round_trip() {
        let dir = crate::library::tests::temp_dir("searches");
        let lib = Library::open(&dir).unwrap();
        assert!(list(&lib).is_empty());
        let s = create(&lib, "  Terrain   in 3MF ", "tag:terrain kind:3mf", "schema:home", None).unwrap();
        let id = s["id"].as_str().unwrap().to_string();
        assert_eq!(s["name"], "Terrain in 3MF");
        assert_eq!(s["scope"], "schema:home");
        assert!(create(&lib, "terrain IN 3mf", "x", "all", None).is_err(), "names are unique");
        assert!(create(&lib, "Empty", "  ", "all", None).is_err(), "a search needs text");
        // never a saved search inside a saved search
        let t = create(&lib, "Other", "big", &format!("search:{id}"), None).unwrap();
        assert_eq!(t["scope"], "all");
        assert_eq!(resolve(&lib, &id, " jug "), Some(("schema:home".into(), "tag:terrain kind:3mf jug".into())));
        let u = update(&lib, &id, &json!({ "name": "Terrain", "q": "tag:terrain" })).unwrap();
        assert_eq!((u["name"].as_str(), u["q"].as_str(), u["scope"].as_str()), (Some("Terrain"), Some("tag:terrain"), Some("schema:home")));
        assert!(update(&lib, &id, &json!({ "name": "other" })).is_err());
        assert_eq!(list(&lib).iter().map(|x| x["name"].as_str().unwrap()).collect::<Vec<_>>(), ["Other", "Terrain"]);
        let gone = delete(&lib, &id).unwrap();
        assert!(get(&lib, &id).is_err());
        assert_eq!(resolve(&lib, &id, ""), None);
        create(&lib, gone["name"].as_str().unwrap(), "", "", Some(&gone)).unwrap();
        assert_eq!(get(&lib, &id).unwrap()["q"], "tag:terrain");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
