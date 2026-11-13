//! Tidying the files inside one model (docs/PLAN.md, "A: rename, delete and new
//! folder inside a model"): rename a file or folder, make a folder (and move the
//! selection into it), move files and folders between the model's folders, and
//! delete them into the journal's recovery storage. Every change is one journal
//! (`model_files`) of renames on the same disk, so Undo puts each back; model.json
//! follows (its cover and its recorded fingerprints).

use crate::{extract, index::Index, library::Library, model, relayout};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const MEDIA: &str = "_media";

fn parent(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(p, _)| p)
}

fn base(rel: &str) -> &str {
    rel.rsplit_once('/').map_or(rel, |(_, b)| b)
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() { name.to_string() } else { format!("{dir}/{name}") }
}

/// Inside `of` (or `of` itself).
fn within(rel: &str, of: &str) -> bool {
    rel == of || rel.starts_with(&format!("{of}/"))
}

/// A name for a file or folder in a model: one Windows can use, not hidden.
fn clean_name(name: &str) -> Result<String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        bail!("Type a name.");
    }
    if name.contains('/') || name.contains('\\') {
        bail!("A name can't have / or \\ in it.");
    }
    if name.starts_with('.') {
        bail!("A name can't start with a dot: the app hides those.");
    }
    if crate::health::bad_name(&name) {
        bail!("Windows can't use that name: leave out < > : \" | ? * and a dot or space at the end, and don't use names such as CON or NUL.");
    }
    if name.chars().count() > 200 {
        bail!("That name is too long.");
    }
    Ok(name)
}

/// A file or folder of the model that may be changed here.
fn own(dir: &Path, rel: &str) -> Result<PathBuf> {
    let rel = rel.trim_matches('/');
    if rel.is_empty() {
        bail!("Pick a file or folder.");
    }
    if rel == model::SIDECAR || within(rel, "_thumbs") {
        bail!("The model's details and previews stay as they are.");
    }
    if rel == MEDIA {
        bail!("The _media folder holds the model's pictures and videos; change what's in it instead.");
    }
    if rel.split('/').any(|p| p.starts_with('.')) {
        bail!("{rel} is hidden.");
    }
    let p = extract::checked(dir, rel)?;
    if !p.exists() {
        bail!("{rel} isn't there any more.");
    }
    Ok(p)
}

/// A folder of the model to put things in ("" is the top).
fn own_dir(dir: &Path, rel: &str) -> Result<PathBuf> {
    let rel = rel.trim_matches('/');
    if rel.is_empty() {
        return Ok(dir.to_path_buf());
    }
    if within(rel, "_thumbs") || rel.split('/').any(|p| p.starts_with('.')) {
        bail!("Things can't go in {rel}.");
    }
    let p = extract::checked(dir, rel)?;
    if !p.is_dir() {
        bail!("The folder {rel} isn't there any more.");
    }
    Ok(p)
}

/// Whether `name` is already in the folder (whatever its case), not counting `except`.
fn taken(folder: &Path, name: &str, except: Option<&str>) -> bool {
    std::fs::read_dir(folder).into_iter().flatten().flatten().any(|e| {
        let n = e.file_name().to_string_lossy().into_owned();
        n.eq_ignore_ascii_case(name) && Some(n.as_str()) != except
    })
}

/// model.json after the moves (model-relative from, to; to None: deleted).
fn follow(side: &mut Value, moves: &[(String, Option<String>)]) -> bool {
    let place = |rel: &str| -> Option<Option<String>> {
        moves.iter().find(|(from, _)| within(rel, from)).map(|(from, to)| to.as_ref().map(|t| format!("{t}{}", &rel[from.len()..])))
    };
    let mut changed = false;
    if let Some(cover) = side["cover"].as_str().map(String::from) {
        match place(&cover) {
            Some(Some(now)) => { side["cover"] = json!(now); changed = true; }
            Some(None) => { side.as_object_mut().unwrap().shift_remove("cover"); changed = true; }
            None => {}
        }
    }
    if let Some(h) = side["hashes"].as_object().cloned() {
        let mut out = serde_json::Map::new();
        for (k, v) in h {
            match place(&k) {
                Some(Some(now)) => { out.insert(now, v); changed = true; }
                Some(None) => changed = true,
                None => { out.insert(k, v); }
            }
        }
        side["hashes"] = Value::Object(out);
    }
    changed
}

/// Make the change. `args`: { id, op, … }: rename { rel, name }; new_folder
/// { parent, name, move: [rels] }; move { rels, to }; delete { rels }.
pub fn change(lib: &Library, ix: &Index, args: &Value) -> Result<Value> {
    lib.writable()?;
    let id = args["id"].as_str().ok_or_else(|| anyhow!("Which model?"))?;
    let m = ix.get(id).ok_or_else(|| anyhow!("That model isn't in the library any more."))?;
    let mrel = m.rel().to_string();
    let dir = lib.resolve(&mrel)?;
    let strs = |k: &str| -> Vec<String> {
        let mut v: Vec<String> = args[k].as_array().into_iter().flatten().filter_map(Value::as_str).map(|s| s.trim_matches('/').to_string()).collect();
        v.sort();
        v.dedup();
        // a folder carries what's in it: not both
        let all = v.clone();
        v.retain(|r| !all.iter().any(|o| o != r && within(r, o)));
        v
    };
    let op = args["op"].as_str().unwrap_or("");
    // (from, to) inside the model; to None: deleted
    let mut moves: Vec<(String, Option<String>)> = vec![];
    let mut made: Vec<String> = vec![];
    let label;
    match op {
        "rename" => {
            let rel = args["rel"].as_str().unwrap_or("").trim_matches('/').to_string();
            let p = own(&dir, &rel)?;
            let name = clean_name(args["name"].as_str().unwrap_or(""))?;
            if name == base(&rel) {
                return Ok(json!({ "journal": null, "moved": 0 }));
            }
            let folder = p.parent().unwrap_or(&dir);
            if taken(folder, &name, Some(base(&rel))) {
                bail!("There's already something called {name} there.");
            }
            label = format!("Renamed {} to {name} in {}", base(&rel), m.v["name"].as_str().unwrap_or(""));
            moves.push((rel.clone(), Some(join(parent(&rel), &name))));
        }
        "new_folder" | "move" => {
            let to = if op == "new_folder" {
                let at = args["parent"].as_str().unwrap_or("").trim_matches('/').to_string();
                let folder = own_dir(&dir, &at)?;
                let name = clean_name(args["name"].as_str().unwrap_or(""))?;
                if taken(&folder, &name, None) {
                    bail!("There's already something called {name} there.");
                }
                let rel = join(&at, &name);
                made.push(rel.clone());
                rel
            } else {
                let to = args["to"].as_str().unwrap_or("").trim_matches('/').to_string();
                own_dir(&dir, &to)?;
                to
            };
            let rels = strs(if op == "new_folder" { "move" } else { "rels" });
            let folder = own_dir(&dir, &to).unwrap_or_else(|_| dir.join(&to));
            let mut names: Vec<String> = vec![];
            for rel in &rels {
                own(&dir, rel)?;
                if parent(rel) == to {
                    continue; // already there
                }
                if within(&to, rel) {
                    bail!("A folder can't go inside itself.");
                }
                let name = base(rel).to_string();
                if names.iter().any(|n| n.eq_ignore_ascii_case(&name)) || (op == "move" && taken(&folder, &name, None)) {
                    bail!("There's already something called {name} in {}.", if to.is_empty() { "the model's top folder".to_string() } else { to.clone() });
                }
                names.push(name.clone());
                moves.push((rel.clone(), Some(join(&to, &name))));
            }
            if op == "move" && moves.is_empty() {
                return Ok(json!({ "journal": null, "moved": 0 }));
            }
            let what = match moves.len() { 0 => String::new(), 1 => format!(" and moved {} into it", base(&moves[0].0)), n => format!(" and moved {n} things into it") };
            label = if op == "new_folder" {
                format!("Made the folder {}{what} in {}", base(&to), m.v["name"].as_str().unwrap_or(""))
            } else {
                format!("Moved {} to {} in {}", if moves.len() == 1 { base(&moves[0].0).to_string() } else { format!("{} things", moves.len()) },
                    if to.is_empty() { "the top folder".to_string() } else { base(&to).to_string() }, m.v["name"].as_str().unwrap_or(""))
            };
        }
        "delete" => {
            let rels = strs("rels");
            if rels.is_empty() {
                bail!("Pick files or folders to delete.");
            }
            for rel in &rels {
                own(&dir, rel)?;
                moves.push((rel.clone(), None));
            }
            label = format!("Deleted {} from {}", if rels.len() == 1 { base(&rels[0]).to_string() } else { format!("{} files and folders", rels.len()) }, m.v["name"].as_str().unwrap_or(""));
        }
        _ => bail!("unknown change {op}"),
    }

    // the journal first, so a stop part way can be put back from Home
    let jid = relayout::new_id(lib)?;
    let kept = relayout::kept_dir(lib, &jid).join("files");
    let at = |rel: &str| format!("{mrel}/{rel}");
    let lib_moves: Vec<Value> = moves.iter().enumerate().map(|(i, (from, to))| json!({
        "from": at(from),
        "to": match to { Some(t) => at(t), None => lib.relative(&kept.join(i.to_string())).unwrap_or_default() },
        "name": base(from),
        "deleted": to.is_none(),
    })).collect();
    let side_before = model::read_sidecar(&dir);
    let has_side = side_before["id"].is_string();
    let mut j = json!({ "kind": "model_files", "label": label, "id": jid, "created": crate::library::now(), "state": "running",
        "op": op, "files": lib_moves, "made": made.iter().map(|r| at(r)).collect::<Vec<_>>(),
        "models": [{ "id": id, "name": m.v["name"], "rel": mrel, "before": if has_side { side_before.clone() } else { Value::Null } }] });
    relayout::write(lib, &j)?;
    let undo_done = |done: &[(PathBuf, PathBuf)], made_dirs: &[PathBuf]| {
        for (from, to) in done.iter().rev() {
            let _ = std::fs::rename(to, from);
        }
        for d in made_dirs.iter().rev() {
            let _ = std::fs::remove_dir(d);
        }
    };
    let mut made_dirs: Vec<PathBuf> = vec![];
    for rel in &made {
        let p = dir.join(rel);
        if let Err(e) = std::fs::create_dir(&p) {
            undo_done(&[], &made_dirs);
            j["state"] = json!("undone");
            relayout::write(lib, &j)?;
            return Err(anyhow!("Couldn't make the folder {rel}: {e}"));
        }
        made_dirs.push(p);
    }
    let mut done: Vec<(PathBuf, PathBuf)> = vec![];
    for v in j["files"].as_array().cloned().unwrap_or_default() {
        let from = lib.resolve(v["from"].as_str().unwrap_or(""))?;
        let to = lib.resolve(v["to"].as_str().unwrap_or(""))?;
        let r = to.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::rename(&from, &to));
        if let Err(e) = r {
            undo_done(&done, &made_dirs);
            j["state"] = json!("undone");
            j["error"] = json!(e.to_string());
            relayout::write(lib, &j)?;
            return Err(anyhow!("Couldn't change {}: {e}", v["name"].as_str().unwrap_or("")));
        }
        done.push((from, to));
    }
    if has_side {
        let mut side = side_before.clone();
        follow(&mut side, &moves);
        side["updated"] = json!(crate::library::now());
        crate::config::write_json(&dir.join(model::SIDECAR), &side)?;
        j["models"][0]["after"] = side;
    }
    j["state"] = json!("done");
    j["finished"] = json!(crate::library::now());
    relayout::write(lib, &j)?;
    Ok(json!({ "journal": jid, "moved": moves.len(), "label": j["label"], "to": moves.iter().map(|(_, t)| json!(t)).collect::<Vec<_>>() }))
}

/// Put it back: each file where it was (newest first), the folders it made gone
/// when they're empty, model.json's cover and fingerprints as they were.
pub fn undo(lib: &Library, mut j: Value) -> Result<Value> {
    let files = j["files"].as_array().cloned().unwrap_or_default();
    // check first: nothing is overwritten, nothing half put back
    for v in &files {
        let from = lib.resolve(v["from"].as_str().unwrap_or(""))?;
        let to = lib.resolve(v["to"].as_str().unwrap_or(""))?;
        if !to.exists() {
            bail!("{} isn't where the change put it any more.", v["name"].as_str().unwrap_or(""));
        }
        // a rename that only changed the case: on Windows `from` is the same file
        let same = from.to_string_lossy().eq_ignore_ascii_case(&to.to_string_lossy());
        if from.exists() && !same {
            bail!("Something else is called {} there now; nothing was changed.", v["name"].as_str().unwrap_or(""));
        }
    }
    let mut failed = vec![];
    for v in files.iter().rev() {
        let from = lib.resolve(v["from"].as_str().unwrap_or(""))?;
        let to = lib.resolve(v["to"].as_str().unwrap_or(""))?;
        let r = from.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::rename(&to, &from));
        if let Err(e) = r {
            failed.push(json!({ "name": v["name"], "error": e.to_string() }));
        }
    }
    for d in j["made"].as_array().cloned().unwrap_or_default().iter().rev() {
        if let Ok(p) = lib.resolve(d.as_str().unwrap_or("")) {
            let _ = std::fs::remove_dir(p); // only when empty
        }
    }
    let id = j["id"].as_str().unwrap_or("").to_string();
    let _ = std::fs::remove_dir(relayout::kept_dir(lib, &id).join("files"));
    let _ = std::fs::remove_dir(relayout::kept_dir(lib, &id));
    let mut refresh = vec![];
    for m in j["models"].as_array().cloned().unwrap_or_default() {
        refresh.push(m["id"].clone());
        let Ok(dir) = lib.resolve(m["rel"].as_str().unwrap_or("")) else { continue };
        let mut side = model::read_sidecar(&dir);
        if !side["id"].is_string() || !m["before"].is_object() {
            continue;
        }
        for k in ["cover", "hashes"] {
            match m["before"].get(k) {
                Some(v) => side[k] = v.clone(),
                None => { side.as_object_mut().unwrap().shift_remove(k); }
            }
        }
        side["updated"] = json!(crate::library::now());
        if let Err(e) = crate::config::write_json(&dir.join(model::SIDECAR), &side) {
            failed.push(json!({ "name": m["name"], "error": e.to_string() }));
        }
    }
    if failed.is_empty() {
        j["state"] = json!("undone");
    } else {
        j["state"] = json!("stopped");
        j["error"] = failed[0]["error"].clone();
    }
    relayout::write(lib, &j)?;
    Ok(json!({ "journal": id, "state": j["state"], "failed": failed, "refresh": refresh }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(name: &str) -> (PathBuf, Library, Index, String) {
        let dir = crate::library::tests::temp_dir(name);
        let lib = Library::open(&dir).unwrap();
        let m = dir.join("Unsorted/Robot");
        for f in ["arm.stl", "leg.stl", "Parts/head.stl", "_media/photo.jpg"] {
            let p = m.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, f).unwrap();
        }
        std::fs::write(m.join("model.json"), r#"{"id":"mrobot","name":"Robot","cover":"_media/photo.jpg","hashes":{"arm.stl":{"size":7},"Parts/head.stl":{"size":14}}}"#).unwrap();
        let ix = Index::build(&lib, None, false);
        (dir, lib, ix, "mrobot".into())
    }

    #[test]
    fn rename_move_new_folder_delete_and_undo() {
        let (dir, lib, mut ix, id) = setup("modelfiles");
        let m = dir.join("Unsorted/Robot");
        let side = || model::read_sidecar(&m);
        // rename a file: its fingerprint follows
        // the journals made, oldest first (ids made in the same millisecond don't sort by age)
        let mut made = vec![];
        let mut keep = |r: Value| { if let Some(j) = r["journal"].as_str() { made.push(j.to_string()); } r };
        let r = keep(change(&lib, &ix, &json!({ "id": id, "op": "rename", "rel": "arm.stl", "name": "Arm left.stl" })).unwrap());
        assert!(m.join("Arm left.stl").is_file() && !m.join("arm.stl").exists());
        assert!(side()["hashes"]["Arm left.stl"].is_object() && side()["hashes"]["arm.stl"].is_null());
        assert!(r["journal"].is_string());
        // names Windows can't use, names taken (whatever the case), the model's own files
        for (rel, name) in [("leg.stl", "a:b.stl"), ("leg.stl", "ARM LEFT.STL"), ("leg.stl", ".hidden"), ("model.json", "x.json"), ("_media", "pics")] {
            assert!(change(&lib, &ix, &json!({ "id": id, "op": "rename", "rel": rel, "name": name })).is_err(), "{rel} -> {name}");
        }
        // only the case changes: fine
        keep(change(&lib, &ix, &json!({ "id": id, "op": "rename", "rel": "leg.stl", "name": "Leg.stl" })).unwrap());
        assert!(std::fs::read_dir(&m).unwrap().flatten().any(|e| e.file_name() == "Leg.stl"));
        // a new folder with two files moved into it
        ix.refresh(&lib, &id);
        let r = keep(change(&lib, &ix, &json!({ "id": id, "op": "new_folder", "parent": "", "name": "Limbs", "move": ["Arm left.stl", "Leg.stl"] })).unwrap());
        assert_eq!(r["moved"], 2);
        assert!(m.join("Limbs/Arm left.stl").is_file() && m.join("Limbs/Leg.stl").is_file());
        assert!(side()["hashes"]["Limbs/Arm left.stl"].is_object());
        // a folder can't go into itself; a folder moved carries its fingerprints
        assert!(change(&lib, &ix, &json!({ "id": id, "op": "move", "rels": ["Limbs"], "to": "Limbs" })).is_err());
        assert!(change(&lib, &ix, &json!({ "id": id, "op": "move", "rels": ["Limbs/Leg.stl"], "to": "Limbs" })).unwrap()["journal"].is_null(), "already there");
        keep(change(&lib, &ix, &json!({ "id": id, "op": "move", "rels": ["Parts"], "to": "Limbs" })).unwrap());
        assert!(side()["hashes"]["Limbs/Parts/head.stl"].is_object());
        // the cover moved with its picture, then deleted with it
        let r = keep(change(&lib, &ix, &json!({ "id": id, "op": "delete", "rels": ["_media/photo.jpg", "Limbs/Parts"] })).unwrap());
        assert!(!m.join("_media/photo.jpg").exists() && !m.join("Limbs/Parts").exists());
        assert!(side()["cover"].is_null() && side()["hashes"]["Limbs/Parts/head.stl"].is_null());
        let jdel = r["journal"].as_str().unwrap().to_string();
        assert!(relayout::kept_dir(&lib, &jdel).join("files/0/head.stl").is_file(), "kept, not erased");
        // Undo, newest first, puts each back
        let undo = |jid: &str| super::undo(&lib, relayout::read(&lib, jid).unwrap()).unwrap();
        assert_eq!(undo(&jdel)["state"], "undone");
        assert!(m.join("_media/photo.jpg").is_file() && m.join("Limbs/Parts/head.stl").is_file());
        assert_eq!(side()["cover"], "_media/photo.jpg");
        assert_eq!(made.pop().as_deref(), Some(jdel.as_str()));
        for jid in made.iter().rev() {
            assert_eq!(undo(jid)["state"], "undone");
        }
        assert!(m.join("arm.stl").is_file() && m.join("leg.stl").is_file() && m.join("Parts/head.stl").is_file());
        assert!(!m.join("Limbs").exists(), "the folder it made is gone");
        assert_eq!(side()["hashes"]["arm.stl"]["size"], 7);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
