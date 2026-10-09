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
