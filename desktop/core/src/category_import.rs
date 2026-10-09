//! Reviewed folder-to-category imports (Agent D).
//! Scans are read-only. The client may edit names, classification and exclusions,
//! but source manifests and destination paths are rebuilt and checked before commit.
use crate::import;
use crate::index::Index;
use crate::library::Library;
use crate::model::{file_kind, SIDECAR};
use crate::schema::{self, Schema};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_DEPTH: usize = 32;
const MAX_ENTRIES: usize = 20_000;

fn string(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or("").to_string()
}
fn strings(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect()
}
fn path_text(p: &Path) -> String { p.display().to_string() }
fn file_count(node: &Value) -> u64 {
    if node["hash"].is_string() { 1 }
    else { node["children"].as_array().into_iter().flatten().map(file_count).sum() }
}

fn digest(path: &Path) -> Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 { break; }
        hash.update(&buf[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn control(name: &str) -> bool {
    name.starts_with('.') || matches!(name.to_ascii_lowercase().as_str(),
        "thumbs.db" | "desktop.ini" | "model.json" | "folder.jpg")
}
fn descend(p: &Path, depth: usize, count: &mut usize, root: bool) -> Result<Value> {
    if depth > MAX_DEPTH { bail!("The folder tree is more than {MAX_DEPTH} levels deep."); }
    *count += 1;
    if *count > MAX_ENTRIES { bail!("This tree has more than {MAX_ENTRIES} entries. Choose a smaller folder."); }
    let meta = std::fs::symlink_metadata(p)?;
    if meta.file_type().is_symlink() { bail!("Symlinks cannot be imported as categories: {}.", p.display()); }
    let name = import::file_name(p);
    let source = path_text(p);
    if meta.is_file() {
        let bytes = meta.len();
        return Ok(json!({ "source": source, "name": name, "kind": "file", "include": !control(&name),
            "bytes": bytes, "hash": digest(p)?, "children": [] }));
    }
    if !meta.is_dir() { bail!("{} is not a regular file or folder.", p.display()); }
    let mut entries: Vec<PathBuf> = std::fs::read_dir(p)
        .with_context(|| format!("Cannot read {}.", p.display()))?
        .map(|r| r.map(|e| e.path())).collect::<std::io::Result<_>>()?;
    entries.sort_by_key(|p| import::file_name(p).to_lowercase());
    let mut children = Vec::with_capacity(entries.len());
    let mut bytes = 0u64;
    let mut has_main = false;
    let mut has_dir = false;
    let mut sidecar = false;
    for entry in entries {
        let child = descend(&entry, depth + 1, count, false)?;
        let n = string(&child, "name");
        if child["kind"] == "file" {
            has_main |= matches!(file_kind(&n), "model" | "archive" | "slicer");
            sidecar |= n == SIDECAR;
        } else {
            has_dir = true;
        }
        bytes = bytes.saturating_add(child["bytes"].as_u64().unwrap_or(0));
        children.push(child);
    }
    // A recognisable model can have internal picture/document folders. If a
    // child already looks like a model, retain the parent as a category.
    let has_model_child = children.iter().any(|c| c["kind"] == "model");
    let kind = if !root && (sidecar || (has_main && (!has_dir || !has_model_child))) {
        "model"
    } else { "category" };
    Ok(json!({ "source": source, "name": name, "kind": kind,
        "include": root || (!name.starts_with('.') && name != "_library"), "group": false, "group_name": format!("{} files", name),
        "target": Value::Null, "map_to": Value::Null, "bytes": bytes, "children": children }))
}

/// Scan selected roots, without writing category files or altering imported data.
pub fn scan(lib: &Library, paths: &[PathBuf]) -> Result<Value> {
    if paths.is_empty() { bail!("Choose at least one folder."); }
    let mut roots = vec![];
    let mut count = 0;
    let mut taken: Vec<PathBuf> = vec![];
    for p in paths {
        import::check_source(lib, p)?;
        let real = p.canonicalize()?;
        if !real.is_dir() { bail!("{} is not a folder.", p.display()); }
        if taken.iter().any(|a| real.starts_with(a) || a.starts_with(&real)) {
            bail!("Select non-overlapping folders to avoid importing anything twice.");
        }
        taken.push(real.clone());
        roots.push(descend(&real, 0, &mut count, true)?);
    }
    Ok(json!({ "roots": roots, "entries": count }))
}

/// Compare the immutable source manifest only. All presentation and mapping
/// fields can be edited, but not a file, a hash or the structure of the source.
fn unchanged(old: &Value, fresh: &Value) -> Result<()> {
    if old["source"] != fresh["source"] || old["bytes"] != fresh["bytes"]
        || old["hash"] != fresh["hash"] {
        bail!("The source changed since it was staged: {}. Scan it again.", string(old, "source"));
    }
    let a = old["children"].as_array().ok_or_else(|| anyhow!("Invalid staging tree."))?;
    let b = fresh["children"].as_array().ok_or_else(|| anyhow!("Invalid source tree."))?;
    if a.len() != b.len() {
        bail!("Files were added or removed in {}. Scan it again.", string(old, "source"));
    }
    for (x, y) in a.iter().zip(b) { unchanged(x, y)?; }
    Ok(())
}
fn label(name: &str) -> Result<String> {
    let n = schema::subcategory_name(name)?;
    if n != name.trim() { bail!("Rename {name} to a valid folder name before importing."); }
    Ok(n)
}
fn model_name(name: &str) -> Result<String> {
    let n = name.trim();
    if n.is_empty() || n.starts_with('_') || n == "." || n == ".."
        || n.contains('/') || n.contains('\\') || n.chars().any(char::is_control) {
        bail!("Give every model a valid name.");
    }
    Ok(n.to_string())
}
fn selected(node: &Value) -> bool { node["include"] != false }
fn as_children(node: &Value) -> Result<&Vec<Value>> {
    node["children"].as_array().ok_or_else(|| anyhow!("Invalid folder tree."))
}
fn virtual_schema(id: &str, name: &str) -> Schema {
    Schema {
        id: id.into(), name: name.into(), folder: name.into(),
        model_folder: "{name} ({author})".into(), levels: vec![],
        raw: json!({ "id": id, "name": name, "folder": name, "model_folder": "{name} ({author})" }),
    }
}
struct Planner<'a> {
    lib: &'a Library,
    schemas: HashMap<String, Schema>,
    categories: Vec<Value>,
    subcategories: Vec<Value>,
    items: Vec<Value>,
    conflicts: Vec<String>,
    taken: HashSet<PathBuf>,
    places: HashSet<String>,
}
impl Planner<'_> {
    fn place(&mut self, id: &str, path: &[String], explicit: bool) {
        let k = format!("{}:{}", id.to_lowercase(), path.join("/").to_lowercase());
        if !explicit && !self.places.insert(k) {
            self.conflicts.push(format!("Two proposed category folders use {}.", path.join(" › ")));
        }
    }
    fn add_model(&mut self, node: &Value, schema_id: &str, path: &[String], files: Vec<String>, name: &str) -> Result<()> {
        let name = model_name(name)?;
        let s = self.schemas.get(schema_id).ok_or_else(|| anyhow!("The category is no longer available."))?;
        let folder = import::folder_name(&s.model_folder, &name, "");
        let dest = import::destination(self.lib, Some(s), path, &folder, None, &self.taken)?;
        let actual = import::file_name(&dest);
        let collision = actual != folder;
        if self.items.iter().any(|i| i["dest"].as_str()
            .is_some_and(|d| d.eq_ignore_ascii_case(&path_text(&dest)))) {
            self.conflicts.push(format!("Two models would have the same destination, ignoring letter case: {}.", dest.display()));
        }
        self.taken.insert(dest.clone());
        let count = if files.is_empty() { file_count(node) } else { files.len() as u64 };
        self.items.push(json!({
            "source": node["source"], "files": files, "name": name, "author": "", "tags": "",
            "schema": schema_id, "values": path, "dest": path_text(&dest),
            "rel": self.lib.relative(&dest), "bytes": node["bytes"],
            "file_count": count,
            "collision": collision, "planned_name": actual
        }));
        Ok(())
    }
    fn walk(&mut self, node: &Value, schema_id: &str, at: &[String], root: bool) -> Result<()> {
        if !selected(node) { return Ok(()); }
        let kind = node["kind"].as_str().ok_or_else(|| anyhow!("Choose a folder type."))?;
        if (node["hash"].is_string()) != (kind == "file") {
            bail!("Only folders can become category containers or grouped models.");
        }
        if kind == "file" {
            if root { bail!("Choose a folder, not a file."); }
            let filename = string(node, "name");
            let name = Path::new(&filename).file_stem().and_then(|s| s.to_str()).unwrap_or(&filename);
            self.add_model(node, schema_id, at, vec![string(node, "source")], name)?;
            return Ok(());
        }
        if kind == "model" {
            if root { bail!("The selected root must be a category. Choose a subfolder to group as a model."); }
            // Folder-as-model preserves the whole nested folder. Do not pretend to
            // support exclusions below a model that would need a different copy plan.
            fn all_in(n: &Value) -> bool {
                n["children"].as_array().is_some_and(|v| v.iter().all(|c| selected(c) || control(&string(c, "name"))) && v.iter().all(all_in))
            }
            if !all_in(node) { bail!("A model folder imports all of its contents. Split it into categories to exclude files."); }
            self.add_model(node, schema_id, at, vec![], &string(node, "name"))?;
            return Ok(());
        }
        if kind != "category" { bail!("Invalid folder classification."); }
        let here = if root {
            at.to_vec()
        } else if node["map_to"].is_array() {
            let mapped = strings(&node["map_to"]);
            let sc = self.schemas.get(schema_id).unwrap();
            if !schema::in_tree(&sc.tree(), &mapped) {
                bail!("Map {} to an existing subcategory or leave it as new.", string(node, "name"));
            }
            mapped
        } else {
            let mut p = at.to_vec();
            p.push(label(&string(node, "name"))?);
            p
        };
        let explicit = root || node["map_to"].is_array();
        if !root {
            if !explicit && self.schemas.get(schema_id).is_some_and(|sc| schema::in_tree(&sc.tree(), &here)) {
                self.conflicts.push(format!("{} already exists. Map it explicitly to that subcategory, or rename it.", here.join(" › ")));
            }
            self.place(schema_id, &here, explicit);
            self.subcategories.push(json!({ "schema": schema_id, "path": here, "existing": explicit }));
        }
        let mut grouped = vec![];
        for child in as_children(node)? {
            if !selected(child) || child["kind"] != "file" { continue; }
            if node["group"] == true { grouped.push(string(child, "source")); }
        }
        if !grouped.is_empty() {
            let mut direct = node.clone();
            direct["bytes"] = json!(as_children(node)?.iter()
                .filter(|c| selected(c) && c["kind"] == "file")
                .map(|c| c["bytes"].as_u64().unwrap_or(0)).sum::<u64>());
            self.add_model(&direct, schema_id, &here, grouped, &string(node, "group_name"))?;
        }
        for child in as_children(node)? {
            if node["group"] == true && child["kind"] == "file" { continue; }
            self.walk(child, schema_id, &here, false)?;
        }
        Ok(())
    }
}

/// Dry-run the complete destination tree; neither this nor scan writes to the
/// library. Every request rehashes its source manifest, catching changed files.
pub fn plan(lib: &Library, ix: &Index, proposal: &Value) -> Result<Value> {
    let roots = proposal["roots"].as_array().ok_or_else(|| anyhow!("No staged folders."))?;
    let paths: Vec<PathBuf> = roots.iter().map(|r| PathBuf::from(string(r, "source"))).collect();
    if paths.is_empty() { bail!("Choose at least one folder."); }
    let fresh = scan(lib, &paths)?;
    for (a, b) in roots.iter().zip(fresh["roots"].as_array().unwrap()) { unchanged(a, b)?; }
    let mut p = Planner {
        lib, schemas: ix.schemas.iter().map(|s| (s.id.clone(), s.clone())).collect(),
        categories: vec![], subcategories: vec![], items: vec![], conflicts: vec![],
        taken: HashSet::new(), places: HashSet::new(),
    };
    let mut new_names = HashSet::new();
    for (i, root) in roots.iter().enumerate() {
        if !selected(root) { continue; }
        if root["kind"] != "category" { bail!("Selected roots must remain category folders."); }
        let (sid, at) = if root["target"].is_object() {
            let sid = string(&root["target"], "schema");
            let values = strings(&root["target"]["values"]);
            let s = p.schemas.get(&sid).ok_or_else(|| anyhow!("The destination category no longer exists."))?;
            if !values.is_empty() && !schema::in_tree(&s.tree(), &values) {
                bail!("Choose an existing destination subcategory for {}.", string(root, "name"));
            }
            (sid, values)
        } else {
            let name = label(&string(root, "name"))?;
            if name.eq_ignore_ascii_case("Unsorted") || !new_names.insert(name.to_lowercase())
                || p.schemas.values().any(|s| s.folder.eq_ignore_ascii_case(&name))
                || p.lib.root().join(&name).exists() {
                p.conflicts.push(format!("The category {} already exists. Rename it or explicitly map to the existing one.", name));
            }
            let draft = format!("draft-{i}");
            p.schemas.insert(draft.clone(), virtual_schema(&draft, &name));
            p.categories.push(json!({ "draft": draft, "name": name, "folder": name }));
            (draft, vec![])
        };
        p.walk(root, &sid, &at, true)?;
    }
    // Model folders cannot also be proposed category containers, including
    // collisions whose only difference is case on Windows.
    for c in &p.subcategories {
        if let (Some(sid), Some(parts)) = (c["schema"].as_str(), c["path"].as_array()) {
            if let Some(s) = p.schemas.get(sid) {
                let mut directory = lib.root().join(&s.folder);
                for part in parts { if let Some(n) = part.as_str() { directory.push(n); } }
                if p.items.iter().any(|it| it["dest"].as_str()
                    .is_some_and(|d| d.eq_ignore_ascii_case(&path_text(&directory)))) {
                    p.conflicts.push(format!("A model and subcategory both use {}. Rename one.", directory.display()));
                }
            }
        }
    }
    if p.items.is_empty() { p.conflicts.push("No included model or file is ready to import.".into()); }
    Ok(json!({ "items": p.items, "categories": p.categories, "subcategories": p.subcategories,
        "conflicts": p.conflicts, "models": p.items.len(),
        "file_count": p.items.iter().map(|i| i["file_count"].as_u64().unwrap_or(0)).sum::<u64>(),
        "bytes": p.items.iter().map(|i| i["bytes"].as_u64().unwrap_or(0)).sum::<u64>() }))
}

/// A commit must use the exact model paths and newly created category nodes
/// shown in the final review, including the transfer mode. The client does
/// not get to override anything: the server recreates and compares the plan.
pub fn require_review(actual: &Value, reviewed: &Value, mode: &str) -> Result<()> {
    if reviewed["reviewed_mode"] != mode {
        bail!("Move or Copy changed since review. Review destinations again.");
    }
    let mut previous = reviewed.clone();
    let Some(obj) = previous.as_object_mut() else {
        bail!("Review the destination plan before importing.");
    };
    obj.remove("reviewed_mode");
    if &previous != actual {
        bail!("The reviewed import paths or category structure have changed. Review destinations again before importing.");
    }
    Ok(())
}

/// Hash every file of a completed model so an Undo never removes a changed copy.
/// Missing/new/modified files all invalidate the saved manifest.
pub fn destination_manifest(dir: &Path) -> Result<Value> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<Value>) -> Result<()> {
        let mut paths = std::fs::read_dir(dir)?.map(|e| e.map(|e| e.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        paths.sort();
        for path in paths {
            let m = std::fs::symlink_metadata(&path)?;
            if m.file_type().is_symlink() { bail!("A model now contains a symlink: {}.", path.display()); }
            if m.is_dir() { walk(base, &path, out)?; }
            else if m.is_file() {
                let rel = path.strip_prefix(base)?.to_string_lossy().replace('\\', "/");
                out.push(json!({ "path": rel, "bytes": m.len(), "sha256": digest(&path)? }));
            }
        }
        Ok(())
    }
    let mut v = vec![];
    walk(dir, dir, &mut v)?;
    Ok(json!(v))
}


/// Predict the exact IDs create() assigns before any categories are touched.
/// Those IDs and paths are journalled first, so interruption between creating a
/// schema file and recording success does not orphan an untracked category.
pub fn drafted_schemas(lib: &Library, planned: &Value) -> Result<Vec<Value>> {
    let mut ids: HashSet<String> = schema::list(lib).into_iter().map(|s| s.id).collect();
    let mut out = vec![];
    for category in planned["categories"].as_array().into_iter().flatten() {
        let name = category["name"].as_str().ok_or_else(|| anyhow!("Missing category name."))?;
        let base = crate::library::slug(name);
        let base = if base.is_empty() { "schema" } else { &base };
        let mut id = base.to_string();
        let mut n = 2;
        while ids.contains(&id) || lib.root().join("_library").join("schemas").join(format!("{id}.json")).exists() {
            id = format!("{base}-{n}");
            n += 1;
        }
        ids.insert(id.clone());
        out.push(json!({ "id": id, "draft": category["draft"], "name": name,
            "folder": category["folder"], "phase": "planned" }));
    }
    Ok(out)
}

fn source_file(path: &Path, relative: &str, out: &mut Vec<Value>) -> Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() { bail!("A source is a symlink: {}.", path.display()); }
    if meta.is_dir() {
        let mut children = std::fs::read_dir(path)?.map(|e| e.map(|e| e.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        children.sort();
        for child in children {
            let name = import::file_name(&child);
            let rel = if relative.is_empty() { name } else { format!("{relative}/{name}") };
            source_file(&child, &rel, out)?;
        }
    } else if meta.is_file() {
        // model.json is rewritten in the published model. Keep its raw bytes
        // so recovery can restore source metadata byte-for-byte after cleanup.
        let raw = if relative == SIDECAR {
            use base64::Engine;
            Value::String(base64::engine::general_purpose::STANDARD.encode(std::fs::read(path)?))
        } else { Value::Null };
        out.push(json!({ "path": path_text(path), "relative": relative,
            "sha256": digest(path)?, "bytes": meta.len(), "original_sidecar": raw }));
    } else { bail!("Not a regular source file: {}.", path.display()); }
    Ok(())
}

/// Durable input preimage for one pending import. Relative names match the
/// files in its published model; absolute names are used for exact restoration.
pub fn source_manifest(item: &Value) -> Result<Value> {
    let mut out = vec![];
    let files = strings(&item["files"]);
    if files.is_empty() {
        source_file(Path::new(item["source"].as_str().unwrap_or("")), "", &mut out)?;
    } else {
        for file in files {
            let path = Path::new(&file);
            source_file(path, &import::file_name(path), &mut out)?;
        }
    }
    Ok(json!(out))
}

fn valid_originals(entries: &[Value]) -> bool {
    entries.iter().all(|entry| {
        let p = Path::new(entry["path"].as_str().unwrap_or(""));
        p.is_file() && entry["sha256"].as_str().is_some_and(|sha| digest(p).ok().as_deref() == Some(sha))
    })
}

/// Check the source before removing it. The complete verified published
/// model must already have a persisted manifest in the running journal.
pub fn remove_originals(item: &Value) -> Result<()> {
    let entries = item["source_manifest"].as_array().ok_or_else(|| anyhow!("Missing recovery manifest."))?;
    if !valid_originals(entries) {
        bail!("Source files changed during transfer; originals were kept where possible.");
    }
    // A new, unreviewed source file must never be erased by removing its
    // parent directory after the checked copy is made.
    let current = source_manifest(item)?;
    if current != item["source_manifest"] {
        bail!("Source files were added or changed during transfer. Do not remove the originals.");
    }
    let files = strings(&item["files"]);
    if files.is_empty() {
        let path = Path::new(item["source"].as_str().unwrap_or(""));
        std::fs::remove_dir_all(path)?;
    } else {
        for file in files {
            let path = Path::new(&file);
            if path.is_dir() { std::fs::remove_dir_all(path)?; }
            else { std::fs::remove_file(path)?; }
        }
    }
    Ok(())
}

/// Undo a recovered Move even if the process stopped partway through deleting
/// its original folder: restore only missing files and refuse to overwrite
/// external changes. Check every destination hash before copying anything.
fn restore_originals(item: &Value, dest: &Path) -> Result<()> {
    let originals = item["source_manifest"].as_array().ok_or_else(|| anyhow!("Missing recovery manifest."))?;
    for entry in originals {
        let original = Path::new(entry["path"].as_str().unwrap_or(""));
        let sha = entry["sha256"].as_str().unwrap_or("");
        if original.exists() && digest(original).ok().as_deref() != Some(sha) {
            bail!("{} was changed outside the app. The library copy was kept.", original.display());
        }
        if original.exists() { continue; }
        let relative = entry["relative"].as_str().unwrap_or("");
        let published = dest.join(relative);
        if entry["original_sidecar"].is_string() {
            continue; // the original model.json was journalled as raw bytes
        }
        if !published.is_file() || digest(&published)? != sha {
            bail!("Cannot recover {} from the imported model; the library copy was kept.", relative);
        }
    }
    for entry in originals {
        let original = Path::new(entry["path"].as_str().unwrap_or(""));
        if original.exists() { continue; }
        std::fs::create_dir_all(original.parent().ok_or_else(|| anyhow!("Invalid source path."))?)?;
        if let Some(saved) = entry["original_sidecar"].as_str() {
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD.decode(saved)?;
            crate::config::write_atomic(original, &bytes)?;
        } else {
            let published = dest.join(entry["relative"].as_str().unwrap_or(""));
            // Copy to the source, not rename: leave the complete library model
            // in place until every recovered source hash is checked.
            std::fs::copy(&published, original)?;
        }
        if digest(original)? != entry["sha256"].as_str().unwrap_or("") {
            bail!("Recovery of {} could not be verified; the library copy was kept.", original.display());
        }
    }
    if !valid_originals(originals) { bail!("Some original files could not be recovered."); }
    Ok(())
}

/// A crash during publication can leave a partial directory with no published
/// hash. Only remove that directory when a complete verified source survives
/// and every copied data file is unchanged. The generated sidecar and preview
/// are allowed because they were produced by this exact pending operation.
fn remove_incomplete_copy(item: &Value, dest: &Path) -> Result<()> {
    let originals = item["source_manifest"].as_array().ok_or_else(|| anyhow!("Missing source manifest."))?;
    if !valid_originals(originals) {
        bail!("Source files are incomplete; the interrupted library copy was kept.");
    }
    let entries = destination_manifest(dest)?;
    for entry in entries.as_array().into_iter().flatten() {
        let rel = entry["path"].as_str().unwrap_or("");
        if rel == SIDECAR || rel == crate::thumb::THUMB { continue; }
        let expected = originals.iter().find(|e| e["relative"] == rel);
        if expected.is_none_or(|e| e["sha256"] != entry["sha256"]) {
            bail!("The interrupted destination contains changed files; it was kept for recovery.");
        }
    }
    std::fs::remove_dir_all(dest)?;
    Ok(())
}

/// A file's journal record is written *before* publication, and again after
/// the checked copy and before original cleanup. Recovery works on every state.
fn undo_item(lib: &Library, entry: &Value) -> Result<()> {
    let dest = lib.resolve(entry["to"].as_str().unwrap_or(""))?;
    let phase = entry["phase"].as_str().unwrap_or("planned");
    if !dest.exists() {
        let original = entry["source_manifest"].as_array().ok_or_else(|| anyhow!("Missing source manifest."))?;
        if !valid_originals(original) {
            bail!("The import destination is missing and the original is incomplete. Manual recovery is needed.");
        }
        return Ok(());
    }
    if !entry["manifest"].is_array() {
        return remove_incomplete_copy(entry, &dest);
    }
    if destination_manifest(&dest)? != entry["manifest"] {
        bail!("The imported model was changed after publication. It was kept for recovery.");
    }
    let originals = entry["source_manifest"].as_array().ok_or_else(|| anyhow!("Missing source manifest."))?;
    if entry["mode"] == "move" || matches!(phase, "cleaning" | "done") {
        restore_originals(entry, &dest)?;
    } else if !valid_originals(originals) {
        bail!("The original copy was changed or deleted; the library copy was kept.");
    }
    std::fs::remove_dir_all(&dest)?;
    Ok(())
}

/// Recovery is an Undo-only operation: no blind replay of potentially stale
/// inputs. Each successful item is marked and synced so Undo can itself resume
/// after a crash. New schemas are removed only after restoring every model.
pub fn undo_running(
    lib: &Library, mut journal: Value,
    cancel: &std::sync::atomic::AtomicBool,
    on_item: &dyn Fn(usize, usize, &str),
) -> Result<Value> {
    use std::sync::atomic::Ordering;
    let moves = journal["moves"].as_array().cloned().unwrap_or_default();
    let mut failed = vec![];
    let mut recovered = 0;
    for (idx, entry) in moves.iter().enumerate().rev() {
        if entry["phase"] == "undone" { continue; }
        if cancel.load(Ordering::Relaxed) {
            failed.push(json!({ "name": entry["name"], "error": "Stopped; recovery may be resumed." }));
            break;
        }
        on_item(moves.len() - idx - 1, moves.len(), entry["name"].as_str().unwrap_or(""));
        match undo_item(lib, entry) {
            Ok(()) => {
                recovered += 1;
                journal["moves"][idx]["phase"] = json!("undone");
                crate::relayout::write(lib, &journal)?;
            }
            Err(e) => failed.push(json!({ "name": entry["name"], "error": format!("{e:#}") })),
        }
    }
    if failed.is_empty() {
        // Categories may have nested subcategories. Only empty directories and
        // schemas belonging to this staged import can be removed.
        for made in journal["created_schemas"].as_array().into_iter().flatten() {
            let (Some(id), Some(folder)) = (made["id"].as_str(), made["folder"].as_str()) else { continue };
            let Some(s) = schema::list(lib).into_iter().find(|s| s.id == id && s.folder == folder) else { continue };
            let mut paths = s.tree();
            paths.sort_by_key(|p| std::cmp::Reverse(p.len()));
            for path in paths {
                let dir = path.iter().fold(lib.root().join(folder), |p, part| p.join(part));
                let _ = std::fs::remove_dir(dir);
            }
            let top = lib.root().join(folder);
            if !top.exists() || std::fs::remove_dir(&top).is_ok() { schema::remove(lib, id)?; }
            else { failed.push(json!({ "name": folder, "error": "Category has unexpected contents and was kept." })); }
        }
        for node in journal["added"].as_array().into_iter().flatten() {
            let (Some(id), Some(parts)) = (node["schema"].as_str(), node["path"].as_array()) else { continue };
            let path: Vec<String> = parts.iter().filter_map(Value::as_str).map(String::from).collect();
            let _ = schema::remove_subcategory(lib, id, &path);
        }
    }
    journal["state"] = json!(if failed.is_empty() { "undone" } else { "stopped" });
    if let Some(first) = failed.first() { journal["error"] = first["error"].clone(); }
    else if let Some(o) = journal.as_object_mut() { o.remove("error"); }
    crate::relayout::write(lib, &journal)?;
    Ok(json!({ "journal": journal["id"], "state": journal["state"], "moved": recovered,
        "failed": failed, "sort": [] }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> (Library, PathBuf, PathBuf) {
        let home = std::env::temp_dir().join(format!("modlib-category-import-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let lib = Library::open(home.join("Lib")).unwrap();
        let src = home.join("Collection");
        (lib, home, src)
    }
    fn put(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn read_only_scan_and_editable_nested_plan_preserve_files() {
        let (lib, home, src) = fixture("nested");
        put(&src.join("Terrain/Rock.stl"), b"stl");
        put(&src.join("Terrain/Rock.png"), b"picture");
        put(&src.join("Terrain/readme.anything"), b"unknown");
        put(&src.join("Terrain/Detail/body.stl"), b"body");
        put(&src.join("Terrain/Detail/photo.jpg"), b"picture 2");
        put(&src.join("Terrain/Detail/photos/another.jpg"), b"more");
        let ix = Index::build(&lib, None, false);
        let mut proposal = scan(&lib, std::slice::from_ref(&src)).unwrap();
        assert_eq!(proposal["roots"][0]["kind"], "category");
        assert_eq!(proposal["roots"][0]["children"][0]["kind"], "category");
        let detail = proposal["roots"][0]["children"][0]["children"].as_array().unwrap().iter()
            .find(|v| v["name"] == "Detail").unwrap();
        assert_eq!(detail["kind"], "model", "a recognisable model may have companion folders");
        let original = plan(&lib, &ix, &proposal).unwrap();
        assert_eq!(original["models"], 4, "{original}");
        assert_eq!(original["categories"][0]["name"], "Collection");
        assert!(original["items"].as_array().unwrap().iter().any(|i| i["name"] == "readme"));
        assert!(!lib.root().join("Collection").exists(), "planning may not create a category folder");
        assert!(schema::list(&lib).is_empty(), "planning may not write a category file");

        proposal["roots"][0]["children"][0]["group"] = json!(true);
        proposal["roots"][0]["children"][0]["group_name"] = json!("Terrain kit");
        let grouped = plan(&lib, &ix, &proposal).unwrap();
        assert_eq!(grouped["models"], 2, "{grouped}");
        assert_eq!(grouped["items"][0]["name"], "Terrain kit");
        assert_eq!(grouped["items"][0]["files"].as_array().unwrap().len(), 3);
        assert!(!lib.root().join("Collection").exists());
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn map_to_existing_requires_explicit_choice_and_detects_changed_sources() {
        let (lib, home, src) = fixture("existing");
        put(&src.join("Terrain/Rock.stl"), b"first");
        let existing = schema::create(&lib, &json!({ "name": "Collection" })).unwrap();
        let ix = Index::build(&lib, None, false);
        let mut proposal = scan(&lib, std::slice::from_ref(&src)).unwrap();
        let collision = plan(&lib, &ix, &proposal).unwrap();
        assert!(!collision["conflicts"].as_array().unwrap().is_empty(), "{collision}");
        proposal["roots"][0]["target"] = json!({ "schema": existing.id, "values": [] });
        let plan_ok = plan(&lib, &ix, &proposal).unwrap();
        assert!(plan_ok["conflicts"].as_array().unwrap().is_empty(), "{plan_ok}");
        assert!(plan_ok["categories"].as_array().unwrap().is_empty());
        // A missing file, added file or changed bytes invalidates the review.
        put(&src.join("Terrain/Rock.stl"), b"second");
        assert!(plan(&lib, &ix, &proposal).is_err());
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn exclusions_and_unsafe_symlinks_are_not_silent() {
        let (lib, home, src) = fixture("exclude");
        put(&src.join("Terrain/Rock.stl"), b"rock");
        put(&src.join("Terrain/Secret.abc"), b"secret");
        let ix = Index::build(&lib, None, false);
        let mut staged = scan(&lib, std::slice::from_ref(&src)).unwrap();
        staged["roots"][0]["children"][0]["kind"] = json!("category");
        for child in staged["roots"][0]["children"][0]["children"].as_array_mut().unwrap() {
            if child["name"] == "Secret.abc" { child["include"] = json!(false); }
        }
        let p = plan(&lib, &ix, &staged).unwrap();
        assert_eq!(p["models"], 1);
        assert!(!p["items"].as_array().unwrap().iter().any(|i| i["name"] == "Secret"));
        // A folder-as-model imports its entire contents; a staged exclusion may
        // not accidentally be ignored when classifying it as a model.
        staged["roots"][0]["children"][0]["kind"] = json!("model");
        assert!(plan(&lib, &ix, &staged).is_err());
        let _ = std::fs::remove_dir_all(home);
    }
}
