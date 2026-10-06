//! Duplicates (docs/PLAN.md, "Phase 5 design", 3): models whose 3D, slicer and
//! archive files are all the same as another model's, and files found in more
//! than one model. Files of one size are compared by a quick fingerprint (the size
//! and the first and last 64 KB), then those that still match by SHA-256. Hashes
//! are kept in each model's model.json (`hashes`), so a file is read once until it
//! changes. An extra copy is set aside in `_library/set-aside/`, journalled like a
//! re-layout so Home can undo it.

use crate::import::{is_main, tree_bytes};
use crate::index::{Index, Model};
use crate::library::{Library, APP_DIR};
use crate::model::{self, SIDECAR};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;

/// Where extra copies go, out of the library's lists, until they're deleted.
pub const SET_ASIDE: &str = "set-aside";
/// The quick fingerprint reads this much from each end of a file.
const EDGE: u64 = 64 * 1024;
/// Files listed as being in more than one model (the biggest first).
const SHARED_MAX: usize = 300;

pub fn set_aside_dir(lib: &Library) -> PathBuf {
    lib.root().join(APP_DIR).join(SET_ASIDE)
}

/// What's known about one file: as kept in model.json under `hashes`.
#[derive(Clone, Default, PartialEq, Debug)]
struct Known {
    size: u64,
    modified: u64,
    quick: Option<String>,
    sha256: Option<String>,
}

impl Known {
    fn from(v: &Value) -> Option<Known> {
        Some(Known {
            size: v["size"].as_u64()?,
            modified: v["modified"].as_u64()?,
            quick: v["quick"].as_str().map(String::from),
            sha256: v["sha256"].as_str().map(String::from),
        })
    }
    fn to_json(&self) -> Value {
        let mut v = json!({ "size": self.size, "modified": self.modified });
        if let Some(q) = &self.quick {
            v["quick"] = json!(q);
        }
        if let Some(s) = &self.sha256 {
            v["sha256"] = json!(s);
        }
        v
    }
}

/// The quick fingerprint of a file of `size` bytes. One of 128 KB or less is read
/// whole, and then the fingerprint is its SHA-256 (the bool says so).
pub fn quick(path: &Path, size: u64) -> std::io::Result<(String, bool)> {
    let mut f = std::fs::File::open(path)?;
    if size <= 2 * EDGE {
        let mut buf = Vec::with_capacity(size as usize);
        f.read_to_end(&mut buf)?;
        return Ok((hex::encode(Sha256::digest(&buf)), true));
    }
    let mut h = Sha256::new();
    h.update(size.to_le_bytes());
    let mut buf = vec![0u8; EDGE as usize];
    f.read_exact(&mut buf)?;
    h.update(&buf);
    f.seek(SeekFrom::Start(size - EDGE))?;
    f.read_exact(&mut buf)?;
    h.update(&buf);
    Ok((hex::encode(h.finalize()), false))
}

/// A file's SHA-256, read in chunks; stops when `cancel` is set.
fn sha256(
    path: &Path,
    cancel: &AtomicBool,
    on_bytes: &mut dyn FnMut(u64),
) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "Stopped.",
            ));
        }
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        on_bytes(n as u64);
    }
    Ok(hex::encode(h.finalize()))
}

/// Do two sets of files (size, where) hold the same contents, by size and quick
/// fingerprint? Import's duplicate warning, once the sizes already match.
pub fn same_prints(a: &[(u64, PathBuf)], b: &[(u64, PathBuf)]) -> bool {
    let prints = |files: &[(u64, PathBuf)]| -> Option<Vec<(u64, String)>> {
        let mut v = files
            .iter()
            .map(|(s, p)| quick(p, *s).ok().map(|q| (*s, q.0)))
            .collect::<Option<Vec<_>>>()?;
        v.sort();
        Some(v)
    };
    a.len() == b.len() && matches!((prints(a), prints(b)), (Some(x), Some(y)) if x == y)
}

/// The files of a model folder that duplicates are judged by: 3D, slicer and
/// archive files (`_media/` aside), as (path in the folder, size, modified in ms).
fn main_files(dir: &Path) -> Vec<(String, u64, u64)> {
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<(String, u64, u64)>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.')
                || (prefix.is_empty() && (name == "_thumbs" || name == "_media"))
            {
                continue;
            }
            let rel = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            match e.file_type() {
                Ok(t) if t.is_dir() => walk(&e.path(), &rel, out),
                Ok(t) if t.is_file() && is_main(&name) => {
                    if let Ok(m) = e.metadata() {
                        let modified = m
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                            .map_or(0, |d| d.as_millis() as u64);
                        out.push((rel, m.len(), modified));
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = vec![];
    walk(dir, "", &mut out);
    out.sort();
    out
}

/// One file of one model, as the search goes.
struct File {
    model: usize,
    rel: String,
    k: Known,
    unreadable: bool,
}

fn distinct(files: &[File], group: &[usize]) -> usize {
    group
        .iter()
        .map(|&i| files[i].model)
        .collect::<HashSet<_>>()
        .len()
}

fn cache_key(id: &str, rel: &str) -> String {
    format!("{id}\n{rel}")
}

/// Which copy to keep: one in a category over one in Unsorted, then the one with
/// more files, then one with a model.json, then the shortest folder path.
fn keep_order(a: &Model, b: &Model) -> std::cmp::Ordering {
    let key = |m: &Model| {
        (
            m.v["schema"].is_null(),
            std::cmp::Reverse(m.v["files"]["count"].as_u64().unwrap_or(0)),
            m.v["sidecar"] != true,
            m.rel().len(),
            m.rel().to_string(),
        )
    };
    key(a).cmp(&key(b))
}

fn brief(m: &Model) -> Value {
    json!({
        "id": m.id(), "rel": m.rel(), "name": m.v["name"], "schema": m.v["schema"], "path": m.v["path"],
        "count": m.v["files"]["count"], "bytes": m.v["files"]["bytes"], "sidecar": m.v["sidecar"],
    })
}

/// Look for duplicates among `models`. `cache` (in the app's data folder) keeps the
/// hashes of models without a model.json, or of a library that can't be written;
/// the others' go into their model.json. `progress` gets {step: "list", item,
/// items} while the folders are read, then {step: "compare" | "hash", bytes,
/// total_bytes, name} while files are read.
pub fn find(
    lib: &Library,
    models: &[Model],
    cache: &Path,
    cancel: &AtomicBool,
    progress: &dyn Fn(Value),
) -> Result<Value> {
    let stopped = || -> Result<()> {
        if cancel.load(Ordering::Relaxed) {
            bail!("Stopped.");
        }
        Ok(())
    };
    let stored = crate::config::read_json_object(cache);
    let writable = lib.writable().is_ok();
    let dirs: Vec<Option<PathBuf>> = models.iter().map(|m| lib.resolve(m.rel()).ok()).collect();
    let mut files: Vec<File> = vec![];
    for (mi, m) in models.iter().enumerate() {
        stopped()?;
        if mi % 25 == 0 {
            progress(
                json!({ "step": "list", "item": mi, "items": models.len(), "name": m.v["name"] }),
            );
        }
        let Some(dir) = &dirs[mi] else { continue };
        let side = if m.v["sidecar"] == true {
            model::read_sidecar(dir)
        } else {
            Value::Null
        };
        for (rel, size, modified) in main_files(dir) {
            let k = Known::from(&side["hashes"][rel.as_str()])
                .or_else(|| Known::from(&stored["files"][cache_key(m.id(), &rel)]))
                .filter(|k| k.size == size && k.modified == modified)
                .unwrap_or(Known {
                    size,
                    modified,
                    ..Default::default()
                });
            files.push(File {
                model: mi,
                rel,
                k,
                unreadable: false,
            });
        }
    }
    let path_of = |f: &File| dirs[f.model].as_ref().map(|d| d.join(&f.rel));
    let mut changed: HashSet<usize> = HashSet::new();

    // files of one size in two or more models: their quick fingerprints
    let mut by_size: HashMap<u64, Vec<usize>> = HashMap::new();
    for (i, f) in files.iter().enumerate() {
        if f.k.size > 0 {
            by_size.entry(f.k.size).or_default().push(i);
        }
    }
    let same_size: Vec<Vec<usize>> = by_size
        .into_values()
        .filter(|g| distinct(&files, g) >= 2)
        .collect();
    let todo: Vec<usize> = same_size
        .iter()
        .flatten()
        .copied()
        .filter(|&i| files[i].k.quick.is_none())
        .collect();
    let total: u64 = todo.iter().map(|&i| files[i].k.size.min(2 * EDGE)).sum();
    let mut done = 0;
    for i in todo {
        stopped()?;
        progress(
            json!({ "step": "compare", "bytes": done, "total_bytes": total, "name": files[i].rel }),
        );
        done += files[i].k.size.min(2 * EDGE);
        let Some(p) = path_of(&files[i]) else {
            continue;
        };
        match quick(&p, files[i].k.size) {
            Ok((q, whole)) => {
                let f = &mut files[i];
                if whole {
                    f.k.sha256 = Some(q.clone());
                }
                f.k.quick = Some(q);
                changed.insert(f.model);
            }
            Err(_) => files[i].unreadable = true,
        }
    }

    // the same fingerprint in two or more models: their SHA-256
    let mut by_quick: HashMap<(u64, String), Vec<usize>> = HashMap::new();
    for &i in same_size.iter().flatten() {
        if let (Some(q), false) = (&files[i].k.quick, files[i].unreadable) {
            by_quick
                .entry((files[i].k.size, q.clone()))
                .or_default()
                .push(i);
        }
    }
    let same_quick: Vec<Vec<usize>> = by_quick
        .into_values()
        .filter(|g| distinct(&files, g) >= 2)
        .collect();
    let todo: Vec<usize> = same_quick
        .iter()
        .flatten()
        .copied()
        .filter(|&i| files[i].k.sha256.is_none())
        .collect();
    let total: u64 = todo.iter().map(|&i| files[i].k.size).sum();
    let mut done = 0;
    for i in todo {
        stopped()?;
        let Some(p) = path_of(&files[i]) else {
            continue;
        };
        let name = files[i].rel.clone();
        let mut last = 0;
        let r = sha256(&p, cancel, &mut |n| {
            done += n;
            if done - last >= 16 << 20 {
                last = done;
                progress(
                    json!({ "step": "hash", "bytes": done, "total_bytes": total, "name": name }),
                );
            }
        });
        match r {
            Ok(s) => {
                files[i].k.sha256 = Some(s);
                changed.insert(files[i].model);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => bail!("Stopped."),
            Err(_) => files[i].unreadable = true,
        }
    }

    // the same contents in two or more models
    let mut by_sha: HashMap<&str, Vec<usize>> = HashMap::new();
    for &i in same_quick.iter().flatten() {
        if let (Some(s), false) = (&files[i].k.sha256, files[i].unreadable) {
            by_sha.entry(s.as_str()).or_default().push(i);
        }
    }
    by_sha.retain(|_, g| distinct(&files, g) >= 2);

    // models whose files are all found in another model, by the list of their contents
    let mut sigs: Vec<Option<Vec<&str>>> = vec![Some(vec![]); models.len()];
    let mut has_files = vec![false; models.len()];
    for f in &files {
        has_files[f.model] = true;
        let shared =
            f.k.sha256
                .as_deref()
                .filter(|s| !f.unreadable && by_sha.contains_key(s));
        let sig = &mut sigs[f.model];
        match (shared, sig.as_mut()) {
            (Some(s), Some(v)) => v.push(s),
            _ => *sig = None,
        }
    }
    let mut by_sig: HashMap<Vec<&str>, Vec<usize>> = HashMap::new();
    for (mi, sig) in sigs.into_iter().enumerate() {
        if let (Some(mut s), true) = (sig, has_files[mi]) {
            s.sort_unstable();
            by_sig.entry(s).or_default().push(mi);
        }
    }
    let mut group_of: HashMap<usize, usize> = HashMap::new();
    let mut groups: Vec<(u64, Vec<usize>)> = vec![];
    for (_, mut ms) in by_sig.into_iter().filter(|(_, ms)| ms.len() >= 2) {
        ms.sort_by(|&a, &b| keep_order(&models[a], &models[b]));
        let bytes: u64 = files
            .iter()
            .filter(|f| f.model == ms[0])
            .map(|f| f.k.size)
            .sum();
        for &m in &ms {
            group_of.insert(m, groups.len());
        }
        groups.push((bytes, ms));
    }
    groups.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| models[a.1[0]].rel().cmp(models[b.1[0]].rel()))
    });
    let mut wasted: u64 = 0;
    let groups: Vec<Value> = groups
        .iter()
        .map(|(bytes, ms)| {
            wasted += ms[1..]
                .iter()
                .map(|&m| models[m].v["files"]["bytes"].as_u64().unwrap_or(0))
                .sum::<u64>();
            json!({
                "bytes": bytes,
                "files": files.iter().filter(|f| f.model == ms[0]).count(),
                "keep": models[ms[0]].id(),
                "models": ms.iter().map(|&m| brief(&models[m])).collect::<Vec<_>>(),
            })
        })
        .collect();

    // files in more than one model that aren't copies of one whole model
    let mut shared: Vec<(u64, Value)> = vec![];
    for (sha, g) in &by_sha {
        let ms: HashSet<usize> = g.iter().map(|&i| files[i].model).collect();
        let first = group_of.get(ms.iter().next().unwrap());
        if first.is_some() && ms.iter().all(|m| group_of.get(m) == first) {
            continue;
        }
        let size = files[g[0]].k.size;
        wasted += size * (ms.len() as u64 - 1);
        let mut seen = HashSet::new();
        let mut inside: Vec<Value> = g
            .iter()
            .filter(|&&i| seen.insert(files[i].model))
            .map(|&i| {
                let m = &models[files[i].model];
                json!({ "id": m.id(), "name": m.v["name"], "rel": m.rel(), "file": files[i].rel })
            })
            .collect();
        inside.sort_by(|a, b| a["rel"].as_str().cmp(&b["rel"].as_str()));
        let name = files[g[0]].rel.rsplit('/').next().unwrap_or("").to_string();
        shared.push((
            size,
            json!({ "sha256": sha, "size": size, "name": name, "in": inside }),
        ));
    }
    shared.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1["name"].as_str().cmp(&b.1["name"].as_str()))
    });
    let shared_count = shared.len();

    // keep what was learnt: in model.json, else in the cache
    let mut kept = Map::new();
    for (mi, m) in models.iter().enumerate() {
        let entries: Map<String, Value> = files
            .iter()
            .filter(|f| f.model == mi && f.k.quick.is_some())
            .map(|f| (f.rel.clone(), f.k.to_json()))
            .collect();
        if entries.is_empty() {
            continue;
        }
        let in_sidecar = writable && m.v["sidecar"] == true;
        if in_sidecar && changed.contains(&mi) {
            if let Some(dir) = &dirs[mi] {
                let mut side = model::read_sidecar(dir);
                if side["hashes"] != Value::Object(entries.clone()) && side["id"].is_string() {
                    side["hashes"] = Value::Object(entries.clone());
                    if crate::config::write_json(&dir.join(SIDECAR), &side).is_ok() {
                        continue;
                    }
                }
            }
        }
        if !in_sidecar {
            for (rel, v) in entries {
                kept.insert(cache_key(m.id(), &rel), v);
            }
        }
    }
    if let Some(dir) = cache.parent() {
        std::fs::create_dir_all(dir)?;
    }
    crate::config::write_json(cache, &json!({ "files": kept }))?;

    let unreadable: Vec<Value> = files
        .iter()
        .filter(|f| f.unreadable)
        .take(50)
        .map(|f| json!({ "id": models[f.model].id(), "rel": models[f.model].rel(), "file": f.rel }))
        .collect();
    Ok(json!({
        "found": crate::library::now(),
        "models": models.len(),
        "files": files.len(),
        "groups": groups,
        "shared": shared.into_iter().take(SHARED_MAX).map(|s| s.1).collect::<Vec<_>>(),
        "shared_count": shared_count,
        "wasted": wasted,
        "unreadable": unreadable,
        "refresh": changed.iter().map(|&mi| models[mi].id()).collect::<Vec<_>>(),
    }))
}

/// Model folders set aside and their size.
pub fn set_aside_stats(lib: &Library) -> Value {
    fn walk(dir: &Path, depth: usize, count: &mut u64, bytes: &mut u64) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        {
            if e.path().join(SIDECAR).is_file() || depth > 40 {
                *count += 1;
                *bytes += tree_bytes(&e.path());
            } else {
                walk(&e.path(), depth + 1, count, bytes);
            }
        }
    }
    let (mut count, mut bytes) = (0, 0);
    walk(&set_aside_dir(lib), 0, &mut count, &mut bytes);
    json!({ "count": count, "bytes": bytes, "rel": format!("{APP_DIR}/{SET_ASIDE}") })
}

/// A saved search as the page shows it: models no longer in the library dropped
/// (set aside, deleted, moved by hand and not found), the rest as they are now.
pub fn view(lib: &Library, ix: &Index, saved: &Value) -> Value {
    let now = |m: &Value| -> Option<Value> {
        let cur = ix.get(m["id"].as_str()?)?;
        Some(brief(cur))
    };
    let groups: Vec<Value> = saved["groups"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| {
            let ms: Vec<Value> = g["models"].as_array()?.iter().filter_map(now).collect();
            if ms.len() < 2 {
                return None;
            }
            let mut g = g.clone();
            if !ms.iter().any(|m| m["id"] == g["keep"]) {
                g["keep"] = ms[0]["id"].clone();
            }
            g["models"] = json!(ms);
            Some(g)
        })
        .collect();
    let shared: Vec<Value> = saved["shared"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let inside: Vec<Value> = s["in"]
                .as_array()?
                .iter()
                .filter_map(|x| {
                    let cur = ix.get(x["id"].as_str()?)?;
                    Some(json!({ "id": cur.id(), "name": cur.v["name"], "rel": cur.rel(), "file": x["file"] }))
                })
                .collect();
            (inside.len() >= 2).then(|| {
                let mut s = s.clone();
                s["in"] = json!(inside);
                s
            })
        })
        .collect();
    // what could be saved, as things are now
    let wasted: u64 = groups
        .iter()
        .flat_map(|g| {
            let keep = g["keep"].clone();
            g["models"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(move |m| m["id"] != keep)
                .map(|m| m["bytes"].as_u64().unwrap_or(0))
        })
        .sum::<u64>()
        + shared
            .iter()
            .map(|s| {
                s["size"].as_u64().unwrap_or(0)
                    * (s["in"].as_array().map_or(1, Vec::len) as u64 - 1)
            })
            .sum::<u64>();
    let listed = saved["shared"].as_array().map_or(0, Vec::len);
    let shared_count = (saved["shared_count"].as_u64().unwrap_or(0) as usize)
        .saturating_sub(listed.saturating_sub(shared.len()));
    json!({
        "found": saved["found"], "models": saved["models"], "files": saved["files"],
        "groups": groups, "shared": shared, "shared_count": shared_count,
        "wasted": wasted, "unreadable": saved["unreadable"],
        "set_aside": set_aside_stats(lib),
    })
}

/// The journal for setting `ids` aside: each folder moves to
/// `_library/set-aside/<where it was>`, keeping its category in model.json.
pub fn set_aside_plan(lib: &Library, ix: &Index, ids: &[String]) -> Result<Value> {
    if ids.is_empty() {
        bail!("Choose the copies to set aside.");
    }
    let mut taken: HashSet<String> = HashSet::new();
    let mut moves = vec![];
    for id in ids {
        let m = ix.get(id).ok_or_else(|| {
            anyhow!("A copy isn't in the library any more. Look for duplicates again.")
        })?;
        let base = format!("{APP_DIR}/{SET_ASIDE}/{}", m.rel());
        let mut to = base.clone();
        let mut n = 2;
        while taken.contains(&to) || lib.resolve(&to)?.exists() {
            to = format!("{base} ({n})");
            n += 1;
        }
        taken.insert(to.clone());
        moves.push(json!({
            "id": m.id(), "name": m.v["name"], "authors": m.v["authors"],
            "from": m.rel(), "to": to, "keep_id": true,
            "schema_before": m.v["schema"], "path_before": m.v["path"],
            "schema_after": m.v["schema"], "path_after": m.v["path"],
        }));
    }
    let n = moves.len();
    Ok(json!({
        "kind": "set-aside",
        "label": format!("Set aside {n} duplicate {}", if n == 1 { "copy" } else { "copies" }),
        "schema": null, "schema_before": null, "schema_after": null,
        "moves": moves,
    }))
}

/// Delete everything set aside. Setting aside can't be undone after this.
pub fn empty_set_aside(lib: &Library) -> Result<Value> {
    lib.writable()?;
    let stats = set_aside_stats(lib);
    let dir = set_aside_dir(lib);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    crate::relayout::mark_emptied(lib)?;
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::tests::temp_dir;
    use crate::relayout;
    use crate::schema;

    fn put(p: &Path, data: &[u8]) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, data).unwrap();
    }

    fn big(seed: u8, n: usize) -> Vec<u8> {
        (0..n)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
            .collect()
    }

    #[test]
    fn finds_copies_sets_them_aside_and_undoes() {
        let root = temp_dir("dupes");
        let lib = Library::open(&root).unwrap();
        schema::create(
            &lib,
            &json!({ "name": "Home items", "subcategories": [{ "name": "Kitchen" }] }),
        )
        .unwrap();
        let body = big(1, 300_000);
        // the same model three times: in a category (with a picture), in Unsorted twice
        put(&root.join("Home items/Kitchen/Jug/jug.stl"), &body);
        put(&root.join("Home items/Kitchen/Jug/jug.png"), b"png");
        put(&root.join("Unsorted/Jug copy/jug.stl"), &body);
        put(
            &root.join("Unsorted/Jug again/Parts/jug-renamed.stl"),
            &body,
        );
        // same size and the same start and end, different in the middle
        let mut other = body.clone();
        other[150_000] ^= 0xff;
        put(&root.join("Unsorted/Not a jug/jug.stl"), &other);
        // a file in two models that aren't copies of each other
        put(&root.join("Unsorted/Tray/tray.stl"), b"solid tray");
        put(&root.join("Unsorted/Tray/handle.stl"), b"solid handle");
        put(&root.join("Unsorted/Tray set/tray.stl"), b"solid tray");
        put(&root.join("Unsorted/Tray set/lid.stl"), b"solid lid");
        // one copy has a model.json: its hashes go there
        model::update(
            &root.join("Unsorted/Jug copy"),
            &json!({}),
            &json!({ "name": "Jug copy" }),
        )
        .unwrap();
        let ix = Index::build(&lib, None, true);
        let cache = root.join("../dupes-cache.json");
        let r = find(&lib, &ix.models, &cache, &AtomicBool::new(false), &|_| {}).unwrap();
        let groups = r["groups"].as_array().unwrap();
        assert_eq!(groups.len(), 1, "{r:#}");
        let rels: Vec<&str> = groups[0]["models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["rel"].as_str().unwrap())
            .collect();
        assert_eq!(
            rels,
            [
                "Home items/Kitchen/Jug",
                "Unsorted/Jug copy",
                "Unsorted/Jug again"
            ]
        );
        assert_eq!(
            groups[0]["keep"],
            ix.models
                .iter()
                .find(|m| m.rel() == "Home items/Kitchen/Jug")
                .unwrap()
                .id()
        );
        let shared = r["shared"].as_array().unwrap();
        assert_eq!(shared.len(), 1);
        assert_eq!(shared[0]["name"], "tray.stl");
        assert_eq!(r["wasted"], json!(2 * 300_000 + 10));
        // hashes were kept: in model.json, and the cache for the others
        let side = model::read_sidecar(&root.join("Unsorted/Jug copy"));
        assert_eq!(side["hashes"]["jug.stl"]["size"], 300_000);
        assert!(side["hashes"]["jug.stl"]["sha256"].is_string());
        let c = crate::config::read_json_object(&cache);
        assert!(c["files"]
            .as_object()
            .unwrap()
            .keys()
            .any(|k| k.ends_with("\nParts/jug-renamed.stl")));
        // a second search reads nothing again
        let ix = Index::build(&lib, None, true);
        let r2 = find(&lib, &ix.models, &cache, &AtomicBool::new(false), &|v| {
            assert_ne!(v["step"], "hash", "{v}");
        })
        .unwrap();
        assert_eq!(r2["groups"], r["groups"]);
        assert!(r2["refresh"].as_array().unwrap().is_empty());

        // set the two extra copies aside, then undo
        let extra: Vec<String> = groups[0]["models"].as_array().unwrap()[1..]
            .iter()
            .map(|m| m["id"].as_str().unwrap().to_string())
            .collect();
        let plan = set_aside_plan(&lib, &ix, &extra).unwrap();
        let id = relayout::start(&lib, &plan).unwrap();
        let done = relayout::apply(&lib, &id, &AtomicBool::new(false), &|_, _, _| {}).unwrap();
        assert_eq!(done["state"], "done", "{done}");
        assert!(root
            .join("_library/set-aside/Unsorted/Jug copy/jug.stl")
            .is_file());
        assert!(root
            .join("_library/set-aside/Unsorted/Jug again/Parts/jug-renamed.stl")
            .is_file());
        assert!(!root.join("Unsorted/Jug copy").exists());
        assert!(schema::list(&lib).len() == 1, "categories are left alone");
        let ix = Index::build(&lib, None, true);
        assert_eq!(ix.models.len(), 4, "set-aside copies aren't listed");
        let v = view(&lib, &ix, &r);
        assert!(v["groups"].as_array().unwrap().is_empty());
        assert_eq!(v["wasted"], 10, "only the shared tray is left");
        assert_eq!(v["set_aside"]["count"], 2);
        relayout::undo(&lib, &id, &AtomicBool::new(false), &|_, _, _| {}).unwrap();
        assert!(root.join("Unsorted/Jug copy/jug.stl").is_file());
        assert!(!set_aside_dir(&lib).exists());
        let ix = Index::build(&lib, None, true);
        assert_eq!(ix.models.len(), 6);

        // set aside again, then delete the copies: that can't be undone
        let ix_ids: Vec<String> = ix
            .models
            .iter()
            .filter(|m| m.rel() == "Unsorted/Jug copy")
            .map(|m| m.id().to_string())
            .collect();
        let plan = set_aside_plan(&lib, &ix, &ix_ids).unwrap();
        let id = relayout::start(&lib, &plan).unwrap();
        relayout::apply(&lib, &id, &AtomicBool::new(false), &|_, _, _| {}).unwrap();
        let gone = empty_set_aside(&lib).unwrap();
        assert_eq!(gone["count"], 1);
        assert!(!set_aside_dir(&lib).exists());
        assert_eq!(relayout::read(&lib, &id).unwrap()["state"], "emptied");
        assert!(relayout::undo(&lib, &id, &AtomicBool::new(false), &|_, _, _| {}).is_err());
    }

    #[test]
    fn same_prints_compares_contents() {
        let d = temp_dir("prints");
        let (a, b, c) = (d.join("a.stl"), d.join("b.stl"), d.join("c.stl"));
        put(&a, &big(3, 200_000));
        put(&b, &big(3, 200_000));
        let mut x = big(3, 200_000);
        x[0] ^= 1;
        put(&c, &x);
        assert!(same_prints(&[(200_000, a.clone())], &[(200_000, b)]));
        assert!(!same_prints(&[(200_000, a)], &[(200_000, c)]));
    }
}
