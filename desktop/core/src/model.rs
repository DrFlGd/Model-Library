//! One model: a folder with its files and a `model.json` sidecar of details
//! (docs/PLAN.md, "Metadata: the sidecar"). The sidecar is the source of truth;
//! keys this app doesn't know are kept when it's rewritten.

use crate::config::{read_json_object, write_json};
use anyhow::{bail, Result};
use serde_json::{json, Map, Value};
use std::path::Path;

pub const SIDECAR: &str = "model.json";
pub const FORMAT: u64 = 1;

/// Details the editor sets (besides the schema's own "fields").
pub const DETAILS: [&str; 8] = [
    "name", "authors", "released", "source", "license", "tags", "notes", "cover",
];

/// What kind of file a path is, by its extension.
pub fn file_kind(name: &str) -> &'static str {
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "stl" | "3mf" | "obj" | "step" | "stp" | "ply" | "amf" | "iges" | "igs" | "f3d"
        | "blend" => "model",
        "gcode" | "bgcode" | "lys" | "lyt" | "chitubox" | "ctb" | "cbddlp" | "goo" | "prz"
        | "fabbproject" | "3mfproject" | "ufp" => "slicer",
        "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp" | "avif" => "image",
        "pdf" | "md" | "txt" | "html" | "htm" | "rtf" | "doc" | "docx" => "doc",
        "mp4" | "webm" | "mov" | "m4v" | "mkv" | "avi" => "video",
        "zip" | "7z" | "rar" | "tar" | "gz" => "archive",
        _ => "other",
    }
}

pub const KINDS: [&str; 7] = [
    "model", "slicer", "image", "doc", "video", "archive", "other",
];

/// Filesystem modification time, in milliseconds since the Unix epoch.
pub fn modified(path: &Path) -> u64 {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Files in a model folder (relative paths, '/'), its parts' sub-folders included,
/// without the sidecar and generated previews. Sorted.
pub fn list_files(dir: &Path) -> Vec<(String, u64)> {
    let mut out = vec![];
    walk(dir, "", &mut out);
    out.sort();
    out
}

fn walk(dir: &Path, prefix: &str, out: &mut Vec<(String, u64)>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        match e.file_type() {
            Ok(t) if t.is_dir() => {
                if !(prefix.is_empty() && name == "_thumbs") && !name.starts_with('.') {
                    walk(&e.path(), &rel, out);
                }
            }
            Ok(_) => {
                if !(prefix.is_empty() && name == SIDECAR) && !name.starts_with('.') {
                    out.push((rel, e.metadata().map(|m| m.len()).unwrap_or(0)));
                }
            }
            Err(_) => {}
        }
    }
}

/// Counts of files by kind, total size, how many 3D, slicer and archive files
/// of each type (`exts`: "stl", "zip"…), and the cover picture: the sidecar's
/// `cover`, else the first image in `_media/`, else the first in the folder.
pub fn summarise(files: &[(String, u64)], sidecar: &Value) -> Value {
    let mut kinds = Map::new();
    let mut exts = Map::new();
    let mut bytes = 0u64;
    for (rel, size) in files {
        bytes += size;
        let k = file_kind(rel);
        kinds.insert(
            k.into(),
            json!(kinds.get(k).and_then(Value::as_u64).unwrap_or(0) + 1),
        );
        if matches!(k, "model" | "slicer" | "archive") {
            if let Some((_, e)) = rel.rsplit_once('.') {
                let e = e.to_ascii_lowercase();
                let n = exts.get(&e).and_then(Value::as_u64).unwrap_or(0) + 1;
                exts.insert(e, json!(n));
            }
        }
    }
    let images = || {
        files
            .iter()
            .map(|(r, _)| r)
            .filter(|r| file_kind(r) == "image")
    };
    let cover = sidecar["cover"]
        .as_str()
        .filter(|c| files.iter().any(|(r, _)| r == c))
        .map(String::from)
        .or_else(|| images().find(|r| r.starts_with("_media/")).cloned())
        .or_else(|| images().next().cloned());
    json!({ "kinds": kinds, "exts": exts, "bytes": bytes, "count": files.len(), "cover": cover })
}

/// The sidecar, or {} when the folder has none (or it isn't valid JSON).
pub fn read_sidecar(dir: &Path) -> Value {
    read_json_object(&dir.join(SIDECAR))
}

/// A new model id: time-ordered, so sorting ids sorts by when models were added.
pub fn new_id() -> String {
    use sha2::{Digest, Sha256};
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let seed = format!(
        "{:?}{}{:?}",
        now,
        std::process::id(),
        std::thread::current().id()
    );
    format!(
        "m{:011x}{}",
        now.as_millis(),
        &hex::encode(Sha256::digest(seed.as_bytes()))[..8]
    )
}

/// Tidy one detail as the editor sends it.
pub fn normalise(field: &str, v: &Value) -> Value {
    let list = |s: &str| {
        s.split(',')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(String::from)
            .collect::<Vec<_>>()
    };
    match (field, v) {
        (_, Value::String(s)) if s.trim().is_empty() => Value::Null,
        ("tags", Value::String(s)) => json!(list(s)),
        ("tags", Value::Array(a)) => json!(a
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()),
        ("authors", Value::String(s)) => json!(list(s)
            .into_iter()
            .map(|n| json!({ "name": n }))
            .collect::<Vec<_>>()),
        ("source", Value::String(s)) => json!({ "url": s.trim() }),
        (_, Value::String(s)) => json!(s.trim()),
        _ => v.clone(),
    }
}

/// A model.json's place: `path`, or an older file's `category` values in order.
pub fn path_of(side: &Value) -> Option<Vec<String>> {
    if let Some(a) = side["path"].as_array() {
        return Some(
            a.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect(),
        );
    }
    side["category"].as_object().map(|c| {
        c.values()
            .filter_map(Value::as_str)
            .map(String::from)
            .collect()
    })
}

/// Write a model's details: `patch` holds details (null removes one) and maybe
/// "fields" (the schema's own; null removes one). Creates the sidecar (with an id,
/// `added`, and the given name/category) if the folder has none; its id is
/// `defaults.id` when given. Returns the sidecar.
pub fn update(dir: &Path, patch: &Value, defaults: &Value) -> Result<Value> {
    if !dir.is_dir() {
        bail!("The model's folder {} is gone.", dir.display());
    }
    let mut side = read_sidecar(dir);
    let now = crate::library::now();
    if side.get("id").and_then(Value::as_str).is_none() {
        side["format"] = json!(FORMAT);
        side["id"] = match defaults["id"].as_str() {
            Some(id) if crate::library::valid_id(id).is_ok() => json!(id),
            _ => json!(new_id()),
        };
        side["added"] = json!(now);
        for (k, v) in defaults.as_object().into_iter().flatten() {
            if side.get(k).is_none() && !v.is_null() {
                side[k] = v.clone();
            }
        }
    }
    let obj = side.as_object_mut().unwrap();
    for (k, v) in patch.as_object().into_iter().flatten() {
        if k == "fields" {
            let mut fields = obj
                .get("fields")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            for (fk, fv) in v.as_object().into_iter().flatten() {
                match normalise(fk, fv) {
                    Value::Null => {
                        fields.shift_remove(fk);
                    }
                    x => {
                        fields.insert(fk.clone(), x);
                    }
                }
            }
            if fields.is_empty() {
                obj.shift_remove("fields");
            } else {
                obj.insert("fields".into(), Value::Object(fields));
            }
        } else if DETAILS.contains(&k.as_str()) {
            match normalise(k, v) {
                Value::Null if k == "name" => bail!("A model needs a name."),
                Value::Null => {
                    obj.shift_remove(k);
                }
                x => {
                    obj.insert(k.clone(), x);
                }
            }
        }
    }
    obj.insert("updated".into(), json!(now));
    write_json(&dir.join(SIDECAR), &side)?;
    Ok(side)
}

/// Record where a model now lives: its schema (None: Unsorted) and its path of
/// subcategories. The folder path stays the truth; this keeps model.json in step
/// with it, so a library re-imported elsewhere knows each model's place.
pub fn set_place(dir: &Path, schema: Option<&str>, path: &[String]) -> Result<Value> {
    let mut side = read_sidecar(dir);
    let obj = side.as_object_mut().unwrap();
    obj.shift_remove("category"); // older files: {level: value}
    match schema {
        Some(s) => {
            obj.insert("schema".into(), json!(s));
            obj.insert("path".into(), json!(path));
        }
        None => {
            obj.shift_remove("schema");
            obj.shift_remove("path");
        }
    }
    write_json(&dir.join(SIDECAR), &side)?;
    Ok(side)
}

/// Wrap a manually copied model file and matching pictures/documents, journalling
/// before any move so a partial operation can be put back from Recent changes.
pub fn wrap_loose(lib: &crate::library::Library, ix: &crate::index::Index, rel: &str) -> Result<Value> {
    lib.writable()?;
    let item = ix.loose_files.iter().find(|f| f["rel"].as_str() == Some(rel))
        .ok_or_else(|| anyhow::anyhow!("That file is no longer waiting to be sorted."))?;
    let source = lib.resolve(rel)?;
    if !source.canonicalize()?.starts_with(lib.root().canonicalize()?) { bail!("That file is outside the library."); }
    if !source.symlink_metadata()?.file_type().is_file() { bail!("That is not a regular file."); }
    let parent = source.parent().ok_or_else(|| anyhow::anyhow!("Missing category folder."))?;
    let name = source.file_stem().unwrap().to_string_lossy().into_owned();
    let stem = |p: &Path| crate::import::stem(&crate::import::file_name(p));
    let wanted = stem(&source);
    let mains: Vec<_> = ix.loose_files.iter().filter_map(|f| f["rel"].as_str())
        .filter_map(|r| lib.resolve(r).ok()).filter(|p| p.parent() == Some(parent)).map(|p| stem(&p)).collect();
    let mut files = vec![source.clone()];
    for e in std::fs::read_dir(parent)?.flatten() {
        let path = e.path();
        if !e.file_type()?.is_file() || !matches!(file_kind(&e.file_name().to_string_lossy()), "image" | "doc") { continue; }
        let candidate = stem(&path);
        if mains.iter().filter(|m| candidate.starts_with(m.as_str())).max_by_key(|m| m.len()) == Some(&wanted) { files.push(path); }
    }
    let base = crate::schema::clean_folder_name(name.trim_start_matches(['_', '.']), 120);
    let mut dest = parent.join(&base);
    let mut n = 2;
    while dest.exists() { dest = parent.join(format!("{base} ({n})")); n += 1; }
    let target = lib.relative(&dest).unwrap();
    let moves: Vec<_> = files.iter().map(|f| json!({"from": lib.relative(f), "to": lib.relative(&dest.join(f.file_name().unwrap()))})).collect();
    let jid = crate::relayout::start(lib, &json!({"kind":"wrap_loose", "label":format!("Put {name} in a folder"), "models":[{"rel":target}], "moves":moves, "dest":target}))?;
    let mut created = false;
    let result = (|| -> Result<Value> {
        std::fs::create_dir(&dest)?;
        created = true;
        for f in &files { std::fs::rename(f, dest.join(f.file_name().unwrap()))?; }
        let side = update(&dest, &json!({"name":name}), &json!({"schema": item["schema"], "path": item["path"]}))?;
        let mut j = crate::relayout::read(lib, &jid)?;
        j["state"] = json!("done"); j["finished"] = json!(crate::library::now());
        save_loose_journal(lib, &jid, &j)?;
        Ok(json!({"id":side["id"],"rel":target,"journal":jid}))
    })();
    if result.is_err() {
        if created { let _ = undo_loose(lib, &jid, &std::sync::atomic::AtomicBool::new(false)); }
        else {
            let mut j = crate::relayout::read(lib, &jid)?;
            j["state"] = json!("undone");
            save_loose_journal(lib, &jid, &j)?;
        }
    }
    result
}

fn save_loose_journal(lib: &crate::library::Library, id: &str, j: &Value) -> Result<()> {
    crate::library::valid_id(id)?;
    write_json(&lib.root().join(crate::library::APP_DIR).join("journal").join(format!("{id}.json")), j)
}

/// Resume an undo, checking all conflicts before moving any files back.
pub fn undo_loose(lib: &crate::library::Library, id: &str, cancel: &std::sync::atomic::AtomicBool) -> Result<Value> {
    lib.writable()?;
    let mut j = crate::relayout::read(lib, id)?;
    let dest = lib.resolve(j["dest"].as_str().ok_or_else(|| anyhow::anyhow!("Missing model folder."))?)?;
    let moves = j["moves"].as_array().cloned().unwrap_or_default();
    for m in &moves {
        let from = lib.resolve(m["from"].as_str().unwrap_or(""))?;
        let to = lib.resolve(m["to"].as_str().unwrap_or(""))?;
        if from.symlink_metadata().is_ok() && to.symlink_metadata().is_ok() { bail!("{} already exists; move it aside before undoing.", from.display()); }
        if !from.exists() && !to.exists() { bail!("A file needed to undo this change is missing."); }
    }
    if dest.exists() {
        let expected: std::collections::HashSet<_> = moves.iter().filter_map(|m| m["to"].as_str()).collect();
        for entry in std::fs::read_dir(&dest)? {
            let entry = entry?;
            let file = entry.file_name();
            if file == SIDECAR || file == "_thumbs" { continue; }
            let path = lib.relative(&entry.path()).unwrap_or_default();
            if !expected.contains(path.as_str()) { bail!("The new model has extra files; move them aside before undoing."); }
        }
    }
    j["state"] = json!("undoing"); j["direction"] = json!("undo"); save_loose_journal(lib, id, &j)?;
    for m in &moves {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) { bail!("Stopped; finish undoing from Recent changes."); }
        let from = lib.resolve(m["from"].as_str().unwrap_or(""))?;
        let to = lib.resolve(m["to"].as_str().unwrap_or(""))?;
        if to.exists() { std::fs::rename(to, from)?; }
    }
    if dest.exists() {
        if dest.join(SIDECAR).exists() { std::fs::remove_file(dest.join(SIDECAR))?; }
        if dest.join("_thumbs").is_dir() { std::fs::remove_dir_all(dest.join("_thumbs"))?; }
        std::fs::remove_dir(&dest)?;
    }
    j["state"] = json!("undone"); save_loose_journal(lib, id, &j)?;
    Ok(json!({"journal":id,"moved":moves.len(),"state":"undone","failed":[]}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lone_stl_and_zip_imports_each_have_a_model_folder() {
        use crate::{import, index::Index, library::Library, schema};
        let root = crate::library::tests::temp_dir("single-file-import");
        let lib = Library::open(root.join("Library")).unwrap();
        let category = schema::create(&lib, &json!({"name":"Household"})).unwrap();
        schema::define_path(&lib, &category.id, &["Kitchen".into()]).unwrap();
        let ix = Index::build(&lib, None, true);
        std::fs::create_dir_all(root.join("Downloads")).unwrap();
        let stop = std::sync::atomic::AtomicBool::new(false);
        let progress = import::Progress { cancel: &stop, on_bytes: &|_| {} };
        for (name, ext) in [("hook", "stl"), ("bracket", "zip")] {
            let file = format!("{name}.{ext}");
            let source = root.join("Downloads").join(&file);
            std::fs::write(&source, b"single model").unwrap();
            let scanned = import::scan(&lib, &ix, &[source.clone()], false).unwrap();
            let mut item = scanned["items"][0].clone();
            item["name"] = json!(name); item["schema"] = json!(category.id); item["values"] = json!(["Kitchen"]);
            let plan = import::plan(&lib, &ix, &[item.clone()]);
            let dest = std::path::PathBuf::from(plan[0]["dest"].as_str().unwrap());
            import::commit_one(&lib, &item, &dest, Some(&category), true, false, &Default::default(), &progress).unwrap();
            assert_eq!(dest, lib.root().join(format!("Household/Kitchen/{name}")));
            assert!(dest.join(file).is_file()); assert!(dest.join(SIDECAR).is_file());
            assert!(!source.exists());
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn loose_wrap_preserves_category_matches_longest_stem_and_undo_restores_files() {
        use crate::{index::Index, library::Library, schema};
        let root = crate::library::tests::temp_dir("loose-wrap");
        let lib = Library::open(&root).unwrap();
        let category = schema::create(&lib, &json!({"name":"Household"})).unwrap();
        schema::define_path(&lib, &category.id, &["Kitchen".into()]).unwrap();
        let parent = root.join("Household/Kitchen");
        std::fs::create_dir_all(&parent).unwrap();
        for name in ["hook.stl", "hook.PNG", "hook notes.pdf", "hook_long.zip", "hook_long.png", "unrelated.pdf"] {
            std::fs::write(parent.join(name), name.as_bytes()).unwrap();
        }
        // An existing folder gets a numbered sibling, without changing the category.
        std::fs::create_dir(parent.join("hook")).unwrap();
        let ix = Index::build(&lib, None, true);
        let made = wrap_loose(&lib, &ix, "Household/Kitchen/hook.stl").unwrap();
        assert_eq!(made["rel"], "Household/Kitchen/hook (2)");
        let dest = lib.resolve(made["rel"].as_str().unwrap()).unwrap();
        assert!(dest.join("hook.PNG").exists()); assert!(dest.join("hook notes.pdf").exists());
        assert!(parent.join("hook_long.png").exists()); assert!(parent.join("unrelated.pdf").exists());
        let side = read_sidecar(&dest);
        assert_eq!(side["schema"], category.id); assert_eq!(side["path"], json!(["Kitchen"]));
        assert_eq!(Index::build(&lib, None, true).models.len(), 1);
        let stop = std::sync::atomic::AtomicBool::new(false);
        // Preflight conflicts keep all wrapped files intact.
        std::fs::write(parent.join("hook.stl"), b"new file").unwrap();
        assert!(undo_loose(&lib, made["journal"].as_str().unwrap(), &stop).is_err());
        assert!(dest.join("hook.PNG").exists());
        std::fs::remove_file(parent.join("hook.stl")).unwrap();
        undo_loose(&lib, made["journal"].as_str().unwrap(), &stop).unwrap();
        assert!(!dest.exists()); assert!(parent.join("hook").is_dir());
        for name in ["hook.stl", "hook.PNG", "hook notes.pdf"] { assert_eq!(std::fs::read(parent.join(name)).unwrap(), name.as_bytes()); }
        assert_eq!(Index::build(&lib, None, true).loose_files.len(), 2);
        std::fs::remove_dir_all(root).unwrap();
    }



    #[test]
    fn kinds_by_extension() {
        assert_eq!(file_kind("a/B.STL"), "model");
        assert_eq!(file_kind("x.fabbproject"), "slicer");
        assert_eq!(file_kind("x.lys"), "slicer");
        assert_eq!(file_kind("_media/cover.JPG"), "image");
        assert_eq!(file_kind("readme.md"), "doc");
        assert_eq!(file_kind("x.zip"), "archive");
        assert_eq!(file_kind("noext"), "other");
    }

    #[test]
    fn files_summary_and_cover() {
        let dir = std::env::temp_dir().join(format!("modlib-model-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Arms")).unwrap();
        std::fs::create_dir_all(dir.join("_media")).unwrap();
        std::fs::create_dir_all(dir.join("_thumbs")).unwrap();
        std::fs::write(dir.join("body.stl"), "0123456789").unwrap();
        std::fs::write(dir.join("Arms/left.stl"), "01234").unwrap();
        std::fs::write(dir.join("a.png"), "x").unwrap();
        std::fs::write(dir.join("_media/z.jpg"), "x").unwrap();
        std::fs::write(dir.join("_thumbs/t.webp"), "x").unwrap();
        std::fs::write(dir.join(SIDECAR), "{}").unwrap();
        let files = list_files(&dir);
        assert_eq!(
            files.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(),
            ["Arms/left.stl", "_media/z.jpg", "a.png", "body.stl"]
        );
        let s = summarise(&files, &json!({}));
        assert_eq!(s["kinds"], json!({ "model": 2, "image": 2 }));
        assert_eq!(s["exts"], json!({ "stl": 2 }));
        assert_eq!(s["bytes"], 17);
        assert_eq!(s["cover"], "_media/z.jpg");
        assert_eq!(
            summarise(&files, &json!({ "cover": "a.png" }))["cover"],
            "a.png"
        );
        assert_eq!(
            summarise(&files, &json!({ "cover": "gone.png" }))["cover"],
            "_media/z.jpg"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sidecar_updates_keep_unknown_keys() {
        let dir = std::env::temp_dir().join(format!("modlib-sidecar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let s = update(
            &dir,
            &json!({ "tags": "monster, big ,", "authors": "Jo, Al" }),
            &json!({ "name": "Tyrant", "schema": "wargames" }),
        )
        .unwrap();
        let id = s["id"].as_str().unwrap().to_string();
        assert!(id.starts_with('m'));
        assert_eq!(s["name"], "Tyrant");
        assert_eq!(s["tags"], json!(["monster", "big"]));
        assert_eq!(s["authors"], json!([{ "name": "Jo" }, { "name": "Al" }]));
        // a newer app's key survives an edit
        let mut raw = read_sidecar(&dir);
        raw["future"] = json!({ "x": 1 });
        write_json(&dir.join(SIDECAR), &raw).unwrap();
        let s = update(&dir, &json!({ "tags": null, "fields": { "scale": "32mm" }, "name": "Hive Tyrant", "bogus": 1 }), &json!({})).unwrap();
        assert_eq!(s["id"], json!(id));
        assert_eq!(s["future"], json!({ "x": 1 }));
        assert!(s.get("tags").is_none() && s.get("bogus").is_none());
        assert_eq!(s["fields"], json!({ "scale": "32mm" }));
        assert_eq!(s["name"], "Hive Tyrant");
        assert!(update(&dir, &json!({ "name": " " }), &json!({})).is_err());
        let s = update(&dir, &json!({ "fields": { "scale": null } }), &json!({})).unwrap();
        assert!(s.get("fields").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
