//! Re-laying out the library (docs/PLAN.md, "Phase 4 design"): renaming, merging or
//! moving a category value, editing a schema (its folder, levels, model-folder
//! template) and deleting one all move model folders. Each is planned (every
//! model's old and new folder and category), written to a journal in
//! `_library/journal/` before anything moves, then run as a job. The newest
//! change can be undone; an interrupted one can be finished or put back.

use crate::import::{destination, folder_name, transfer, Progress};
use crate::index::{Index, UNSORTED};
use crate::library::{Library, APP_DIR};
use crate::model;
use crate::schema::{self, LevelFrom, Schema};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

/// Journals kept in a library (older ones are removed).
const KEEP: usize = 20;

fn journal_dir(lib: &Library) -> PathBuf {
    lib.root().join(APP_DIR).join("journal")
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .map(|x| x.as_str().unwrap_or("").trim().to_string())
        .collect()
}

/// The category values of a folder under a schema, from its place on disk.
fn values_of(lib: &Library, schema: &Schema, dir: &Path) -> Vec<String> {
    let top = lib.root().join(&schema.folder);
    dir.parent()
        .and_then(|p| p.strip_prefix(&top).ok())
        .map(|r| {
            r.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn category_of(schema: &Schema, values: &[String]) -> Value {
    let m: Map<String, Value> = schema
        .levels
        .iter()
        .zip(values)
        .map(|((k, _), v)| (k.clone(), json!(v)))
        .collect();
    Value::Object(m)
}

/// Plan a change. `change`: {kind: "category", schema, from: [values], to: [values]}
/// | {kind: "schema", schema, spec, rename_folders} | {kind: "delete", schema}.
/// Returns the journal to be (label, schemas before and after, moves).
pub fn plan(lib: &Library, ix: &Index, change: &Value) -> Result<Value> {
    let sid = change["schema"].as_str().unwrap_or("");
    let old = ix
        .schema(sid)
        .cloned()
        .ok_or_else(|| anyhow!("There's no category {sid} any more."))?;
    let models: Vec<&crate::index::Model> = ix
        .models
        .iter()
        .filter(|m| m.v["schema"] == json!(sid))
        .collect();
    let path_of = |m: &crate::index::Model| strings(&m.v["path"]);
    // (model, new values, new folder name) for every model the change touches
    let mut wanted: Vec<(&crate::index::Model, Vec<String>, Option<String>)> = vec![];
    let kind = change["kind"].as_str().unwrap_or("");
    let (label, after): (String, Option<Schema>) = match kind {
        "category" => {
            let from = strings(&change["from"]);
            let to = strings(&change["to"]);
            if from.is_empty() || from.len() != to.len() || from.len() > old.levels.len() {
                bail!("Give a value for each level down to the one you're changing.");
            }
            if let Some(i) = to.iter().position(String::is_empty) {
                bail!("Choose a {}.", old.levels[i].1);
            }
            if from == to {
                bail!("Nothing changes: the values are the same.");
            }
            for m in &models {
                let p = path_of(m);
                if p.len() >= from.len() && p[..from.len()] == from[..] {
                    let mut v = to.clone();
                    v.extend_from_slice(&p[from.len()..]);
                    wanted.push((m, v, None));
                }
            }
            if wanted.is_empty() {
                bail!("There are no models in {}.", from.join(" › "));
            }
            let what = if from[..from.len() - 1] == to[..to.len() - 1] {
                format!("Renamed {} to {}", from.last().unwrap(), to.last().unwrap())
            } else {
                format!("Moved {} to {}", from.join(" › "), to.join(" › "))
            };
            (what, Some(old.clone()))
        }
        "schema" => {
            let (v, levels) = schema::edited(lib, &old, &change["spec"])?;
            let new =
                Schema::from_value(&v).ok_or_else(|| anyhow!("That category isn't valid."))?;
            let rename = change["rename_folders"].as_bool() == Some(true);
            for m in &models {
                let p = path_of(m);
                let values: Vec<String> = levels
                    .iter()
                    .map(|l| match l {
                        LevelFrom::Old(i) => p.get(*i).cloned().unwrap_or_default(),
                        LevelFrom::New(v) => v.clone(),
                    })
                    .collect();
                let name = rename.then(|| {
                    let author = m.v["authors"][0].as_str().unwrap_or("");
                    folder_name(
                        &new.model_folder,
                        m.v["name"].as_str().unwrap_or(""),
                        author,
                    )
                });
                wanted.push((m, values, name));
            }
            (format!("Edited the category {}", new.name), Some(new))
        }
        "delete" => {
            for m in &models {
                wanted.push((m, vec![], None));
            }
            (
                format!(
                    "Deleted the category {} (its models went to Unsorted)",
                    old.name
                ),
                None,
            )
        }
        _ => bail!("unknown change {kind}"),
    };
    let mut taken: HashSet<PathBuf> = HashSet::new();
    let mut moves = vec![];
    for (m, values, name) in wanted {
        let dir = lib.resolve(m.rel())?;
        let folder = name.unwrap_or_else(|| {
            dir.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
        let dest = destination(lib, after.as_ref(), &values, &folder, Some(&dir), &taken)?;
        taken.insert(dest.clone());
        let (schema_after, cat_after) = match &after {
            Some(s) => (json!(s.id), category_of(s, &values_of(lib, s, &dest))),
            None => (Value::Null, json!({})),
        };
        let to = lib.relative(&dest).unwrap_or_default();
        let cat_before = category_of(&old, &path_of(m));
        if to == m.rel() && schema_after == json!(sid) && cat_after == cat_before {
            continue; // nothing changes for this one (a label or a field edit)
        }
        moves.push(json!({
            "id": m.id(), "name": m.v["name"],
            "authors": m.v["authors"],
            "from": m.rel(), "to": to,
            "schema_before": sid, "category_before": cat_before,
            "schema_after": schema_after, "category_after": cat_after,
        }));
    }
    Ok(json!({
        "kind": kind, "label": label, "schema": sid,
        "schema_before": old.to_json(),
        "schema_after": after.map(|s| s.to_json()),
        "moves": moves,
    }))
}

/// What the preview shows: counts and a sample of the moves.
pub fn summary(plan: &Value) -> Value {
    let moves = plan["moves"].as_array().cloned().unwrap_or_default();
    let moving: Vec<&Value> = moves.iter().filter(|m| m["from"] != m["to"]).collect();
    let clashes = moving
        .iter()
        .filter(|m| {
            let base = |k: &str| {
                m[k].as_str()
                    .unwrap_or("")
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .to_string()
            };
            let (a, b) = (base("from"), base("to"));
            a != b && b.ends_with(')') && b.starts_with(a.trim_end_matches(')'))
        })
        .count();
    let folder_before = plan["schema_before"]["folder"].clone();
    json!({
        "label": plan["label"],
        "models": moves.len(),
        "moving": moving.len(),
        "clashes": clashes,
        "folder_before": folder_before,
        "folder_after": plan["schema_after"]["folder"],
        "sample": moving.iter().take(30).map(|m| json!({ "name": m["name"], "from": m["from"], "to": m["to"] })).collect::<Vec<_>>(),
    })
}

/// Write a planned change to the journal ("running"); returns its id.
pub fn start(lib: &Library, plan: &Value) -> Result<String> {
    lib.writable()?;
    let dir = journal_dir(lib);
    std::fs::create_dir_all(&dir)?;
    let id = model::new_id(); // time-ordered, so the newest sorts first
    let mut j = plan.clone();
    j["id"] = json!(id);
    j["created"] = json!(crate::library::now());
    j["state"] = json!("running");
    write(lib, &j)?;
    // keep the newest few
    let mut all = list(lib);
    for old in all.drain(KEEP.min(all.len())..) {
        let _ =
            std::fs::remove_file(dir.join(format!("{}.json", old["id"].as_str().unwrap_or("x"))));
    }
    Ok(id)
}

fn write(lib: &Library, j: &Value) -> Result<()> {
    let id = j["id"].as_str().unwrap_or("");
    crate::library::valid_id(id)?;
    crate::config::write_json(&journal_dir(lib).join(format!("{id}.json")), j)
}

pub fn read(lib: &Library, id: &str) -> Result<Value> {
    crate::library::valid_id(id)?;
    let p = journal_dir(lib).join(format!("{id}.json"));
    if !p.is_file() {
        bail!("That change isn't recorded any more.");
    }
    Ok(crate::config::read_json_object(&p))
}

/// The recorded changes, newest first.
pub fn list(lib: &Library) -> Vec<Value> {
    let mut out: Vec<Value> = std::fs::read_dir(journal_dir(lib))
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
                .map(|e| crate::config::read_json_object(&e.path()))
                .filter(|j| j["id"].is_string())
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| b["id"].as_str().cmp(&a["id"].as_str()));
    out
}

/// What Home shows about a change.
pub fn brief(j: &Value) -> Value {
    let moves = j["moves"].as_array().map_or(0, Vec::len);
    json!({ "id": j["id"], "label": j["label"], "created": j["created"], "state": j["state"], "direction": j["direction"], "models": moves, "error": j["error"] })
}

/// Remove empty folders from `dir` upwards, up to (not including) the stops.
fn prune(lib: &Library, stops: &[PathBuf], mut dir: Option<&Path>) {
    while let Some(d) = dir {
        if stops.iter().any(|s| s == d) || !d.starts_with(lib.root()) || d == lib.root() {
            break;
        }
        if std::fs::remove_dir(d).is_err() {
            break;
        }
        dir = d.parent();
    }
}

fn stops(lib: &Library, j: &Value) -> Vec<PathBuf> {
    let mut s: Vec<PathBuf> = schema::list(lib)
        .iter()
        .map(|s| lib.root().join(&s.folder))
        .collect();
    for k in ["schema_before", "schema_after"] {
        if let Some(f) = j[k]["folder"].as_str() {
            s.push(lib.root().join(f));
        }
    }
    s.push(lib.root().join(UNSORTED));
    s
}

/// Put one model folder in place and record its place in model.json.
fn place(
    lib: &Library,
    from: &str,
    to: &str,
    m: &Value,
    schema: &Value,
    category: &Value,
    p: &Progress,
) -> Result<()> {
    let (src, dest) = (lib.resolve(from)?, lib.resolve(to)?);
    if src != dest && !(dest.is_dir() && !src.exists()) {
        if !src.is_dir() {
            bail!("{from} isn't there any more");
        }
        if let Some(w) = transfer(&src, &dest, true, false, p)? {
            bail!("{w}");
        }
    }
    // a model without model.json gets one, so its id (and star) survives the move
    let authors: Vec<Value> = m["authors"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|n| json!({ "name": n }))
        .collect();
    let defaults = json!({ "name": m["name"], "authors": if authors.is_empty() { Value::Null } else { json!(authors) } });
    model::update(&dest, &json!({}), &defaults)?;
    let cat = category.as_object().cloned().unwrap_or_default();
    model::set_place(&dest, schema.as_str(), &cat)?;
    Ok(())
}

/// Run (or finish) a journalled change. `on_item(i, n, name)` reports progress.
pub fn apply(
    lib: &Library,
    id: &str,
    cancel: &AtomicBool,
    on_item: &dyn Fn(usize, usize, &str),
) -> Result<Value> {
    lib.writable()?;
    let mut j = read(lib, id)?;
    if j["state"] == "done" {
        bail!("That change is already done.");
    }
    j["state"] = json!("running");
    j["direction"] = json!("apply");
    write(lib, &j)?;
    let moves = j["moves"].as_array().cloned().unwrap_or_default();
    let stops = stops(lib, &j);
    let p = Progress {
        cancel,
        on_bytes: &|_| {},
    };
    let (mut moved, mut failed) = (0, vec![]);
    for (i, m) in moves.iter().enumerate() {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            failed.push(json!({ "name": m["name"], "error": "Stopped." }));
            break;
        }
        on_item(i, moves.len(), m["name"].as_str().unwrap_or(""));
        let (from, to) = (
            m["from"].as_str().unwrap_or(""),
            m["to"].as_str().unwrap_or(""),
        );
        match place(
            lib,
            from,
            to,
            m,
            &m["schema_after"],
            &m["category_after"],
            &p,
        ) {
            Ok(()) => {
                moved += 1;
                if from != to {
                    prune(lib, &stops, lib.resolve(from)?.parent());
                }
            }
            Err(e) => failed.push(json!({ "name": m["name"], "error": format!("{e:#}") })),
        }
    }
    if failed.is_empty() {
        match &j["schema_after"] {
            Value::Null => schema::remove(lib, j["schema"].as_str().unwrap_or(""))?,
            v => schema::save(lib, v)?,
        }
        let (before, after) = (
            j["schema_before"]["folder"].as_str(),
            j["schema_after"]["folder"].as_str(),
        );
        if let Some(b) = before.filter(|b| Some(*b) != after) {
            let _ = std::fs::remove_dir(lib.root().join(b)); // only if empty
        }
        if let Some(a) = after {
            std::fs::create_dir_all(lib.root().join(a))?;
        }
        j["state"] = json!("done");
        j.as_object_mut().unwrap().shift_remove("error");
    } else {
        j["state"] = json!("stopped");
        j["error"] = json!(failed
            .iter()
            .filter_map(|f| f["error"].as_str())
            .next()
            .unwrap_or(""));
    }
    j["finished"] = json!(crate::library::now());
    write(lib, &j)?;
    Ok(json!({ "journal": id, "moved": moved, "failed": failed, "state": j["state"] }))
}

/// Put a change back: every folder to where it was, model.json and the schema as
/// they were. Only the newest change can be undone (later ones may depend on it).
pub fn undo(
    lib: &Library,
    id: &str,
    cancel: &AtomicBool,
    on_item: &dyn Fn(usize, usize, &str),
) -> Result<Value> {
    lib.writable()?;
    let mut j = read(lib, id)?;
    if j["state"] == "undone" {
        bail!("That change was already undone.");
    }
    if let Some(newer) = list(lib).into_iter().find(|x| x["state"] != "undone") {
        if newer["id"] != j["id"] {
            bail!("Only the newest change can be undone.");
        }
    }
    j["state"] = json!("undoing");
    j["direction"] = json!("undo");
    write(lib, &j)?;
    let moves = j["moves"].as_array().cloned().unwrap_or_default();
    let stops = stops(lib, &j);
    let p = Progress {
        cancel,
        on_bytes: &|_| {},
    };
    let (mut moved, mut failed) = (0, vec![]);
    for (i, m) in moves.iter().enumerate().rev() {
        on_item(
            moves.len() - 1 - i,
            moves.len(),
            m["name"].as_str().unwrap_or(""),
        );
        let (from, to) = (
            m["from"].as_str().unwrap_or(""),
            m["to"].as_str().unwrap_or(""),
        );
        let back = lib.resolve(to).map(|d| d.is_dir()).unwrap_or(false);
        if !back && lib.resolve(from).map(|d| d.is_dir()).unwrap_or(false) {
            // never moved: just put its model.json back
            if let Err(e) = place(
                lib,
                from,
                from,
                m,
                &m["schema_before"],
                &m["category_before"],
                &p,
            ) {
                failed.push(json!({ "name": m["name"], "error": format!("{e:#}") }));
            }
            continue;
        }
        match place(
            lib,
            to,
            from,
            m,
            &m["schema_before"],
            &m["category_before"],
            &p,
        ) {
            Ok(()) => {
                moved += 1;
                if from != to {
                    prune(lib, &stops, lib.resolve(to)?.parent());
                }
            }
            Err(e) => failed.push(json!({ "name": m["name"], "error": format!("{e:#}") })),
        }
    }
    if failed.is_empty() {
        schema::save(lib, &j["schema_before"])?;
        let (before, after) = (
            j["schema_before"]["folder"].as_str(),
            j["schema_after"]["folder"].as_str(),
        );
        if let Some(a) = after.filter(|a| Some(*a) != before) {
            let _ = std::fs::remove_dir(lib.root().join(a));
        }
        if let Some(b) = before {
            std::fs::create_dir_all(lib.root().join(b))?;
        }
        j["state"] = json!("undone");
        j.as_object_mut().unwrap().shift_remove("error");
    } else {
        j["state"] = json!("stopped");
        j["error"] = json!(failed
            .iter()
            .filter_map(|f| f["error"].as_str())
            .next()
            .unwrap_or(""));
    }
    write(lib, &j)?;
    Ok(json!({ "journal": id, "moved": moved, "failed": failed, "state": j["state"] }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::tests::temp_dir;

    fn put(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "solid x").unwrap();
    }

    fn run(lib: &Library, change: Value) -> (Value, String) {
        let ix = Index::build(lib, None, true);
        let plan = plan(lib, &ix, &change).unwrap();
        let id = start(lib, &plan).unwrap();
        let r = apply(lib, &id, &AtomicBool::new(false), &|_, _, _| {}).unwrap();
        (r, id)
    }

    #[test]
    fn renames_merges_edits_and_undoes() {
        let root = temp_dir("relayout");
        let lib = Library::open(&root).unwrap();
        schema::create(
            &lib,
            &json!({ "name": "Wargames", "levels": [{ "label": "Game" }, { "label": "Faction" }] }),
        )
        .unwrap();
        put(&root.join("Wargames/40k/Tyranid/Hive Tyrant (Jo)/t.stl"));
        put(&root.join("Wargames/40k/Tyranid/Carnifex/c.stl"));
        put(&root.join("Wargames/40k/Tyranids/Lictor/l.stl"));
        put(&root.join("Wargames/40k/Tyranids/Carnifex/c2.stl"));
        // merge Tyranid into Tyranids: a clash gets (2)
        let ix = Index::build(&lib, None, true);
        let p = plan(&lib, &ix, &json!({ "kind": "category", "schema": "wargames", "from": ["40k", "Tyranid"], "to": ["40k", "Tyranids"] })).unwrap();
        let s = summary(&p);
        assert_eq!(
            (s["moving"].clone(), s["clashes"].clone()),
            (json!(2), json!(1)),
            "{s}"
        );
        let (r, id) = run(
            &lib,
            json!({ "kind": "category", "schema": "wargames", "from": ["40k", "Tyranid"], "to": ["40k", "Tyranids"] }),
        );
        assert_eq!(r["state"], "done", "{r}");
        assert!(
            root.join("Wargames/40k/Tyranids/Carnifex (2)/c.stl")
                .is_file()
                && !root.join("Wargames/40k/Tyranid").exists()
        );
        let side = model::read_sidecar(&root.join("Wargames/40k/Tyranids/Hive Tyrant (Jo)"));
        assert_eq!(
            side["category"],
            json!({ "game": "40k", "faction": "Tyranids" })
        );
        // undo puts it back
        undo(&lib, &id, &AtomicBool::new(false), &|_, _, _| {}).unwrap();
        assert!(
            root.join("Wargames/40k/Tyranid/Carnifex/c.stl").is_file()
                && !root.join("Wargames/40k/Tyranids/Carnifex (2)").exists()
        );
        let side = model::read_sidecar(&root.join("Wargames/40k/Tyranid/Hive Tyrant (Jo)"));
        assert_eq!(side["category"]["faction"], "Tyranid");
        // edit the schema: new top folder, a level added at the top, folders renamed to the template
        let ix = Index::build(&lib, None, true);
        let spec = json!({ "name": "Minis", "folder": "Minis", "model_folder": "{author} - {name}",
            "levels": [{ "label": "Kind", "value": "Wargames" }, { "key": "game", "label": "Game" }, { "key": "faction", "label": "Army" }] });
        let p = plan(&lib, &ix, &json!({ "kind": "schema", "schema": "wargames", "spec": spec, "rename_folders": true })).unwrap();
        assert_eq!(p["schema_after"]["levels"][2]["key"], "faction");
        let (r, edit) = run(
            &lib,
            json!({ "kind": "schema", "schema": "wargames", "spec": spec, "rename_folders": true }),
        );
        assert_eq!(r["moved"], 4, "{r}");
        assert!(
            root.join("Minis/Wargames/40k/Tyranid/Jo - Hive Tyrant/t.stl")
                .is_file()
                && !root.join("Wargames").exists()
        );
        assert_eq!(schema::list(&lib)[0].folder, "Minis");
        // only the newest change can be undone
        assert!(undo(&lib, &id, &AtomicBool::new(false), &|_, _, _| {}).is_err());
        undo(&lib, &edit, &AtomicBool::new(false), &|_, _, _| {}).unwrap();
        assert!(
            root.join("Wargames/40k/Tyranid/Hive Tyrant (Jo)/t.stl")
                .is_file()
                && !root.join("Minis").exists()
        );
        // delete: models go to Unsorted
        let (r, _) = run(&lib, json!({ "kind": "delete", "schema": "wargames" }));
        assert_eq!(r["state"], "done");
        assert!(root.join("Unsorted/Lictor/l.stl").is_file() && schema::list(&lib).is_empty());
        let side = model::read_sidecar(&root.join("Unsorted/Lictor"));
        assert!(side.get("schema").is_none() && side["id"].is_string());
        let _ = std::fs::remove_dir_all(&root);
    }
}
