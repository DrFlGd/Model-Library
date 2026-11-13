//! The health check (docs/PLAN.md, "Health check design"): what in a library
//! needs looking at, most serious first, each with the fix the app can make.
//! The quick check reads only folders and model.json files; the deep one also
//! reads every 3D, slicer and archive file whole, compares it with the SHA-256
//! kept in model.json (`hashes`, as Duplicates keeps it) and looks inside
//! archives and 3MF files. The first deep check records what isn't recorded
//! yet, so later checks can tell a damaged file from one that was changed.

use crate::index::{Index, Model, UNSORTED};
use crate::library::{Library, APP_DIR};
use crate::model::{self, SIDECAR};
use anyhow::{bail, Result};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// The kinds of problem, most serious first (the page shows them in this order).
pub const KINDS: [&str; 12] = [
    "damaged",
    "gone",
    "unreadable",
    "broken_json",
    "same_id",
    "empty",
    "cover",
    "lists",
    "no_sidecar",
    "long_path",
    "bad_name",
    "leftovers",
];
/// Items kept of each kind (the count says how many there are).
const MAX_ITEMS: usize = 500;
/// Windows can't always open a longer path (the limit is 260 with the drive).
pub const LONG_PATH: usize = 250;
/// Where leftovers are put so they can be looked at and deleted.
pub const LEFTOVERS: &str = "Leftovers from stopped copies";

fn brief(m: &Model) -> Value {
    json!({ "id": m.id(), "name": m.v["name"], "rel": m.rel(), "key": m.rel() })
}

fn with(mut v: Value, extra: Value) -> Value {
    for (k, x) in extra.as_object().into_iter().flatten() {
        v[k] = x.clone();
    }
    v
}

/// Names Windows can't use: reserved characters, a trailing space or dot, or a
/// device name (CON, NUL, COM1…), with or without an extension.
pub fn bad_name(name: &str) -> bool {
    if name.is_empty() || name == "." || name == ".." {
        return false;
    }
    if name.chars().any(|c| matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || (c as u32) < 32) {
        return true;
    }
    if name.ends_with(' ') || name.ends_with('.') {
        return true;
    }
    let stem = name.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0')
}

/// A file or folder left by a copy, import or archive change that was stopped
/// before it finished (the app's own temporary names).
pub fn leftover_name(name: &str, dir: bool) -> bool {
    if dir {
        [".importing-", ".extract-", ".archive-stage-"].iter().any(|p| name.starts_with(p))
    } else {
        name.starts_with(".model-transfer-")
            || name.rsplit_once('.').is_some_and(|(_, e)| e.starts_with("tmp-") && e.contains("ThreadId"))
    }
}

/// Why a 3D or archive file can't be what it says, from its structure: None when
/// it looks whole (or is locked with a password).
fn structure(path: &Path, name: &str, size: u64) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".stl") {
        return stl_problem(path, size);
    }
    if lower.ends_with(".3mf") {
        let f = std::fs::File::open(path).ok()?;
        return zip::ZipArchive::new(f).err().map(|_| "isn't a whole 3MF file".to_string());
    }
    if crate::archive::is_archive(name) && !crate::archive::is_part(name) {
        if let Err(e) = crate::archive::list(path) {
            let e = format!("{e:#}");
            if e != crate::archive::LOCKED && e != crate::archive::WRONG_PASSWORD {
                return Some("can't be opened as an archive".into());
            }
        }
    }
    None
}

/// A binary STL is 84 bytes and 50 per triangle; a text one ends with
/// "endsolid". Either cut short is a download or copy that didn't finish.
fn stl_problem(path: &Path, size: u64) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut head = [0u8; 84];
    let n = f.read(&mut head).ok()?;
    let binary_ok = n == 84 && {
        let tris = u32::from_le_bytes([head[80], head[81], head[82], head[83]]) as u64;
        84 + 50 * tris == size
    };
    if binary_ok {
        return None;
    }
    if head[..n.min(5)].eq_ignore_ascii_case(b"solid") {
        let back = size.min(1024);
        let mut tail = vec![0u8; back as usize];
        if f.seek(SeekFrom::Start(size - back)).is_ok() && f.read_exact(&mut tail).is_ok() {
            let t = String::from_utf8_lossy(&tail).to_ascii_lowercase();
            if t.contains("endsolid") {
                return None;
            }
        }
        return Some("is cut short (a text STL without its end)".into());
    }
    Some("is cut short or isn't an STL (its size doesn't match its triangles)".into())
}

/// Leftovers anywhere in the library: (path in the library, folder?, bytes).
fn leftovers(lib: &Library, keep: &HashSet<String>) -> Vec<(String, bool, u64)> {
    fn walk(lib: &Library, dir: &Path, depth: usize, keep: &HashSet<String>, out: &mut Vec<(String, bool, u64)>) {
        if depth > 40 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(t) = e.file_type() else { continue };
            if t.is_symlink() {
                continue;
            }
            let p = e.path();
            let is_dir = t.is_dir();
            if leftover_name(&name, is_dir) {
                if !keep.iter().any(|id| name.contains(id.as_str())) {
                    if let Some(rel) = lib.relative(&p) {
                        out.push((rel, is_dir, crate::import::tree_bytes(&p)));
                    }
                }
            } else if is_dir
                && !name.starts_with('.')
                && !(depth == 1 && name == crate::dupes::SET_ASIDE && dir.ends_with(APP_DIR))
            {
                walk(lib, &p, depth + 1, keep, out);
            }
        }
    }
    let mut out = vec![];
    walk(lib, lib.root(), 0, keep, &mut out);
    out.sort();
    out
}

/// The ids of changes not finished yet (their staging folders are kept).
fn unfinished(lib: &Library) -> HashSet<String> {
    crate::relayout::list(lib)
        .into_iter()
        .filter(|j| !matches!(j["state"].as_str(), Some("done" | "undone" | "emptied")))
        .filter_map(|j| j["id"].as_str().map(String::from))
        .collect()
}

struct Found {
    kinds: BTreeMap<usize, (usize, Vec<Value>)>,
}

impl Found {
    fn add(&mut self, kind: &str, v: Value) {
        let i = KINDS.iter().position(|k| *k == kind).expect("a known kind");
        let e = self.kinds.entry(i).or_insert((0, vec![]));
        e.0 += 1;
        if e.1.len() < MAX_ITEMS {
            e.1.push(v);
        }
    }
    fn groups(self) -> Vec<Value> {
        self.kinds
            .into_iter()
            .map(|(i, (count, items))| json!({ "kind": KINDS[i], "count": count, "items": items }))
            .collect()
    }
}

/// Check the library. `deep`: also read every 3D, slicer and archive file.
pub fn check(
    lib: &Library,
    ix: &Index,
    deep: bool,
    cancel: &AtomicBool,
    progress: &dyn Fn(Value),
) -> Result<Value> {
    let stopped = || -> Result<()> {
        if cancel.load(Ordering::Relaxed) {
            bail!("Stopped.");
        }
        Ok(())
    };
    let writable = lib.writable().is_ok();
    let mut found = Found { kinds: BTreeMap::new() };
    let mut ids: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut deep_todo: Vec<(usize, Vec<(String, u64, u64)>)> = vec![];
    let mut files_seen = 0usize;
    for (mi, m) in ix.models.iter().enumerate() {
        stopped()?;
        if mi % 25 == 0 {
            progress(json!({ "step": "models", "item": mi, "items": ix.models.len(), "name": m.v["name"] }));
        }
        let Ok(dir) = lib.resolve(m.rel()) else { continue };
        if !dir.is_dir() {
            continue;
        }
        let files = model::list_files(&dir);
        files_seen += files.len();
        if files.is_empty() {
            found.add("empty", brief(m));
        }
        if m.v["files"]["cover_missing"] == true {
            found.add("cover", with(brief(m), json!({ "cover": m.v["files"]["explicit_cover"] })));
        }
        // long paths: the longest, and how many
        let mut long = 0;
        let mut longest = (0usize, String::new());
        for (rel, _) in &files {
            let n = dir.join(rel).to_string_lossy().encode_utf16().count();
            if n > LONG_PATH {
                long += 1;
                if n > longest.0 {
                    longest = (n, rel.clone());
                }
            }
        }
        if long > 0 {
            found.add("long_path", with(brief(m), json!({ "file": longest.1, "length": longest.0, "files": long })));
        }
        // names Windows can't use, in the model's own path or its files
        let mut bad: Vec<String> = m.rel().split('/').filter(|c| bad_name(c)).map(String::from).collect();
        bad.extend(files.iter().filter(|(rel, _)| rel.split('/').any(bad_name)).map(|(rel, _)| rel.clone()));
        if !bad.is_empty() {
            found.add("bad_name", with(brief(m), json!({ "file": bad[0], "files": bad.len() })));
        }
        let side_path = dir.join(SIDECAR);
        if m.v["sidecar"] == true {
            let side = model::read_sidecar(&dir);
            if let Some(id) = side["id"].as_str() {
                ids.entry(id.to_string()).or_default().push(mi);
            }
            let gone: Vec<String> = side["hashes"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(f, _)| f.clone())
                .filter(|f| crate::library::rel_inside(f).is_err() || !dir.join(f).is_file())
                .collect();
            if !gone.is_empty() {
                found.add("gone", with(brief(m), json!({ "files": gone })));
            }
        } else if side_path.is_file() {
            let ok = std::fs::read(&side_path)
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                .is_some_and(|v| v.is_object());
            if !ok {
                found.add("broken_json", brief(m));
            } else if !m.v["schema"].is_null() {
                found.add("no_sidecar", brief(m));
            }
        } else if !m.v["schema"].is_null() {
            found.add("no_sidecar", brief(m));
        }
        if deep {
            deep_todo.push((mi, crate::dupes::main_files(&dir)));
        }
    }

    // the same id in two or more folders (one copied by hand)
    for (id, ms) in &ids {
        if ms.len() < 2 {
            continue;
        }
        let mut ms: Vec<&Model> = ms.iter().map(|&i| &ix.models[i]).collect();
        ms.sort_by(|a, b| (a.v["moved"] == true).cmp(&(b.v["moved"] == true)).then(a.rel().cmp(b.rel())));
        found.add("same_id", json!({ "id": id, "key": id, "name": ms[0].v["name"],
            "models": ms.iter().enumerate().map(|(i, m)| with(brief(m), json!({ "keep": i == 0 }))).collect::<Vec<_>>() }));
    }

    // lists naming models the library doesn't have
    for f in lib.favourites().as_array().into_iter().flatten().filter_map(Value::as_str) {
        if ix.get(f).is_none() {
            found.add("lists", json!({ "list": "Starred", "coll": null, "model": f, "key": format!("favs|{f}") }));
        }
    }
    for c in crate::collections::list(lib) {
        let cid = c["id"].as_str().unwrap_or("");
        for mid in crate::collections::members(&c) {
            if ix.get(&mid).is_none() {
                found.add("lists", json!({ "list": c["name"], "coll": cid, "model": mid, "key": format!("{cid}|{mid}") }));
            }
        }
    }

    // leftovers of stopped copies (never those an unfinished change still needs)
    progress(json!({ "step": "leftovers" }));
    let keep = unfinished(lib);
    for (rel, is_dir, bytes) in leftovers(lib, &keep) {
        stopped()?;
        found.add("leftovers", json!({ "rel": rel, "key": rel, "folder": is_dir, "bytes": bytes }));
    }

    // deep: read every main file whole
    let mut refresh = vec![];
    let mut bytes_read = 0u64;
    if deep {
        let total: u64 = deep_todo.iter().flat_map(|(_, f)| f.iter().map(|x| x.1)).sum();
        let mut last = 0u64;
        for (mi, files) in &deep_todo {
            let m = &ix.models[*mi];
            let Ok(dir) = lib.resolve(m.rel()) else { continue };
            let mut side = if m.v["sidecar"] == true { model::read_sidecar(&dir) } else { Value::Null };
            let mut learnt = Map::new();
            for (rel, size, modified) in files {
                stopped()?;
                let path = dir.join(rel);
                if *size == 0 {
                    found.add("unreadable", with(brief(m), json!({ "key": format!("{}|{rel}", m.rel()), "file": rel, "why": "is empty (0 bytes)" })));
                    continue;
                }
                let r = crate::dupes::sha256(&path, cancel, &mut |n| {
                    bytes_read += n;
                    if bytes_read - last >= 16 << 20 {
                        last = bytes_read;
                        progress(json!({ "step": "read", "bytes": bytes_read, "total_bytes": total, "name": rel }));
                    }
                });
                let sha = match r {
                    Ok(s) => s,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => bail!("Stopped."),
                    Err(e) => {
                        found.add("unreadable", with(brief(m), json!({ "key": format!("{}|{rel}", m.rel()), "file": rel, "why": format!("can't be read ({e})") })));
                        continue;
                    }
                };
                let known = &side["hashes"][rel.as_str()];
                let same_file = known["size"].as_u64() == Some(*size) && known["modified"].as_u64() == Some(*modified);
                match known["sha256"].as_str() {
                    Some(was) if same_file && was != sha => {
                        found.add("damaged", with(brief(m), json!({ "key": format!("{}|{rel}", m.rel()), "file": rel,
                            "size": size, "modified": modified, "sha256": sha })));
                        continue;
                    }
                    Some(_) if same_file => {}
                    _ => {
                        learnt.insert(rel.clone(), json!({ "size": size, "modified": modified, "sha256": sha }));
                    }
                }
                if let Some(why) = structure(&path, rel, *size) {
                    found.add("unreadable", with(brief(m), json!({ "key": format!("{}|{rel}", m.rel()), "file": rel, "why": why })));
                }
            }
            // keep what was learnt, so the next check can tell damage from a change
            if writable && !learnt.is_empty() && side["id"].is_string() {
                if !side["hashes"].is_object() {
                    side["hashes"] = json!({});
                }
                for (k, v) in learnt {
                    side["hashes"][k] = v;
                }
                if crate::config::write_json(&dir.join(SIDECAR), &side).is_ok() {
                    refresh.push(m.id().to_string());
                }
            }
        }
    }
    Ok(json!({
        "checked": crate::library::now(),
        "deep": deep,
        "models": ix.models.len(),
        "files": files_seen,
        "bytes_read": bytes_read,
        "groups": found.groups(),
        "refresh": refresh,
    }))
}

/// Leave out what's no longer true by the library as it's read now: models that
/// have gone, and models the issue no longer fits (one that got a model.json, a
/// cover back, files…).
pub fn current(saved: &Value, ix: &Index) -> Value {
    let mut out = saved.clone();
    let by_rel = |rel: &str| ix.models.iter().find(|m| m.rel() == rel);
    if let Some(groups) = out["groups"].as_array_mut() {
        for g in groups.iter_mut() {
            let kind = g["kind"].as_str().unwrap_or("").to_string();
            if matches!(kind.as_str(), "lists" | "leftovers" | "same_id") {
                continue;
            }
            let before = g["items"].as_array().map_or(0, Vec::len);
            let items: Vec<Value> = g["items"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|it| {
                    let Some(m) = by_rel(it["rel"].as_str().unwrap_or("")) else { return false };
                    match kind.as_str() {
                        "empty" => m.v["files"]["count"].as_u64() == Some(0),
                        "cover" => m.v["files"]["cover_missing"] == true,
                        "no_sidecar" => m.v["sidecar"] != true,
                        "broken_json" => m.v["sidecar"] != true,
                        _ => true,
                    }
                })
                .cloned()
                .collect();
            let dropped = before - items.len();
            g["count"] = json!(g["count"].as_u64().unwrap_or(0).saturating_sub(dropped as u64));
            g["items"] = json!(items);
        }
        groups.retain(|g| g["count"].as_u64().unwrap_or(0) > 0 && g["items"].as_array().is_some_and(|i| !i.is_empty()));
    }
    out
}

/// Take fixed items (by key) off a saved result.
pub fn without(saved: &mut Value, kind: &str, keys: &[String]) {
    if let Some(groups) = saved["groups"].as_array_mut() {
        for g in groups.iter_mut().filter(|g| g["kind"] == kind) {
            let before = g["items"].as_array().map_or(0, Vec::len);
            if let Some(items) = g["items"].as_array_mut() {
                items.retain(|it| !keys.iter().any(|k| it["key"].as_str() == Some(k)));
            }
            let gone = before - g["items"].as_array().map_or(0, Vec::len);
            g["count"] = json!(g["count"].as_u64().unwrap_or(0).saturating_sub(gone as u64));
        }
        groups.retain(|g| g["count"].as_u64().unwrap_or(0) > 0);
    }
}

/// What a fix did: keys fixed, ones that failed, model ids that changed (old,
/// new), and models to read again.
#[derive(Default)]
pub struct Fixed {
    pub fixed: Vec<String>,
    pub failed: Vec<Value>,
    pub renamed: Vec<(String, String)>,
    pub refresh: Vec<String>,
    /// For a journal (Undo): { id, name, rel, before, after } of each model changed.
    pub journal: Vec<Value>,
}

impl Fixed {
    fn fail(&mut self, key: &str, e: impl std::fmt::Display) {
        self.failed.push(json!({ "key": key, "error": e.to_string() }));
    }
}

/// A fresh name in `dir`: `base`, then `base-2`…
fn free_name(dir: &Path, base: &str) -> std::path::PathBuf {
    let mut p = dir.join(base);
    let mut n = 2;
    while p.exists() {
        p = dir.join(format!("{base}-{n}"));
        n += 1;
    }
    p
}

/// Make the fix for `kind` on the saved `items` chosen. `first`: what a model's
/// first model.json starts from.
pub fn fix(lib: &Library, ix: &Index, kind: &str, items: &[Value], first: &dyn Fn(&Model) -> Value) -> Result<Fixed> {
    lib.writable()?;
    let mut out = Fixed::default();
    let model_at = |rel: &str| ix.models.iter().find(|m| m.rel() == rel);
    for it in items {
        let key = it["key"].as_str().unwrap_or("").to_string();
        let rel = it["rel"].as_str().unwrap_or("");
        let r: Result<()> = (|| {
            match kind {
                "damaged" => {
                    // the file was changed on purpose: what it holds now is right
                    let dir = lib.resolve(rel)?;
                    let mut side = model::read_sidecar(&dir);
                    if !side["id"].is_string() {
                        bail!("Its model.json has gone.");
                    }
                    let file = it["file"].as_str().unwrap_or("");
                    side["hashes"][file] = json!({ "size": it["size"], "modified": it["modified"], "sha256": it["sha256"] });
                    crate::config::write_json(&dir.join(SIDECAR), &side)?;
                    out.refresh.push(side["id"].as_str().unwrap_or("").to_string());
                }
                "gone" => {
                    let dir = lib.resolve(rel)?;
                    let mut side = model::read_sidecar(&dir);
                    if let Some(h) = side["hashes"].as_object_mut() {
                        for f in it["files"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                            if crate::library::rel_inside(f).is_err() || !dir.join(f).is_file() {
                                h.shift_remove(f);
                            }
                        }
                    }
                    crate::config::write_json(&dir.join(SIDECAR), &side)?;
                    out.refresh.push(side["id"].as_str().unwrap_or("").to_string());
                }
                "broken_json" => {
                    let m = model_at(rel).ok_or_else(|| anyhow::anyhow!("That model isn't in the library any more."))?;
                    let dir = lib.resolve(rel)?;
                    model::keep_broken(&dir)?;
                    let side = model::update(&dir, &json!({}), &first(m))?;
                    let new = side["id"].as_str().unwrap_or("").to_string();
                    out.renamed.push((m.id().to_string(), new.clone()));
                    out.refresh.push(m.id().to_string());
                }
                "no_sidecar" => {
                    let m = model_at(rel).ok_or_else(|| anyhow::anyhow!("That model isn't in the library any more."))?;
                    let dir = lib.resolve(rel)?;
                    if dir.join(SIDECAR).is_file() && model::read_sidecar(&dir)["id"].is_string() {
                        return Ok(());
                    }
                    let side = model::update(&dir, &json!({}), &first(m))?;
                    out.renamed.push((m.id().to_string(), side["id"].as_str().unwrap_or("").to_string()));
                    out.refresh.push(m.id().to_string());
                }
                "cover" => {
                    let m = model_at(rel).ok_or_else(|| anyhow::anyhow!("That model isn't in the library any more."))?;
                    let dir = lib.resolve(rel)?;
                    let before = model::read_sidecar(&dir);
                    model::update(&dir, &json!({ "cover": null }), &first(m))?;
                    out.journal.push(json!({ "id": m.id(), "name": m.v["name"], "rel": rel, "before": before, "after": model::read_sidecar(&dir) }));
                    out.refresh.push(m.id().to_string());
                }
                "same_id" => {
                    // the one kept keeps the id (and its stars and lists); the
                    // others get their own
                    for c in it["models"].as_array().into_iter().flatten().filter(|c| c["keep"] != true) {
                        let crel = c["rel"].as_str().unwrap_or("");
                        let dir = lib.resolve(crel)?;
                        let before = model::read_sidecar(&dir);
                        if before["id"] != it["id"] {
                            continue;
                        }
                        let mut side = before.clone();
                        side["id"] = json!(model::new_id());
                        crate::config::write_json(&dir.join(SIDECAR), &side)?;
                        out.journal.push(json!({ "id": side["id"], "name": c["name"], "rel": crel, "before": before, "after": side }));
                    }
                    out.refresh.push(it["id"].as_str().unwrap_or("").to_string());
                }
                "lists" => {
                    let mid = it["model"].as_str().unwrap_or("");
                    if ix.get(mid).is_some() {
                        return Ok(()); // it's back
                    }
                    match it["coll"].as_str() {
                        Some(cid) => {
                            crate::collections::change(lib, cid, &[], &[mid.to_string()])?;
                        }
                        None => {
                            let mut favs: Vec<Value> = lib.favourites().as_array().cloned().unwrap_or_default();
                            favs.retain(|f| f.as_str() != Some(mid));
                            lib.set_favourites(&json!(favs))?;
                        }
                    }
                }
                "leftovers" => {
                    // never deleted here: put where they can be looked at
                    let from = lib.resolve(rel)?;
                    if !from.exists() {
                        return Ok(());
                    }
                    let name = from.file_name().map(|n| n.to_string_lossy().trim_start_matches('.').to_string()).unwrap_or_default();
                    let dest_dir = lib.root().join(UNSORTED).join(LEFTOVERS);
                    std::fs::create_dir_all(&dest_dir)?;
                    std::fs::rename(&from, free_name(&dest_dir, if name.is_empty() { "leftover" } else { &name }))?;
                }
                other => bail!("There's no fix for {other}."),
            }
            Ok(())
        })();
        match r {
            Ok(()) => out.fixed.push(key),
            Err(e) => out.fail(&key, format!("{e:#}")),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::Index;

    fn put(p: &Path, data: &[u8]) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, data).unwrap();
    }

    fn kinds(r: &Value) -> Vec<(String, u64)> {
        r["groups"].as_array().unwrap().iter().map(|g| (g["kind"].as_str().unwrap().to_string(), g["count"].as_u64().unwrap())).collect()
    }

    fn stl(tris: u32) -> Vec<u8> {
        let mut b = vec![0u8; 80];
        b.extend(tris.to_le_bytes());
        b.extend(vec![1u8; 50 * tris as usize]);
        b
    }

    #[test]
    fn names_windows_cant_use() {
        for bad in ["a:b", "what?", "end.", "end ", "CON", "nul.txt", "COM1.stl", "lpt9", "tab\there"] {
            assert!(bad_name(bad), "{bad}");
        }
        for ok in ["Hive Tyrant", "COM10", "console.stl", "a.b.c", "..", "LPT0"] {
            assert!(!bad_name(ok), "{ok}");
        }
        assert!(leftover_name(".importing-Tyrant", true));
        assert!(leftover_name(".model-transfer-j1-a.stl", false));
        assert!(leftover_name("model.tmp-123-ThreadId3", false));
        assert!(!leftover_name(".importing-Tyrant", false));
        assert!(!leftover_name("notes.tmp", false));
    }

    #[test]
    fn finds_problems_and_fixes_them() {
        let root = crate::library::tests::temp_dir("health");
        let lib = Library::open(&root).unwrap();
        let s = crate::schema::create(&lib, &json!({ "name": "Things" })).unwrap();
        let sid = s.id.clone();
        let things = root.join(&s.folder);
        // a good model, read deep once so its hashes are known
        put(&things.join("Good/a.stl"), &stl(3));
        model::update(&things.join("Good"), &json!({ "name": "Good" }), &json!({})).unwrap();
        model::set_place(&things.join("Good"), Some(&sid), &[]).unwrap();
        // no model.json, in a category
        put(&things.join("Plain/b.stl"), &stl(2));
        // broken model.json
        put(&things.join("Broken/c.stl"), &stl(1));
        put(&things.join("Broken/model.json"), b"{ not json");
        // empty model, cover missing
        std::fs::create_dir_all(things.join("Empty")).unwrap();
        model::update(&things.join("Empty"), &json!({ "name": "Empty", "cover": "gone.png" }), &json!({})).unwrap();
        model::set_place(&things.join("Empty"), Some(&sid), &[]).unwrap();
        // a copy made by hand (same id)
        put(&things.join("Copy/a.stl"), &stl(3));
        std::fs::copy(things.join("Good/model.json"), things.join("Copy/model.json")).unwrap();
        // a leftover, and a cut-short STL in Unsorted
        put(&root.join("Unsorted/.importing-Thing/x.stl"), b"partial");
        put(&root.join("Unsorted/Short/s.stl"), &stl(4)[..150]);
        // a star for a model that's gone
        lib.set_favourites(&json!(["mgone"])).unwrap();

        let ix = Index::build(&lib, None, true);
        let no = AtomicBool::new(false);
        let r = check(&lib, &ix, false, &no, &|_| {}).unwrap();
        assert_eq!(
            kinds(&r),
            [("broken_json".into(), 1), ("same_id".into(), 1), ("empty".into(), 1), ("cover".into(), 1),
             ("lists".into(), 1), ("no_sidecar".into(), 1), ("leftovers".into(), 1)]
        );
        // deep: the cut-short STL, and hashes recorded for next time
        let r = check(&lib, &ix, true, &no, &|_| {}).unwrap();
        let unreadable = r["groups"].as_array().unwrap().iter().find(|g| g["kind"] == "unreadable").unwrap();
        assert_eq!(unreadable["items"][0]["file"], "s.stl");
        assert!(model::read_sidecar(&things.join("Good"))["hashes"]["a.stl"]["sha256"].is_string());
        // a file damaged in place (same size and time, different contents)
        let good = things.join("Good/a.stl");
        let t = std::fs::metadata(&good).unwrap().modified().unwrap();
        let mut bytes = stl(3);
        bytes[100] = 9;
        std::fs::write(&good, &bytes).unwrap();
        std::fs::File::options().write(true).open(&good).unwrap().set_modified(t).unwrap();
        let ix = Index::build(&lib, None, true);
        let r = check(&lib, &ix, true, &no, &|_| {}).unwrap();
        let damaged = r["groups"].as_array().unwrap().iter().find(|g| g["kind"] == "damaged").unwrap().clone();
        assert_eq!(damaged["items"].as_array().unwrap().len(), 1);
        let item = &damaged["items"][0];
        assert!(item["rel"].as_str().unwrap().ends_with("Good"));

        // fixes
        let first = |m: &Model| crate::api::first_sidecar(m);
        let group = |kind: &str| r["groups"].as_array().unwrap().iter().find(|g| g["kind"] == kind).unwrap()["items"].as_array().unwrap().clone();
        let f = fix(&lib, &ix, "damaged", &group("damaged"), &first).unwrap();
        assert_eq!(f.fixed.len(), 1);
        let f = fix(&lib, &ix, "broken_json", &group("broken_json"), &first).unwrap();
        assert_eq!(f.renamed.len(), 1);
        assert!(things.join("Broken/model.json.broken").is_file());
        assert!(model::read_sidecar(&things.join("Broken"))["id"].is_string());
        let f = fix(&lib, &ix, "same_id", &group("same_id"), &first).unwrap();
        assert_eq!(f.journal.len(), 1);
        assert_ne!(model::read_sidecar(&things.join("Good"))["id"], model::read_sidecar(&things.join("Copy"))["id"]);
        fix(&lib, &ix, "cover", &group("cover"), &first).unwrap();
        assert!(model::read_sidecar(&things.join("Empty"))["cover"].is_null());
        fix(&lib, &ix, "lists", &group("lists"), &first).unwrap();
        assert_eq!(lib.favourites(), json!([]));
        let f = fix(&lib, &ix, "no_sidecar", &group("no_sidecar"), &first).unwrap();
        assert_eq!(f.renamed.len(), 1);
        fix(&lib, &ix, "leftovers", &group("leftovers"), &first).unwrap();
        assert!(root.join("Unsorted").join(LEFTOVERS).join("importing-Thing/x.stl").is_file());

        let ix = Index::build(&lib, None, true);
        let r = check(&lib, &ix, true, &no, &|_| {}).unwrap();
        assert_eq!(kinds(&r), [("unreadable".into(), 2), ("empty".into(), 1)], "{r:#}");
        let mut saved = r.clone();
        let k = group_key(&saved, "empty");
        without(&mut saved, "empty", &[k]);
        assert_eq!(kinds(&saved), [("unreadable".into(), 2)]);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn group_key(r: &Value, kind: &str) -> String {
        r["groups"].as_array().unwrap().iter().find(|g| g["kind"] == kind).unwrap()["items"][0]["key"].as_str().unwrap().to_string()
    }
}
