//! Getting models into the library, and moving them between categories
//! (docs/PLAN.md, "Phase 2 design"): proposing models from a messy folder or
//! dropped items (`scan`), working out where each one goes (`plan`), and moving
//! or copying them there with every copied file checked (`commit_one`).

use crate::index::{fold, Index, UNSORTED};
use crate::library::{Library, APP_DIR};
use crate::model::{self, file_kind, SIDECAR};
use crate::schema::{self, clean_folder_name, parse_folder_name, Schema};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// The template for Unsorted models and schemas without one.
const DEFAULT_TEMPLATE: &str = "{name} ({author})";

/// Kinds that make a file a model on its own when it's loose in a folder.
fn is_main(name: &str) -> bool {
    matches!(file_kind(name), "model" | "slicer" | "archive")
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn stem(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(name)
        .to_lowercase()
}

/// "hive_tyrant_v2" -> "hive tyrant v2" (only when the name has no spaces).
fn tidy(name: &str) -> String {
    let n = if name.contains(' ') {
        name.to_string()
    } else {
        name.replace(['_'], " ")
    };
    n.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Name and author from a folder or file name.
fn guess_name(raw: &str) -> (String, Option<String>) {
    let (n, a) = parse_folder_name(DEFAULT_TEMPLATE, raw.trim());
    (tidy(&n), a)
}

/// A proposed model: a folder, or loose files that belong together.
#[derive(Debug, Clone)]
struct Found {
    path: PathBuf,
    /// Loose files (empty for a folder).
    files: Vec<PathBuf>,
}

/// What a folder's contents propose: its sub-folders, and its loose files grouped
/// by name (pictures and documents go with a model file their name starts with).
fn propose(dir: &Path) -> Result<(Vec<Found>, Vec<String>)> {
    let mut dirs = vec![];
    let mut files = vec![];
    for e in std::fs::read_dir(dir)
        .with_context(|| format!("can't read {}", dir.display()))?
        .flatten()
    {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        match e.file_type() {
            Ok(t) if t.is_dir() => dirs.push(e.path()),
            Ok(_) => files.push(e.path()),
            Err(_) => {}
        }
    }
    dirs.sort();
    files.sort();
    let mut out: Vec<Found> = dirs
        .into_iter()
        .map(|d| Found {
            path: d,
            files: vec![],
        })
        .collect();
    let mut groups: Vec<(String, Vec<PathBuf>)> = vec![];
    for f in files.iter().filter(|f| is_main(&file_name(f))) {
        let s = stem(&file_name(f));
        match groups.iter_mut().find(|g| g.0 == s) {
            Some(g) => g.1.push(f.clone()),
            None => groups.push((s, vec![f.clone()])),
        }
    }
    let mut left = vec![];
    for f in files.iter().filter(|f| !is_main(&file_name(f))) {
        let s = stem(&file_name(f));
        // the longest model name this file's name starts with
        match groups
            .iter_mut()
            .filter(|g| s.starts_with(&g.0))
            .max_by_key(|g| g.0.len())
        {
            Some(g) => g.1.push(f.clone()),
            None => left.push(file_name(f)),
        }
    }
    for (_, fs) in groups {
        out.push(Found {
            path: fs[0].clone(),
            files: fs,
        });
    }
    Ok((out, left))
}

/// Every model value a schema already has at each depth, with the path above it.
fn known_paths(ix: &Index, schema: &str) -> Vec<Vec<String>> {
    let mut seen = HashSet::new();
    ix.models
        .iter()
        .filter(|m| m.v["schema"] == json!(schema))
        .filter_map(|m| serde_json::from_value::<Vec<String>>(m.v["path"].clone()).ok())
        .chain(ix.schema(schema).map(Schema::tree).unwrap_or_default())
        .filter(|p| seen.insert(p.clone()))
        .collect()
}

/// The schema and category values the folder names on `path` suggest.
fn guess_category(ix: &Index, path: &Path, name: &str) -> Value {
    let mut segs: HashSet<String> = path
        .components()
        .rev()
        .take(8)
        .map(|c| fold(&c.as_os_str().to_string_lossy()))
        .collect();
    segs.insert(fold(name));
    let mut best: Option<(usize, &Schema, Vec<String>)> = None;
    for s in &ix.schemas {
        if (segs.contains(&fold(&s.folder)) || segs.contains(&fold(&s.name))) && best.is_none() {
            best = Some((1, s, vec![]));
        }
        for p in known_paths(ix, &s.id) {
            for d in (0..p.len()).rev() {
                if segs.contains(&fold(&p[d])) {
                    if best.as_ref().is_none_or(|b| b.0 < d + 2) {
                        best = Some((d + 2, s, p[..=d].to_vec()));
                    }
                    break;
                }
            }
        }
    }
    match best {
        Some((_, s, values)) => json!({ "schema": s.id, "values": values }),
        None => json!({ "schema": null, "values": [] }),
    }
}

fn sizes_of(files: &[(String, u64)]) -> Vec<u64> {
    let mut v: Vec<u64> = files.iter().map(|f| f.1).collect();
    v.sort_unstable();
    v
}

/// One candidate as the page shows it.
fn describe(lib: &Library, ix: &Index, f: &Found) -> Value {
    let is_dir = f.files.is_empty();
    let files: Vec<(String, u64)> = if is_dir {
        model::list_files(&f.path)
    } else {
        f.files
            .iter()
            .map(|p| {
                (
                    file_name(p),
                    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0),
                )
            })
            .collect()
    };
    let side = if is_dir {
        model::read_sidecar(&f.path)
    } else {
        json!({})
    };
    let raw = if is_dir {
        file_name(&f.path)
    } else {
        file_name(&f.path)
            .rsplit_once('.')
            .map(|(s, _)| s.to_string())
            .unwrap_or_else(|| file_name(&f.path))
    };
    let (mut name, mut author) = guess_name(&raw);
    if let Some(n) = side["name"].as_str().filter(|n| !n.trim().is_empty()) {
        name = n.to_string();
    }
    if let Some(a) = side["authors"].as_array().and_then(|a| a.first()) {
        author = a["name"].as_str().or(a.as_str()).map(String::from);
    }
    let mut summary = model::summarise(&files, &side);
    summary.as_object_mut().unwrap().remove("cover");
    let mut warnings = vec![];
    if files.is_empty() {
        warnings.push(json!({ "kind": "empty" }));
    }
    if is_dir && !files.iter().any(|(r, _)| !r.contains('/') && is_main(r)) {
        let parts: Vec<String> = std::fs::read_dir(&f.path)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().is_dir())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        let with_models: Vec<String> = parts
            .into_iter()
            .filter(|p| {
                !p.starts_with(['_', '.'])
                    && files
                        .iter()
                        .any(|(r, _)| r.starts_with(&format!("{p}/")) && is_main(r))
            })
            .collect();
        if with_models.len() >= 2 {
            let mut with_models = with_models;
            with_models.sort();
            warnings.push(json!({ "kind": "several", "parts": with_models }));
        }
    }
    let rel = lib.relative(&f.path);
    if let Some(m) = rel
        .as_ref()
        .and_then(|r| ix.models.iter().find(|m| m.rel() == r))
    {
        warnings.push(json!({ "kind": "in-library", "id": m.id(), "rel": m.rel() }));
    }
    let count = files.len() as u64;
    let bytes: u64 = files.iter().map(|f| f.1).sum();
    if count > 0 {
        let mine = sizes_of(&files);
        for m in ix.models.iter().filter(|m| {
            m.v["files"]["count"] == json!(count)
                && m.v["files"]["bytes"] == json!(bytes)
                && Some(m.rel()) != rel.as_deref()
        }) {
            if let Ok(dir) = lib.resolve(m.rel()) {
                if sizes_of(&model::list_files(&dir)) == mine {
                    warnings.push(json!({ "kind": "duplicate", "id": m.id(), "name": m.v["name"], "rel": m.rel() }));
                    break;
                }
            }
        }
    }
    let guess = match (
        side["schema"].as_str().and_then(|s| ix.schema(s)),
        model::path_of(&side),
    ) {
        (Some(s), Some(path)) => json!({ "schema": s.id, "values": path }),
        _ => guess_category(ix, &f.path, &name),
    };
    json!({
        "source": f.path.display().to_string(),
        "files": f.files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        "folder": is_dir,
        "name": name,
        "author": author,
        "tags": side["tags"].as_array().map(|t| t.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default(),
        "has_sidecar": side.get("id").is_some(),
        "summary": summary,
        "warnings": warnings,
        "guess": guess,
    })
}

/// Refuse the library itself, a folder holding it, and the app's own folders.
fn check_source(lib: &Library, p: &Path) -> Result<()> {
    let p = p
        .canonicalize()
        .with_context(|| format!("{} isn't there", p.display()))?;
    let root = lib
        .root()
        .canonicalize()
        .unwrap_or_else(|_| lib.root().to_path_buf());
    if root.starts_with(&p) {
        bail!(
            "{} is the library or holds it: choose a folder inside it instead.",
            p.display()
        );
    }
    if p.starts_with(root.join(APP_DIR)) {
        bail!("{} is the app's own folder.", p.display());
    }
    Ok(())
}

/// Propose models. `contents`: each path's contents are the models (sorting a
/// folder); otherwise each path is one model (dropped items).
pub fn scan(lib: &Library, ix: &Index, paths: &[PathBuf], contents: bool) -> Result<Value> {
    let mut found = vec![];
    let mut left = vec![];
    for p in paths {
        check_source(lib, p)?;
        if contents && p.is_dir() {
            let (f, l) = propose(p)?;
            found.extend(f);
            left.extend(l.into_iter().map(|n| p.join(n).display().to_string()));
        } else if p.is_dir() {
            found.push(Found {
                path: p.clone(),
                files: vec![],
            });
        } else if p.is_file() {
            found.push(Found {
                path: p.clone(),
                files: vec![p.clone()],
            });
        } else {
            bail!("{} isn't there.", p.display());
        }
    }
    let items: Vec<Value> = found.iter().map(|f| describe(lib, ix, f)).collect();
    Ok(json!({ "items": items, "left_behind": left }))
}

/// Where one model goes: `<schema folder>/<subcategories…>/<model folder name>` or
/// `Unsorted/<model folder name>`, unique among `taken` and what's on disk
/// (`own` is the model's current folder, which doesn't count as taken).
pub fn destination(
    lib: &Library,
    schema: Option<&Schema>,
    values: &[String],
    folder_name: &str,
    own: Option<&Path>,
    taken: &HashSet<PathBuf>,
) -> Result<PathBuf> {
    let mut dir = lib.root().to_path_buf();
    match schema {
        Some(s) => {
            dir.push(&s.folder);
            let path = values
                .iter()
                .map(|v| schema::subcategory_name(v))
                .collect::<Result<Vec<String>>>()?;
            let tree = s.tree();
            if !schema::in_tree(&tree, &path) && path.len() > s.levels.len() {
                schema::check_path(lib, &s.folder, &tree, &path)?;
            }
            for v in path {
                dir.push(v);
            }
        }
        None => dir.push(UNSORTED),
    }
    let base = clean_folder_name(folder_name, 120);
    let base = if base.starts_with('_') {
        base.trim_start_matches('_').to_string()
    } else {
        base
    };
    let base = if base.is_empty() {
        "Untitled".to_string()
    } else {
        base
    };
    let mut n = 1;
    loop {
        let name = if n == 1 {
            base.clone()
        } else {
            format!("{base} ({n})")
        };
        let p = dir.join(&name);
        if own == Some(p.as_path()) || (!taken.contains(&p) && !p.exists()) {
            return Ok(p);
        }
        n += 1;
    }
}

/// A model folder's name from the template: "{name} ({author})", or the name alone.
pub fn folder_name(template: &str, name: &str, author: &str) -> String {
    let (name, author) = (name.trim(), author.trim());
    if author.is_empty() || !template.contains("{author}") {
        return name.to_string();
    }
    template.replace("{name}", name).replace("{author}", author)
}

/// Where each item goes, or why it can't. Items: { source, schema, values, name, author }.
pub fn plan(lib: &Library, ix: &Index, items: &[Value]) -> Vec<Value> {
    let mut taken = HashSet::new();
    items
        .iter()
        .map(|it| {
            let r = (|| -> Result<PathBuf> {
                let name = it["name"].as_str().unwrap_or("").trim();
                if name.is_empty() {
                    bail!("Give it a name.");
                }
                let schema = match it["schema"].as_str().filter(|s| !s.is_empty()) {
                    Some(id) => Some(
                        ix.schema(id)
                            .ok_or_else(|| anyhow!("There's no category {id} any more."))?,
                    ),
                    None => None,
                };
                let values: Vec<String> = it["values"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|v| v.as_str().unwrap_or("").to_string())
                    .collect();
                let template = schema
                    .map(|s| s.model_folder.as_str())
                    .unwrap_or(DEFAULT_TEMPLATE);
                let src = PathBuf::from(it["source"].as_str().unwrap_or(""));
                let own = if it["files"].as_array().is_none_or(Vec::is_empty) {
                    Some(src.as_path())
                } else {
                    None
                };
                destination(
                    lib,
                    schema,
                    &values,
                    &folder_name(template, name, it["author"].as_str().unwrap_or("")),
                    own,
                    &taken,
                )
            })();
            match r {
                Ok(p) => {
                    taken.insert(p.clone());
                    json!({ "dest": p.display().to_string(), "rel": lib.relative(&p) })
                }
                Err(e) => json!({ "error": format!("{e:#}") }),
            }
        })
        .collect()
}

// ------------------------------------------------------------ moving and copying

/// Progress of a copy, and the Stop button.
pub struct Progress<'a> {
    pub cancel: &'a AtomicBool,
    pub on_bytes: &'a dyn Fn(u64),
}

fn stopped(p: &Progress) -> Result<()> {
    if p.cancel.load(Ordering::Relaxed) {
        bail!("Stopped.");
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<[u8; 32]> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().into())
}

/// Copy one file, hashing the original as it's read, then check the copy.
fn copy_checked(src: &Path, dest: &Path, p: &Progress) -> Result<()> {
    use sha2::{Digest, Sha256};
    let mut from =
        std::fs::File::open(src).with_context(|| format!("can't read {}", src.display()))?;
    let mut to =
        std::fs::File::create(dest).with_context(|| format!("can't write {}", dest.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        stopped(p)?;
        let n = from.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        to.write_all(&buf[..n])?;
        (p.on_bytes)(n as u64);
    }
    to.sync_all()?;
    drop(to);
    let want: [u8; 32] = h.finalize().into();
    if hash_file(dest)? != want {
        bail!("the copy of {} doesn't match the original", src.display());
    }
    if let Ok(t) = std::fs::metadata(src).and_then(|m| m.modified()) {
        let _ = std::fs::File::options()
            .write(true)
            .open(dest)
            .and_then(|f| f.set_modified(t));
    }
    Ok(())
}

fn copy_tree(src: &Path, dest: &Path, p: &Progress) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    for e in std::fs::read_dir(src)?.flatten() {
        let (s, d) = (e.path(), dest.join(e.file_name()));
        if e.file_type()?.is_dir() {
            copy_tree(&s, &d, p)?;
        } else {
            copy_checked(&s, &d, p)?;
        }
    }
    Ok(())
}

/// Total size of a folder's files (for progress).
pub fn tree_bytes(p: &Path) -> u64 {
    if p.is_file() {
        return std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    }
    std::fs::read_dir(p)
        .map(|rd| rd.flatten().map(|e| tree_bytes(&e.path())).sum())
        .unwrap_or(0)
}

/// Move or copy a folder or file to `dest` (which must not exist yet). A move is a
/// rename when it can be (`force_copy` makes it copy, check, then delete, as across
/// drives). Returns a note when the original couldn't be removed after a move.
pub fn transfer(
    src: &Path,
    dest: &Path,
    mv: bool,
    force_copy: bool,
    p: &Progress,
) -> Result<Option<String>> {
    if dest.exists() {
        bail!("{} is already there", dest.display());
    }
    if dest.starts_with(src) {
        bail!("can't put {} inside itself", src.display());
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if mv && !force_copy && std::fs::rename(src, dest).is_ok() {
        (p.on_bytes)(tree_bytes(dest));
        return Ok(None);
    }
    let r = if src.is_dir() {
        copy_tree(src, dest, p)
    } else {
        copy_checked(src, dest, p)
    };
    if let Err(e) = r {
        if dest.is_dir() {
            let _ = std::fs::remove_dir_all(dest);
        } else {
            let _ = std::fs::remove_file(dest);
        }
        return Err(e);
    }
    if mv {
        let gone = if src.is_dir() {
            std::fs::remove_dir_all(src)
        } else {
            std::fs::remove_file(src)
        };
        if let Err(e) = gone {
            return Ok(Some(format!(
                "Copied, but the original couldn't be removed: {e}"
            )));
        }
    }
    Ok(None)
}

/// Import one planned item: move or copy it to `dest`, then write its model.json
/// (keeping the one it brought, minus an id that's already in use).
#[allow(clippy::too_many_arguments)]
pub fn commit_one(
    lib: &Library,
    item: &Value,
    dest: &Path,
    schema: Option<&Schema>,
    mv: bool,
    force_copy: bool,
    ids: &HashSet<String>,
    p: &Progress,
) -> Result<Value> {
    lib.writable()?;
    let src = PathBuf::from(item["source"].as_str().unwrap_or(""));
    let loose: Vec<PathBuf> = item["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(PathBuf::from)
        .collect();
    let mut notes = vec![];
    if loose.is_empty() {
        if let Some(n) = transfer(&src, dest, mv, force_copy, p)? {
            notes.push(n);
        }
    } else {
        let staging = dest.with_file_name(format!(".importing-{}", file_name(dest)));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging)?;
        let mut moved = vec![];
        for f in &loose {
            match transfer(f, &staging.join(file_name(f)), mv, force_copy, p) {
                Ok(n) => {
                    notes.extend(n);
                    moved.push(f.clone());
                }
                Err(e) => {
                    // put back what was already moved, then give up
                    for m in &moved {
                        let _ = std::fs::rename(staging.join(file_name(m)), m);
                    }
                    let _ = std::fs::remove_dir_all(&staging);
                    return Err(e);
                }
            }
        }
        std::fs::rename(&staging, dest)?;
    }
    let mut side = model::read_sidecar(dest);
    if side["id"].as_str().is_some_and(|id| ids.contains(id)) {
        side.as_object_mut().unwrap().remove("id");
        crate::config::write_json(&dest.join(SIDECAR), &side)?;
    }
    let path = place_of(lib, schema, dest);
    let mut patch = json!({ "name": item["name"], "authors": item["author"].as_str().unwrap_or(""), "tags": item["tags"].as_str().unwrap_or("") });
    if let Some(f) = item["fields"].as_object() {
        patch["fields"] = json!(f);
    }
    model::update(
        dest,
        &patch,
        &json!({ "imported_from": src.display().to_string() }),
    )?;
    let side = model::set_place(dest, schema.map(|s| s.id.as_str()), &path)?;
    if let Some(s) = schema {
        schema::define_path(lib, &s.id, &path)?;
    }
    Ok(
        json!({ "id": side["id"], "dest": dest.display().to_string(), "rel": lib.relative(dest), "note": if notes.is_empty() { Value::Null } else { json!(notes.join(" ")) } }),
    )
}

/// Move a model already in the library to another category (or Unsorted). Its folder
/// keeps its name; category folders left empty are removed. Returns its new folder.
pub fn move_model(
    lib: &Library,
    ix: &Index,
    id: &str,
    schema: Option<&Schema>,
    values: &[String],
) -> Result<PathBuf> {
    lib.writable()?;
    let m = ix
        .get(id)
        .ok_or_else(|| anyhow!("That model isn't in the library any more."))?;
    let dir = lib.resolve(m.rel())?;
    let dest = destination(
        lib,
        schema,
        values,
        &file_name(&dir),
        Some(&dir),
        &HashSet::new(),
    )?;
    if dest != dir {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(&dir, &dest).with_context(|| format!("couldn't move {}", m.rel()))?;
        prune_empty(lib, ix, dir.parent());
    }
    // a first model.json starts from what the folder said
    let defaults = json!({ "name": m.v["name"], "authors": m.v["authors"].as_array().filter(|a| !a.is_empty()).map(|a| a.iter().map(|n| json!({ "name": n })).collect::<Vec<_>>()) });
    model::update(&dest, &json!({}), &defaults)?;
    let path = place_of(lib, schema, &dest);
    model::set_place(&dest, schema.map(|s| s.id.as_str()), &path)?;
    if let Some(s) = schema {
        schema::define_path(lib, &s.id, &path)?;
    }
    Ok(dest)
}

/// A model folder's path of subcategories under its schema's folder.
pub fn place_of(lib: &Library, schema: Option<&Schema>, dir: &Path) -> Vec<String> {
    let Some(s) = schema else {
        return vec![];
    };
    dir.parent()
        .and_then(|p| p.strip_prefix(lib.root().join(&s.folder)).ok())
        .map(|r| {
            r.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// Remove empty category folders from `dir` upwards, stopping at a schema's own
/// folder, Unsorted and the library itself.
fn prune_empty(lib: &Library, ix: &Index, mut dir: Option<&Path>) {
    let stops: Vec<PathBuf> = ix
        .schemas
        .iter()
        .map(|s| lib.root().join(&s.folder))
        .chain([lib.root().join(UNSORTED), lib.root().to_path_buf()])
        .collect();
    while let Some(d) = dir {
        if stops.iter().any(|s| s == d)
            || !d.starts_with(lib.root())
            || crate::schema::kept_folder(lib, d)
        {
            break;
        }
        if std::fs::remove_dir(d).is_err() {
            break; // not empty
        }
        dir = d.parent();
    }
}

/// Folders at the top of the library that aren't a category's, Unsorted or the app's
/// (empty ones aside: they're what sorting a folder leaves behind).
pub fn loose_folders(lib: &Library, ix: &Index) -> Vec<String> {
    let used: HashSet<String> = ix
        .schemas
        .iter()
        .map(|s| s.folder.to_lowercase())
        .chain([UNSORTED.to_lowercase()])
        .collect();
    let mut out: Vec<String> = std::fs::read_dir(lib.root())
        .map(|rd| {
            rd.flatten()
                .filter(|e| {
                    e.path().is_dir()
                        && std::fs::read_dir(e.path()).is_ok_and(|mut rd| rd.next().is_some())
                })
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| !n.starts_with(['_', '.']) && !used.contains(&n.to_lowercase()))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// Ids in use in the library (a re-imported model.json with one of them gets a new id).
pub fn ids_in_use(ix: &Index) -> HashSet<String> {
    ix.models.iter().map(|m| m.id().to_string()).collect()
}

/// Schemas by id, for the job (which runs without the index).
pub fn schemas_by_id(ix: &Index) -> HashMap<String, Schema> {
    ix.schemas
        .iter()
        .map(|s| (s.id.clone(), s.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("modlib-import-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn put(p: &Path, body: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    fn no_progress() -> (AtomicBool, impl Fn(u64)) {
        (AtomicBool::new(false), |_| {})
    }

    #[test]
    fn proposes_models_from_a_messy_folder() {
        let d = tmp("propose");
        let lib = Library::open(d.join("Lib")).unwrap();
        schema::create(
            &lib,
            &json!({ "name": "Wargames", "levels": [{ "label": "Game" }, { "label": "Faction" }] }),
        )
        .unwrap();
        put(
            &lib.root()
                .join("Wargames/Warhammer 40k/Tyranid/Hive Tyrant (Jo)/body.stl"),
            "0123456789",
        );
        let ix = Index::build(&lib, None, false);
        let src = d.join("Downloads");
        put(&src.join("Tyranid/Carnifex (Al Jones)/carnifex.stl"), "x");
        put(&src.join("Armour Set/Helmet/helmet.stl"), "x");
        put(&src.join("Armour Set/Arms/arm.stl"), "x");
        put(&src.join("benchy.stl"), "x");
        put(&src.join("benchy.3mf"), "x");
        put(&src.join("benchy_photo.jpg"), "x");
        put(&src.join("dragon.zip"), "x");
        put(&src.join("notes.txt"), "x");
        put(&src.join("Copy of tyrant/body.stl"), "0123456789");
        let r = scan(&lib, &ix, std::slice::from_ref(&src), true).unwrap();
        let items = r["items"].as_array().unwrap();
        let names: Vec<&str> = items.iter().map(|i| i["name"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            [
                "Armour Set",
                "Copy of tyrant",
                "Tyranid",
                "benchy",
                "dragon"
            ]
        );
        assert_eq!(
            items[0]["warnings"][0],
            json!({ "kind": "several", "parts": ["Arms", "Helmet"] })
        );
        assert_eq!(items[1]["warnings"][0]["kind"], "duplicate");
        assert_eq!(items[3]["files"].as_array().unwrap().len(), 3);
        assert_eq!(r["left_behind"].as_array().unwrap().len(), 1);
        // the Tyranid folder is one level up: scanning it as one model guesses its faction
        let r = scan(&lib, &ix, &[src.join("Tyranid/Carnifex (Al Jones)")], false).unwrap();
        let it = &r["items"][0];
        assert_eq!(
            (it["name"].as_str(), it["author"].as_str()),
            (Some("Carnifex"), Some("Al Jones"))
        );
        assert_eq!(
            it["guess"],
            json!({ "schema": "wargames", "values": ["Warhammer 40k", "Tyranid"] })
        );
        assert!(scan(&lib, &ix, &[lib.root().to_path_buf()], true).is_err());
        assert!(scan(&lib, &ix, std::slice::from_ref(&d), true).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn plans_destinations() {
        let d = tmp("plan");
        let lib = Library::open(d.join("Lib")).unwrap();
        schema::create(&lib, &json!({ "name": "Wargames" })).unwrap();
        std::fs::create_dir_all(lib.root().join("Unsorted/Benchy")).unwrap();
        put(&lib.root().join("Wargames/Lamp (Jo)/model.json"), "{}");
        let ix = Index::build(&lib, None, false);
        let p = plan(
            &lib,
            &ix,
            &[
                json!({ "source": "/x/a", "schema": "wargames", "values": ["40k", "Tyranid: Hive?"], "name": "Hive Tyrant", "author": "Jo" }),
                json!({ "source": "/x/b", "schema": "wargames", "values": ["40k", "Tyranid: Hive?"], "name": "Hive Tyrant", "author": "Jo" }),
                json!({ "source": "/x/c", "schema": "wargames", "values": ["40k", " "], "name": "X" }),
                json!({ "source": "/x/d", "schema": null, "name": "Benchy", "author": "" }),
                json!({ "source": "/x/e", "schema": "wargames", "values": [], "name": "Top" }),
                json!({ "source": "/x/f", "schema": "wargames", "values": ["40k", "_x"], "name": "Y" }),
                json!({ "source": "/x/g", "schema": "wargames", "values": ["Lamp (Jo)", "Bits"], "name": "Z" }),
            ],
        );
        assert_eq!(p[0]["rel"], "Wargames/40k/Tyranid- Hive-/Hive Tyrant (Jo)");
        assert_eq!(
            p[1]["rel"],
            "Wargames/40k/Tyranid- Hive-/Hive Tyrant (Jo) (2)"
        );
        assert_eq!(p[2]["error"], "Give every subcategory a name.");
        assert_eq!(p[3]["rel"], "Unsorted/Benchy (2)");
        // a model can sit at the top of a category, but not in an app folder
        assert_eq!(p[4]["rel"], "Wargames/Top");
        assert!(p[5]["error"]
            .as_str()
            .unwrap()
            .contains("can't start with _"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn imports_by_move_and_by_checked_copy() {
        let d = tmp("commit");
        let lib = Library::open(d.join("Lib")).unwrap();
        let s = schema::create(&lib, &json!({ "name": "Wargames" })).unwrap();
        let src = d.join("Downloads");
        put(&src.join("Tyrant/body.stl"), "solid");
        put(&src.join("Tyrant/Arms/arm.stl"), "solid arm");
        put(
            &src.join("Old/model.json"),
            r#"{"id":"m1","name":"Old one","future":1}"#,
        );
        put(&src.join("Old/old.stl"), "x");
        put(&src.join("benchy.stl"), "b");
        put(&src.join("benchy.png"), "p");
        let (cancel, cb) = no_progress();
        let p = Progress {
            cancel: &cancel,
            on_bytes: &cb,
        };
        let ids: HashSet<String> = ["m1".to_string()].into();
        let item = json!({ "source": src.join("Tyrant").display().to_string(), "files": [], "name": "Hive Tyrant", "author": "Jo", "tags": "big", "values": ["40k", "Tyranid"] });
        let dest = lib.root().join("Wargames/40k/Tyranid/Hive Tyrant (Jo)");
        let r = commit_one(&lib, &item, &dest, Some(&s), true, true, &ids, &p).unwrap();
        assert!(dest.join("Arms/arm.stl").is_file() && !src.join("Tyrant").exists());
        let side = model::read_sidecar(&dest);
        assert_eq!(side["path"], json!(["40k", "Tyranid"]));
        // the place is in the category's tree now
        assert_eq!(
            schema::list(&lib)[0].tree(),
            vec![
                vec!["40k".to_string()],
                vec!["40k".into(), "Tyranid".into()]
            ]
        );
        assert_eq!(
            (
                side["schema"].as_str(),
                side["tags"].clone(),
                r["id"].clone()
            ),
            (Some("wargames"), json!(["big"]), side["id"].clone())
        );
        // copy keeps the original; a used id is replaced, unknown keys kept
        let item = json!({ "source": src.join("Old").display().to_string(), "files": [], "name": "Old one", "author": "" });
        let dest = lib.root().join("Unsorted/Old one");
        commit_one(&lib, &item, &dest, None, false, false, &ids, &p).unwrap();
        let side = model::read_sidecar(&dest);
        assert!(src.join("Old/old.stl").is_file() && dest.join("old.stl").is_file());
        assert_ne!(side["id"], "m1");
        assert_eq!(side["future"], 1);
        assert!(side["schema"].is_null() && side.get("path").is_none());
        // loose files get a folder
        let item = json!({ "source": src.join("benchy.stl").display().to_string(), "files": [src.join("benchy.stl").display().to_string(), src.join("benchy.png").display().to_string()], "name": "Benchy", "author": "" });
        let dest = lib.root().join("Unsorted/Benchy");
        commit_one(&lib, &item, &dest, None, true, true, &ids, &p).unwrap();
        assert!(
            dest.join("benchy.stl").is_file()
                && dest.join("benchy.png").is_file()
                && !src.join("benchy.stl").exists()
        );
        // stopping leaves the original and no half copy
        cancel.store(true, Ordering::Relaxed);
        put(&src.join("Big/a.stl"), "x");
        let item = json!({ "source": src.join("Big").display().to_string(), "files": [], "name": "Big", "author": "" });
        assert!(commit_one(
            &lib,
            &item,
            &lib.root().join("Unsorted/Big"),
            None,
            true,
            true,
            &ids,
            &p
        )
        .is_err());
        assert!(src.join("Big/a.stl").is_file() && !lib.root().join("Unsorted/Big").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn moves_models_between_categories() {
        let d = tmp("move");
        let lib = Library::open(d.join("Lib")).unwrap();
        schema::create(&lib, &json!({ "name": "Wargames" })).unwrap();
        put(&lib.root().join("Unsorted/Carnifex (Al)/c.stl"), "x");
        put(&lib.root().join("Old stuff/x.stl"), "x");
        let ix = Index::build(&lib, None, false);
        assert_eq!(loose_folders(&lib, &ix), ["Old stuff"]);
        let id = ix.models[0].id().to_string();
        let s = ix.schema("wargames").cloned();
        let dest = move_model(
            &lib,
            &ix,
            &id,
            s.as_ref(),
            &["40k".into(), "Tyranid".into()],
        )
        .unwrap();
        assert_eq!(
            lib.relative(&dest).unwrap(),
            "Wargames/40k/Tyranid/Carnifex (Al)"
        );
        let side = model::read_sidecar(&dest);
        assert_eq!(
            (side["name"].as_str(), side["authors"][0]["name"].as_str()),
            (Some("Carnifex"), Some("Al"))
        );
        assert_eq!(side["path"], json!(["40k", "Tyranid"]));
        // up a level: branches can be any depth
        let ix = Index::build(&lib, None, false);
        let id = ix.models[0].id().to_string();
        let dest = move_model(&lib, &ix, &id, s.as_ref(), &["40k".into()]).unwrap();
        assert_eq!(lib.relative(&dest).unwrap(), "Wargames/40k/Carnifex (Al)");
        let ix = Index::build(&lib, None, false);
        assert_eq!(ix.models[0].v["path"], json!(["40k"]));
        // and back to Unsorted: the subcategories stay, as they're in the tree
        move_model(&lib, &ix, &id, None, &[]).unwrap();
        assert!(lib.root().join("Unsorted/Carnifex (Al)/c.stl").is_file());
        assert!(lib.root().join("Wargames/40k/Tyranid").is_dir());
        let ix = Index::build(&lib, None, false);
        assert_eq!(ix.models.len(), 1);
        assert_eq!(ix.models[0].rel(), "Unsorted/Carnifex (Al)");
        let _ = std::fs::remove_dir_all(&d);
    }
}
