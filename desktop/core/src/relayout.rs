//! Re-laying out the library (docs/PLAN.md, "Phase 4 design" and "Subcategory tree
//! design"): renaming, merging or moving a subcategory, editing a schema (its
//! folder, its tree of subcategories, model-folder template) and deleting one all
//! move model folders. Each is planned (every model's old and new folder and path
//! of subcategories), written to a journal in
//! `_library/journal/` before anything moves, then run as a job. The newest
//! change can be undone; an interrupted one can be finished or put back.
//!
//! The UI pass (docs/PLAN.md, "UI pass design", step 2) journals the other changes
//! too: moving models to a category (`move`, planned and run like the rest),
//! importing (`import`, recorded once the files are in) and editing details or the
//! cover (`details`, recorded as it's saved). A change to details can be undone
//! while newer changes leave its models alone.

use crate::import::{destination, file_name, folder_name, place_of, transfer, Progress};
use crate::index::{Index, UNSORTED};
use crate::library::{Library, APP_DIR};
use crate::model;
use crate::schema::{self, Remap, Schema};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

/// Journals kept in a library (older ones are removed).
const KEEP: usize = 20;

fn journal_dir(lib: &Library) -> PathBuf {
    lib.root().join(APP_DIR).join("journal")
}

/// A journal's own folder, for files it keeps (a cover it replaced).
pub fn kept_dir(lib: &Library, id: &str) -> PathBuf {
    journal_dir(lib).join(id)
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .map(|x| x.as_str().unwrap_or("").trim().to_string())
        .collect()
}

/// A move's path of subcategories (`path_before`/`path_after`), or an older
/// journal's category values in order.
fn path_in(m: &Value, side: &str) -> Vec<String> {
    match m[format!("path_{side}")].as_array() {
        Some(_) => strings(&m[format!("path_{side}")]),
        None => m[format!("category_{side}")]
            .as_object()
            .map(|c| {
                c.values()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// Plan a change. `change`: {kind: "category", schema, from: [path], to: [path]}
/// (rename, merge or move a subcategory) | {kind: "schema", schema, spec,
/// rename_folders} | {kind: "delete", schema}.
/// Returns the journal to be (label, schemas before and after, moves).
pub fn plan(lib: &Library, ix: &Index, change: &Value) -> Result<Value> {
    if change["kind"] == "move" {
        return plan_move(lib, ix, change);
    }
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
            let to = strings(&change["to"])
                .iter()
                .map(|n| schema::subcategory_name(n))
                .collect::<Result<Vec<String>>>()?;
            if from.is_empty() || to.is_empty() {
                bail!("Choose a subcategory and where it goes.");
            }
            if from == to {
                bail!("Nothing changes: the names are the same.");
            }
            if to.len() > from.len() && schema::starts_with(&to, &from) {
                bail!("A subcategory can't go inside itself.");
            }
            let maps = [Remap {
                from: from.clone(),
                to: to.clone(),
                keep_below: true,
            }];
            for m in &models {
                let p = path_of(m);
                if schema::starts_with(&p, &from) {
                    wanted.push((m, schema::remap(&p, &maps), None));
                }
            }
            let mut v = schema::as_tree(lib, &old);
            let tree = schema::subcategories(&v);
            if wanted.is_empty() && !tree.iter().any(|p| schema::starts_with(p, &from)) {
                bail!("There's nothing in {}.", from.join(" › "));
            }
            if !schema::in_tree(&tree, &to) {
                schema::check_path(lib, &old.folder, &tree, &to)?;
            }
            let what = if from[..from.len() - 1] == to[..to.len() - 1] {
                format!("Renamed {} to {}", from.last().unwrap(), to.last().unwrap())
            } else {
                format!("Moved {} to {}", from.join(" › "), to.join(" › "))
            };
            // the tree follows: everything below it moves with it
            let mut moved: Vec<Vec<String>> =
                tree.iter().map(|p| schema::remap(p, &maps)).collect();
            moved.push(to.clone());
            schema::set_subcategories(&mut v, &moved);
            (what, Schema::from_value(&v))
        }
        "schema" => {
            let (v, maps) = schema::edited(lib, &old, &change["spec"])?;
            let new =
                Schema::from_value(&v).ok_or_else(|| anyhow!("That category isn't valid."))?;
            let rename = change["rename_folders"].as_bool() == Some(true);
            for m in &models {
                let values = schema::remap(&path_of(m), &maps);
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
        let schema_after = after.as_ref().map(|s| json!(s.id)).unwrap_or(Value::Null);
        let path_after = place_of(lib, after.as_ref(), &dest);
        let to = lib.relative(&dest).unwrap_or_default();
        let path_before = path_of(m);
        if to == m.rel() && schema_after == json!(sid) && path_after == path_before {
            continue; // nothing changes for this one (a name or a field edit)
        }
        moves.push(json!({
            "id": m.id(), "name": m.v["name"],
            "authors": m.v["authors"],
            "from": m.rel(), "to": to,
            "schema_before": sid, "path_before": path_before,
            "schema_after": schema_after, "path_after": path_after,
        }));
    }
    // the page can name the change better (deleting a subcategory is an edit of the category)
    let label = change["label"]
        .as_str()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .unwrap_or(label);
    Ok(json!({
        "kind": kind, "label": label, "schema": sid,
        "schema_before": old.to_json(),
        "schema_after": after.map(|s| s.to_json()),
        "moves": moves,
    }))
}

/// Plan moving models to a category and subcategory, or to Unsorted: `change` is
/// {kind: "move", ids, schema (null: Unsorted), values}. Folders keep their names (a
/// clash gets a number); a subcategory made on the way is in `added`, so undoing
/// takes it away again. Models keep their ids (and stars) when they get a model.json.
fn plan_move(lib: &Library, ix: &Index, change: &Value) -> Result<Value> {
    let target = match change["schema"].as_str().filter(|s| !s.is_empty()) {
        Some(id) => Some(
            ix.schema(id)
                .cloned()
                .ok_or_else(|| anyhow!("There's no category {id} any more."))?,
        ),
        None => None,
    };
    let values = strings(&change["values"]);
    let mut taken: HashSet<PathBuf> = HashSet::new();
    let (mut moves, mut place) = (vec![], None);
    for id in strings(&change["ids"]) {
        let m = ix
            .get(&id)
            .ok_or_else(|| anyhow!("A model isn't in the library any more: read it again."))?;
        let dir = lib.resolve(m.rel())?;
        let dest = destination(
            lib,
            target.as_ref(),
            &values,
            &file_name(&dir),
            Some(&dir),
            &taken,
        )?;
        taken.insert(dest.clone());
        let path_after = place_of(lib, target.as_ref(), &dest);
        place.get_or_insert(path_after.clone());
        let to = lib.relative(&dest).unwrap_or_default();
        let schema_after = target.as_ref().map(|s| json!(s.id)).unwrap_or(Value::Null);
        if to == m.rel() && schema_after == m.v["schema"] && path_after == strings(&m.v["path"]) {
            continue; // it's there already
        }
        moves.push(json!({
            "id": m.id(), "keep_id": true, "name": m.v["name"],
            "authors": m.v["authors"],
            "from": m.rel(), "to": to,
            "schema_before": m.v["schema"], "path_before": strings(&m.v["path"]),
            "schema_after": schema_after, "path_after": path_after,
        }));
    }
    if moves.is_empty() {
        bail!(match strings(&change["ids"]).len() {
            0 => "Choose the models to move.",
            1 => "It's there already: choose another place.",
            _ => "They're there already: choose another place.",
        });
    }
    let path = place.unwrap_or_default();
    let mut added = vec![];
    if let Some(s) = &target {
        let tree = schema::subcategories(&schema::as_tree(lib, s));
        if let Some(k) = (1..=path.len()).find(|&k| !schema::in_tree(&tree, &path[..k])) {
            added.push(json!({ "schema": s.id, "path": path[..k] }));
        }
    }
    let place = match &target {
        Some(s) => std::iter::once(s.name.clone())
            .chain(path.iter().cloned())
            .collect::<Vec<_>>()
            .join(" › "),
        None => UNSORTED.to_string(),
    };
    let what = match moves.as_slice() {
        [one] => one["name"].as_str().unwrap_or("a model").to_string(),
        all => format!("{} models", all.len()),
    };
    Ok(json!({
        "kind": "move", "label": format!("Moved {what} to {place}"),
        "schema": target.as_ref().map(|s| s.id.clone()),
        "schema_before": null, "schema_after": null,
        "added": added, "moves": moves,
    }))
}

/// Whether a change edits a category, and so saves its file when done or undone.
fn edits_schema(j: &Value) -> bool {
    matches!(j["kind"].as_str(), Some("category" | "schema" | "delete"))
}

/// Take away the subcategories a change made, unless something has gone into them
/// (or below them) since.
fn remove_added(lib: &Library, j: &Value) {
    let added: Vec<(String, Vec<String>)> = j["added"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| Some((a["schema"].as_str()?.to_string(), strings(&a["path"]))))
        .collect();
    for (sid, path) in &added {
        let Some(s) = schema::list(lib).into_iter().find(|s| &s.id == sid) else {
            continue;
        };
        let dir = path
            .iter()
            .fold(lib.root().join(&s.folder), |d, x| d.join(x));
        // a subcategory added below it since (not by this change) keeps it
        let others = schema::subcategories(&s.raw).into_iter().any(|p| {
            p.len() > path.len()
                && schema::starts_with(&p, path)
                && !added
                    .iter()
                    .any(|(x, a)| x == sid && schema::starts_with(&p, a))
        });
        if !others && !has_files(&dir) {
            let _ = schema::remove_subcategory(lib, sid, path);
        }
    }
}

/// Whether there's a file anywhere in `dir`.
fn has_files(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|rd| {
        rd.flatten().any(|e| {
            let p = e.path();
            if p.is_dir() {
                has_files(&p)
            } else {
                true
            }
        })
    })
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
    let id = new_id(lib)?;
    begin(lib, &id, plan, "running")?;
    Ok(id)
}

/// An id for a change about to be journalled (time-ordered, so the newest sorts
/// first), for one that keeps files in its folder before it's recorded.
pub fn new_id(lib: &Library) -> Result<String> {
    lib.writable()?;
    std::fs::create_dir_all(journal_dir(lib))?;
    Ok(model::new_id())
}

/// Record a change that's already made (details saved, models imported): "done".
pub fn record(lib: &Library, id: &str, change: &Value) -> Result<()> {
    begin(lib, id, change, "done")
}

fn begin(lib: &Library, id: &str, change: &Value, state: &str) -> Result<()> {
    let mut j = change.clone();
    j["id"] = json!(id);
    j["created"] = json!(crate::library::now());
    j["state"] = json!(state);
    if state == "done" {
        j["finished"] = j["created"].clone();
    }
    write(lib, &j)?;
    // keep the newest few
    let dir = journal_dir(lib);
    let mut all = list(lib);
    for old in all.drain(KEEP.min(all.len())..) {
        let old = old["id"].as_str().unwrap_or("x");
        let _ = std::fs::remove_file(dir.join(format!("{old}.json")));
        let _ = std::fs::remove_dir_all(kept_dir(lib, old));
    }
    Ok(())
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
    let models = j["moves"]
        .as_array()
        .or(j["models"].as_array())
        .map_or(0, Vec::len);
    json!({ "id": j["id"], "kind": j["kind"], "label": j["label"], "created": j["created"], "state": j["state"], "direction": j["direction"], "models": models, "error": j["error"] })
}

/// The recorded changes for Home, newest first, each saying whether it can be
/// undone now (`undo`: true, or why not).
pub fn briefs(lib: &Library) -> Vec<Value> {
    let all = list(lib);
    all.iter()
        .enumerate()
        .map(|(i, j)| {
            let mut b = brief(j);
            b["undo"] = match blocked_by(&all[..i], j) {
                _ if j["state"] != "done" => json!(false),
                Some(newer) => json!(format!(
                    "Undo the newer change first: {}.",
                    newer["label"].as_str().unwrap_or("")
                )),
                None => json!(true),
            };
            b
        })
        .collect()
}

/// The model folders a change leaves things in (where an undo starts from).
fn folders(j: &Value) -> Vec<String> {
    let at = |list: &Value, k: &str| -> Vec<String> {
        list.as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| m[k].as_str().map(String::from))
            .collect()
    };
    match j["kind"].as_str() {
        Some("details") => at(&j["models"], "rel"),
        _ => {
            let mut v = at(&j["moves"], "to");
            v.extend(at(&j["moves"], "from"));
            v
        }
    }
}

/// The newest of `newer` (changes made after `j`, newest first) that stops `j`
/// being undone: changes that move folders are undone newest first; a change to
/// details only waits for newer changes to the same models.
fn blocked_by<'a>(newer: &'a [Value], j: &Value) -> Option<&'a Value> {
    let mine = folders(j);
    let near = |a: &str, b: &str| {
        a == b || a.starts_with(&format!("{b}/")) || b.starts_with(&format!("{a}/"))
    };
    newer
        .iter()
        .filter(|x| x["state"] != "undone" && x["id"] != j["id"])
        .find(|x| {
            if x["kind"] != "details" && j["kind"] != "details" {
                return true;
            }
            folders(x).iter().any(|a| mine.iter().any(|b| near(a, b)))
        })
}

/// Subcategory folders after a change: `target`'s are made, `other`'s that
/// `target` doesn't keep are removed when they're empty.
fn sync_folders(lib: &Library, target: &Value, other: &Value) -> Result<()> {
    let dirs = |v: &Value| -> Vec<PathBuf> {
        let Some(top) = v["folder"].as_str() else {
            return vec![];
        };
        schema::subcategories(v)
            .iter()
            .map(|p| p.iter().fold(lib.root().join(top), |d, x| d.join(x)))
            .collect()
    };
    let keep = dirs(target);
    let mut gone: Vec<PathBuf> = dirs(other)
        .into_iter()
        .filter(|d| !keep.contains(d))
        .collect();
    gone.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    for d in gone {
        let _ = std::fs::remove_dir(&d); // only if empty
    }
    for d in keep {
        std::fs::create_dir_all(d)?;
    }
    Ok(())
}

/// Remove empty folders from `dir` upwards, up to (not including) the stops.
fn prune(lib: &Library, stops: &[PathBuf], mut dir: Option<&Path>) {
    while let Some(d) = dir {
        if stops.iter().any(|s| s == d)
            || !d.starts_with(lib.root())
            || d == lib.root()
            || schema::kept_folder(lib, d)
        {
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
fn place(lib: &Library, from: &str, to: &str, m: &Value, side: &str, p: &Progress) -> Result<()> {
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
    let mut defaults = json!({ "name": m["name"], "authors": if authors.is_empty() { Value::Null } else { json!(authors) } });
    if m["keep_id"] == true {
        defaults["id"] = m["id"].clone(); // a copy set aside comes back as it was
    }
    model::update(&dest, &json!({}), &defaults)?;
    model::set_place(
        &dest,
        m[format!("schema_{side}")].as_str(),
        &path_in(m, side),
    )?;
    Ok(())
}

/// A finished change's categories: the schema saved (or removed) and its folders made.
fn apply_schema(lib: &Library, j: &Value) -> Result<()> {
    match &j["schema_after"] {
        Value::Null => schema::remove(lib, j["schema"].as_str().unwrap_or(""))?,
        v => schema::save(lib, v)?,
    }
    sync_folders(lib, &j["schema_after"], &j["schema_before"])?;
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
    Ok(())
}

/// An undone change's categories: the schema as it was, and its folders.
fn undo_schema(lib: &Library, j: &Value) -> Result<()> {
    schema::save(lib, &j["schema_before"])?;
    sync_folders(lib, &j["schema_before"], &j["schema_after"])?;
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
    Ok(())
}

/// The set-aside copies were deleted: those changes can't be undone or finished.
pub fn mark_emptied(lib: &Library) -> Result<()> {
    for mut j in list(lib) {
        if j["kind"] == "set-aside" && j["state"] != "undone" && j["state"] != "emptied" {
            j["state"] = json!("emptied");
            write(lib, &j)?;
        }
    }
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
    if j["state"] == "emptied" {
        bail!("Those copies were deleted already.");
    }
    if matches!(j["kind"].as_str(), Some("details" | "import")) {
        bail!("That change can't be made again from here.");
    }
    j["state"] = json!("running");
    j["direction"] = json!("apply");
    write(lib, &j)?;
    let moves = j["moves"].as_array().cloned().unwrap_or_default();
    if j["kind"] == "move" {
        // the place is a subcategory before the folders go in (so it isn't tidied away)
        if let Some(sid) = moves.first().and_then(|m| m["schema_after"].as_str()) {
            schema::define_path(lib, sid, &path_in(&moves[0], "after"))?;
        }
    }
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
        match place(lib, from, to, m, "after", &p) {
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
        // setting copies aside and moving models move folders only
        if edits_schema(&j) {
            apply_schema(lib, &j)?;
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
    if j["state"] == "emptied" {
        bail!("Those copies were deleted, so setting them aside can't be undone.");
    }
    let all = list(lib);
    let newer: Vec<Value> = all
        .iter()
        .take_while(|x| x["id"] != j["id"])
        .cloned()
        .collect();
    if let Some(x) = blocked_by(&newer, &j) {
        bail!(
            "Undo the newer change first: {}.",
            x["label"].as_str().unwrap_or("")
        );
    }
    j["state"] = json!("undoing");
    j["direction"] = json!("undo");
    write(lib, &j)?;
    match j["kind"].as_str() {
        Some("details") => return undo_details(lib, j),
        Some("import") => return undo_import(lib, j, cancel, on_item),
        _ => {}
    }
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
            if let Err(e) = place(lib, from, from, m, "before", &p) {
                failed.push(json!({ "name": m["name"], "error": format!("{e:#}") }));
            }
            continue;
        }
        match place(lib, to, from, m, "before", &p) {
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
        if edits_schema(&j) {
            undo_schema(lib, &j)?;
        }
        remove_added(lib, &j);
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

/// A change's end: `ok` when nothing failed, else "stopped" with the first error.
fn end(lib: &Library, j: &mut Value, failed: &[Value], ok: &str) -> Result<()> {
    if failed.is_empty() {
        j["state"] = json!(ok);
        j.as_object_mut().unwrap().shift_remove("error");
    } else {
        j["state"] = json!("stopped");
        j["error"] = json!(failed
            .iter()
            .filter_map(|f| f["error"].as_str())
            .next()
            .unwrap_or(""));
    }
    write(lib, j)
}

/// Put details back: each model's model.json as it was, and the files the change
/// replaced (kept in the journal's folder) or added.
fn undo_details(lib: &Library, mut j: Value) -> Result<Value> {
    let id = j["id"].as_str().unwrap_or("").to_string();
    let (mut done, mut failed, mut refresh) = (0, vec![], vec![]);
    for m in j["models"].as_array().cloned().unwrap_or_default() {
        let (name, rel) = (
            m["name"].as_str().unwrap_or(""),
            m["rel"].as_str().unwrap_or(""),
        );
        let dir = lib.resolve(rel)?;
        if !dir.is_dir() {
            failed
                .push(json!({ "name": name, "error": format!("{name} isn't in {rel} any more") }));
            continue;
        }
        if !m["before"]["id"].is_string() {
            continue;
        }
        match crate::config::write_json(&dir.join(model::SIDECAR), &m["before"]) {
            Ok(()) => {
                done += 1;
                refresh.push(m["id"].clone());
            }
            Err(e) => failed.push(json!({ "name": name, "error": format!("{e:#}") })),
        }
    }
    for f in j["files"].as_array().cloned().unwrap_or_default() {
        let path = lib.resolve(f["rel"].as_str().unwrap_or(""))?;
        let r = match f["kept"].as_str() {
            Some(k) => std::fs::create_dir_all(path.parent().unwrap_or(lib.root()))
                .and_then(|_| std::fs::copy(kept_dir(lib, &id).join(k), &path).map(|_| ())),
            None => match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            },
        };
        if let Err(e) = r {
            failed.push(json!({ "name": f["rel"], "error": e.to_string() }));
        }
    }
    end(lib, &mut j, &failed, "undone")?;
    Ok(
        json!({ "journal": id, "moved": done, "failed": failed, "state": j["state"], "refresh": refresh }),
    )
}

/// Put an import back: each model's files where they came from, or, for a copy
/// whose original is still there, the copy deleted. The subcategories it made go
/// again. `sort` in the result: the workspace items to mark as not imported.
fn undo_import(
    lib: &Library,
    mut j: Value,
    cancel: &AtomicBool,
    on_item: &dyn Fn(usize, usize, &str),
) -> Result<Value> {
    let moves = j["moves"].as_array().cloned().unwrap_or_default();
    let copied = j["mode"] == "copy";
    let stops = stops(lib, &j);
    let p = Progress {
        cancel,
        on_bytes: &|_| {},
    };
    let (mut moved, mut failed, mut back) = (0, vec![], vec![]);
    for (i, m) in moves.iter().enumerate().rev() {
        let name = m["name"].as_str().unwrap_or("");
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            failed.push(json!({ "name": name, "error": "Stopped." }));
            break;
        }
        on_item(moves.len() - 1 - i, moves.len(), name);
        match put_back(lib, m, copied, &p) {
            Ok(()) => {
                moved += 1;
                back.push(m["sort"].clone());
                if let Ok(d) = lib.resolve(m["to"].as_str().unwrap_or("")) {
                    prune(lib, &stops, d.parent());
                }
            }
            Err(e) => failed.push(json!({ "name": name, "error": format!("{e:#}") })),
        }
    }
    if failed.is_empty() {
        remove_added(lib, &j);
    }
    end(lib, &mut j, &failed, "undone")?;
    Ok(
        json!({ "journal": j["id"], "moved": moved, "failed": failed, "state": j["state"], "sort": back }),
    )
}

/// One imported model out of the library again.
fn put_back(lib: &Library, m: &Value, copied: bool, p: &Progress) -> Result<()> {
    let to = m["to"].as_str().unwrap_or("");
    let dest = lib.resolve(to)?;
    let src = PathBuf::from(m["source"].as_str().unwrap_or(""));
    let loose: Vec<PathBuf> = strings(&m["files"])
        .into_iter()
        .filter(|f| !f.is_empty())
        .map(PathBuf::from)
        .collect();
    let originals: Vec<&Path> = if loose.is_empty() {
        vec![src.as_path()]
    } else {
        loose.iter().map(PathBuf::as_path).collect()
    };
    let there = originals.iter().all(|o| o.exists());
    if !dest.exists() {
        if there {
            return Ok(()); // put back already
        }
        bail!("{to} isn't in the library any more");
    }
    if copied && there {
        std::fs::remove_dir_all(&dest)?; // the original is still where it was
        return Ok(());
    }
    // what importing added to the folder goes: its model.json (or the one it had) and preview
    match &m["sidecar_before"] {
        Value::Object(_) => {
            crate::config::write_json(&dest.join(model::SIDECAR), &m["sidecar_before"])?
        }
        _ => {
            let _ = std::fs::remove_file(dest.join(model::SIDECAR));
        }
    }
    if m["thumb_before"] != true {
        let _ = std::fs::remove_file(dest.join(crate::thumb::THUMB));
        if let Some(d) = Path::new(crate::thumb::THUMB).parent() {
            let _ = std::fs::remove_dir(dest.join(d));
        }
    }
    if loose.is_empty() {
        if src.exists() {
            bail!("there's something at {} again", src.display());
        }
        if let Some(w) = transfer(&dest, &src, true, false, p)? {
            bail!("{w}");
        }
    } else {
        for f in &loose {
            if f.exists() {
                bail!("there's something at {} again", f.display());
            }
            if let Some(w) = transfer(&dest.join(file_name(f)), f, true, false, p)? {
                bail!("{w}");
            }
        }
        schema::remove_empty_tree(&dest);
    }
    Ok(())
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

    fn tree(lib: &Library) -> Vec<String> {
        schema::list(lib)[0]
            .tree()
            .iter()
            .map(|p| p.join("/"))
            .collect()
    }

    #[test]
    fn renames_merges_moves_edits_and_undoes() {
        let root = temp_dir("relayout");
        let lib = Library::open(&root).unwrap();
        schema::create(
            &lib,
            &json!({ "name": "Home items", "subcategories": [
                { "name": "Office", "subcategories": [{ "name": "Desk items" }, { "name": "Computer models" }] },
                { "name": "Kitchen" }, { "name": "Garage" }] }),
        )
        .unwrap();
        assert!(root.join("Home items/Office/Computer models").is_dir());
        put(&root.join("Home items/Office/Desk items/Lamp (Jo)/l.stl"));
        put(&root.join("Home items/Office/Desk items/Tray/t.stl"));
        put(&root.join("Home items/Office/Pen pot/p.stl"));
        put(&root.join("Home items/Kitchen/Tray/t2.stl"));
        put(&root.join("Home items/Hook/h.stl"));
        let ix = Index::build(&lib, None, true);
        let mut places: Vec<String> = ix
            .models
            .iter()
            .map(|m| {
                format!(
                    "{} @ {}",
                    m.v["name"].as_str().unwrap(),
                    strings(&m.v["path"]).join("/")
                )
            })
            .collect();
        places.sort();
        assert_eq!(
            places,
            [
                "Hook @ ",
                "Lamp @ Office/Desk items",
                "Pen pot @ Office",
                "Tray @ Kitchen",
                "Tray @ Office/Desk items"
            ]
        );
        // merge Desk items into Kitchen, a level up: a clash gets (2)
        let change = json!({ "kind": "category", "schema": "home-items", "from": ["Office", "Desk items"], "to": ["Kitchen"] });
        let s = summary(&plan(&lib, &ix, &change).unwrap());
        assert_eq!(
            (s["moving"].clone(), s["clashes"].clone()),
            (json!(2), json!(1)),
            "{s}"
        );
        assert!(plan(&lib, &ix, &json!({ "kind": "category", "schema": "home-items", "from": ["Office"], "to": ["Office", "Desk items", "Office"] })).is_err());
        // nor into a model's folder
        let e = plan(&lib, &ix, &json!({ "kind": "category", "schema": "home-items", "from": ["Kitchen"], "to": ["Hook"] })).unwrap_err();
        assert_eq!(
            e.to_string(),
            "Hook is a model's folder, not a subcategory."
        );
        let (r, id) = run(&lib, change);
        assert_eq!(r["state"], "done", "{r}");
        assert!(
            root.join("Home items/Kitchen/Tray (2)/t.stl").is_file()
                && !root.join("Home items/Office/Desk items").exists()
        );
        let side = model::read_sidecar(&root.join("Home items/Kitchen/Lamp (Jo)"));
        assert_eq!(side["path"], json!(["Kitchen"]));
        assert_eq!(
            tree(&lib),
            ["Garage", "Kitchen", "Office", "Office/Computer models"]
        );
        // undo puts it back
        undo(&lib, &id, &AtomicBool::new(false), &|_, _, _| {}).unwrap();
        assert!(
            root.join("Home items/Office/Desk items/Tray/t.stl")
                .is_file()
                && !root.join("Home items/Kitchen/Tray (2)").exists()
        );
        let side = model::read_sidecar(&root.join("Home items/Office/Desk items/Lamp (Jo)"));
        assert_eq!(side["path"], json!(["Office", "Desk items"]));
        assert!(tree(&lib).contains(&"Office/Desk items".to_string()));
        // edit the category: new top folder and folder names, Office renamed to Study
        // with Garage moved into it, Kitchen removed (its models go to the top), and
        // a new branch two deep
        let ix = Index::build(&lib, None, true);
        let spec = json!({ "name": "Rooms", "folder": "Rooms", "model_folder": "{author} - {name}",
            "subcategories": [
                { "name": "Study", "orig": ["Office"], "subcategories": [
                    { "name": "Desk items", "orig": ["Office", "Desk items"] },
                    { "name": "Computer models", "orig": ["Office", "Computer models"] },
                    { "name": "Garage", "orig": ["Garage"] }] },
                { "name": "Bathroom", "subcategories": [{ "name": "Shelves" }] }],
            "removed": [["Kitchen"]] });
        let change = json!({ "kind": "schema", "schema": "home-items", "spec": spec, "rename_folders": true });
        let p = plan(&lib, &ix, &change).unwrap();
        assert!(p["schema_after"].get("levels").is_none());
        let (r, edit) = run(&lib, change);
        assert_eq!(r["moved"], 5, "{r}");
        for f in [
            "Rooms/Study/Desk items/Jo - Lamp/l.stl",
            "Rooms/Study/Desk items/Tray/t.stl",
            "Rooms/Study/Pen pot/p.stl",
            "Rooms/Tray/t2.stl",
            "Rooms/Hook/h.stl",
        ] {
            assert!(root.join(f).is_file(), "{f}");
        }
        assert!(
            root.join("Rooms/Bathroom/Shelves").is_dir()
                && root.join("Rooms/Study/Garage").is_dir()
        );
        assert!(!root.join("Home items").exists() && !root.join("Rooms/Kitchen").exists());
        assert_eq!(
            tree(&lib),
            [
                "Bathroom",
                "Bathroom/Shelves",
                "Study",
                "Study/Computer models",
                "Study/Desk items",
                "Study/Garage"
            ]
        );
        assert_eq!(schema::list(&lib)[0].folder, "Rooms");
        // only the newest change can be undone
        assert!(undo(&lib, &id, &AtomicBool::new(false), &|_, _, _| {}).is_err());
        undo(&lib, &edit, &AtomicBool::new(false), &|_, _, _| {}).unwrap();
        assert!(
            root.join("Home items/Office/Desk items/Lamp (Jo)/l.stl")
                .is_file()
                && root.join("Home items/Kitchen/Tray/t2.stl").is_file()
                && !root.join("Rooms").exists()
        );
        assert!(tree(&lib).contains(&"Kitchen".to_string()));
        // delete: models go to Unsorted
        let (r, _) = run(&lib, json!({ "kind": "delete", "schema": "home-items" }));
        assert_eq!(r["state"], "done");
        assert!(root.join("Unsorted/Hook/h.stl").is_file() && schema::list(&lib).is_empty());
        let side = model::read_sidecar(&root.join("Unsorted/Hook"));
        assert!(
            side.get("schema").is_none() && side.get("path").is_none() && side["id"].is_string()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn older_categories_with_levels_still_move() {
        let root = temp_dir("relayout-levels");
        let lib = Library::open(&root).unwrap();
        schema::create(
            &lib,
            &json!({ "name": "Wargames", "levels": [{ "label": "Game" }, { "label": "Faction" }] }),
        )
        .unwrap();
        put(&root.join("Wargames/40k/Tyranid/Hive Tyrant (Jo)/t.stl"));
        put(&root.join("Wargames/40k/Tyranids/Lictor/l.stl"));
        std::fs::create_dir_all(root.join("Wargames/40k/Orks")).unwrap();
        // a move to another depth makes it a tree; the empty Orks folder is kept in it
        let (r, id) = run(
            &lib,
            json!({ "kind": "category", "schema": "wargames", "from": ["40k", "Tyranid"], "to": ["Tyranid"] }),
        );
        assert_eq!(r["state"], "done", "{r}");
        assert!(root
            .join("Wargames/Tyranid/Hive Tyrant (Jo)/t.stl")
            .is_file());
        assert_eq!(tree(&lib), ["40k", "40k/Orks", "40k/Tyranids", "Tyranid"]);
        let ix = Index::build(&lib, None, true);
        assert_eq!(ix.models.len(), 2);
        // undo brings the levels back
        undo(&lib, &id, &AtomicBool::new(false), &|_, _, _| {}).unwrap();
        assert_eq!(schema::list(&lib)[0].levels.len(), 2);
        assert!(root
            .join("Wargames/40k/Tyranid/Hive Tyrant (Jo)/t.stl")
            .is_file());
        let _ = std::fs::remove_dir_all(&root);
    }
}
