//! Planned category-tree restructuring. No category or model is changed by plan().
 //! The resulting journal is executed by relayout, so its recovery and Undo rules
 //! are shared with imports and ordinary folder moves.
use crate::import::{destination, file_name, place_of};
use crate::index::{Index, Model};
use crate::library::{slug, Library};
use crate::schema::{self, Schema};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
struct Node {
    schema: String,
    path: Vec<String>,
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect()
}
fn node(v: &Value) -> Result<Node> {
    let schema = v["schema"].as_str().ok_or_else(|| anyhow!("Choose a category."))?.to_string();
    if schema.is_empty() { bail!("Unsorted isn't a category that can be merged or removed."); }
    Ok(Node { schema, path: strings(&v["path"]) })
}
fn same(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && schema::starts_with(a, b)
}
fn contains(tree: &[Vec<String>], path: &[String]) -> bool {
    tree.iter().any(|p| same(p, path))
}
fn parent(path: &[String]) -> Vec<String> { path[..path.len() - 1].to_vec() }
/// Reject links anywhere below the approved library root. A lexically relative
/// destination can otherwise escape through a linked category directory.
fn check_links(lib: &Library, path: &Path) -> Result<()> {
    let rel = path.strip_prefix(lib.root()).map_err(|_| anyhow!("Path is outside the library."))?;
    let mut dir = lib.root().to_path_buf();
    for segment in rel.components() {
        dir.push(segment.as_os_str());
        if let Ok(info) = std::fs::symlink_metadata(&dir) {
            if info.file_type().is_symlink() {
                bail!("Linked folder {} cannot be used as a category destination.", dir.display());
            }
        }
    }
    Ok(())
}

/// Windows regards case-only differences as path collisions. Resolve them now
/// even when planning on a case-sensitive system, so libraries stay portable.
fn clashes_case_insensitive(path: &Path, own: &Path, taken: &HashSet<PathBuf>) -> bool {
    let filename = path.file_name().unwrap_or_default().to_string_lossy();
    taken.iter().any(|p| p.to_string_lossy().eq_ignore_ascii_case(&path.to_string_lossy()))
        || path.parent().and_then(|p| std::fs::read_dir(p).ok()).is_some_and(|rd| {
            rd.flatten().any(|e| {
                e.path() != own && e.file_name().to_string_lossy().eq_ignore_ascii_case(&filename)
            })
        })
}

fn model_destination(
    lib: &Library, schema: Option<&Schema>, path: &[String],
    name: &str, src: &Path, taken: &HashSet<PathBuf>,
) -> Result<PathBuf> {
    for n in 1..=1000 {
        let label = if n == 1 { name.to_string() } else { format!("{name} ({n})") };
        let dest = destination(lib, schema, path, &label, Some(src), taken)?;
        check_links(lib, &dest)?;
        if !clashes_case_insensitive(&dest, src, taken) { return Ok(dest); }
    }
    bail!("Too many model folder name collisions. Choose another destination.")
}
fn on_node(model: &Model, source: &Node) -> bool {
    model.v["schema"].as_str() == Some(source.schema.as_str())
        && schema::starts_with(&strings(&model.v["path"]), &source.path)
}

/// Ensure no user files, unindexed folders or links are hidden inside category
/// containers slated for removal. A model's files are owned by its model folder.
fn owned_contents(lib: &Library, ix: &Index, source: &Node, old: &Schema) -> Result<(u64, u64)> {
    let tree = schema::subcategories(&schema::as_tree(lib, old));
    let dir = source.path.iter().fold(lib.root().join(&old.folder), |p, n| p.join(n));
    fn files(path: &Path) -> Result<(u64, u64)> {
        let mut counts = (0, 0);
        for e in std::fs::read_dir(path)? {
            let e = e?;
            let ty = e.file_type()?;
            if ty.is_symlink() { bail!("A linked file or folder at {} needs to be handled separately.", e.path().display()); }
            if ty.is_dir() {
                let (n, b) = files(&e.path())?;
                counts.0 += n;
                counts.1 += b;
            } else if ty.is_file() {
                counts.0 += 1;
                counts.1 += e.metadata()?.len();
            } else { bail!("Unsupported filesystem entry at {}.", e.path().display()); }
        }
        Ok(counts)
    }
    fn walk(lib: &Library, ix: &Index, old: &Schema, tree: &[Vec<String>], at: &[String], dir: &Path) -> Result<(u64, u64)> {
        let mut counts = (0, 0);
        for e in std::fs::read_dir(dir)? {
            let e = e?;
            let ty = e.file_type()?;
            if ty.is_symlink() { bail!("A linked file or folder at {} needs to be handled separately.", e.path().display()); }
            let name = e.file_name().to_string_lossy().to_string();
            let mut child = at.to_vec();
            child.push(name);
            if !ty.is_dir() {
                bail!("{} contains a file outside any model. Move it into a model before restructuring.", dir.display());
            }
            if contains(tree, &child) {
                let (n, b) = walk(lib, ix, old, tree, &child, &e.path())?;
                counts.0 += n;
                counts.1 += b;
            } else if ix.models.iter().any(|m| {
                m.v["schema"].as_str() == Some(old.id.as_str())
                    && lib.resolve(m.rel()).is_ok_and(|p| p == e.path())
            }) {
                let (n, b) = files(&e.path())?;
                counts.0 += n;
                counts.1 += b;
            } else {
                bail!("Unindexed folder {} must be reviewed before restructuring.", e.path().display());
            }
        }
        Ok(counts)
    }
    if !dir.is_dir() || dir.symlink_metadata()?.file_type().is_symlink() {
        bail!("The source category folder is missing or linked: {}.", dir.display());
    }
    walk(lib, ix, old, &tree, &source.path, &dir)
}

/// Validate node references and proposed paths without a filesystem write. Also
/// usable with a proposed tree (before its folders exist) by passing known paths.
pub fn validate_tree(paths: &[Vec<String>]) -> Result<()> {
    for (i, p) in paths.iter().enumerate() {
        if p.is_empty() { bail!("A subcategory must have a name."); }
        for n in p { schema::subcategory_name(n)?; }
        if paths[..i].iter().any(|q| same(q, p)) {
            bail!("Two subcategories map to {}. Choose a different name or an explicit merge.", p.join(" › "));
        }
        if p.len() > 1 && !paths.iter().any(|q| same(q, &parent(p))) {
            bail!("{} has no parent in the proposed hierarchy.", p.join(" › "));
        }
    }
    Ok(())
}

/// Change shapes:
/// {kind:"restructure", operation:"remove-up"|"remove-unsorted", source:{schema,path}}
/// {kind:"restructure", operation:"merge", sources:[{schema,path},...],
///  target:{schema,path} OR {schema,parent,name} OR {name},
///  child_conflicts:"merge"|"rename"|"cancel"}.
/// Empty source paths name a whole category (merge only). An empty existing
/// target path names an existing category; a new top-level target names a new one.
pub fn plan(lib: &Library, ix: &Index, change: &Value) -> Result<Value> {
    lib.writable()?;
    let operation = change["operation"].as_str().unwrap_or("");
    let merging = operation == "merge";
    if !merging && operation != "remove-up" && operation != "remove-unsorted" {
        bail!("Unknown category restructuring operation.");
    }
    let sources: Vec<Node> = if merging {
        change["sources"].as_array().into_iter().flatten().map(node).collect::<Result<_>>()?
    } else { vec![node(&change["source"])?] };
    if (merging && sources.len() < 2) || sources.is_empty() {
        bail!("Choose at least two categories or subcategories to merge.");
    }
    let mut old: BTreeMap<String, Value> = BTreeMap::new();
    for sc in &ix.schemas { old.insert(sc.id.clone(), schema::as_tree(lib, sc)); }
    for s in &sources {
        let v = old.get(&s.schema).ok_or_else(|| anyhow!("The source category no longer exists."))?;
        if !s.path.is_empty() && !contains(&schema::subcategories(v), &s.path) {
            bail!("The source subcategory {} no longer exists.", s.path.join(" › "));
        }
        if !merging && s.path.is_empty() { bail!("Use Delete category for top-level categories."); }
    }
    for (i, a) in sources.iter().enumerate() {
        for b in &sources[..i] {
            if a.schema == b.schema
                && (schema::starts_with(&a.path, &b.path) || schema::starts_with(&b.path, &a.path)) {
                bail!("Don't merge a category with one of its own descendants.");
            }
        }
    }
    let mut sizes = (0_u64, 0_u64);
    for s in &sources {
        let sc = ix.schema(&s.schema).ok_or_else(|| anyhow!("Missing category."))?;
        let (n, bytes) = owned_contents(lib, ix, s, sc)?;
        sizes.0 += n;
        sizes.1 += bytes;
    }
    let mut after = old.clone();
    let mut target_id = sources[0].schema.clone();
    let mut target_path: Vec<String> = vec![];
    let mut mapped: Vec<(String, Vec<String>, Vec<String>)> = vec![];
    let mut node_collisions = 0_u64;
    let mut nodes = 0_u64;
    if merging {
        let to = &change["target"];
        let new_top = to["schema"].is_null();
        if new_top {
            let name = to["name"].as_str().unwrap_or("").trim();
            if name.is_empty() { bail!("Name the new target category."); }
            let folder = schema::clean_folder_name(name, 60);
            if folder.eq_ignore_ascii_case("Unsorted") || folder.starts_with('_')
                || ix.schemas.iter().any(|s| s.folder.eq_ignore_ascii_case(&folder))
                || lib.root().join(&folder).exists() {
                bail!("The new category folder {} already exists or is protected.", folder);
            }
            let base = slug(name);
            target_id = if base.is_empty() { "category".into() } else { base };
            let original_id = target_id.clone();
            let mut n = 2;
            while after.contains_key(&target_id) {
                target_id = format!("{original_id}-{n}");
                n += 1;
            }
            after.insert(target_id.clone(), json!({"format": schema::FORMAT, "id": target_id,
                "name": name, "folder": folder, "model_folder": "{name} ({author})",
                "fields": [], "subcategories": [], "created": crate::library::now()}));
        } else {
            target_id = to["schema"].as_str().ok_or_else(|| anyhow!("Choose the target category."))?.to_string();
            let v = after.get(&target_id).ok_or_else(|| anyhow!("Target category no longer exists."))?;
            let existing = schema::subcategories(v);
            if let Some(name) = to["name"].as_str() {
                let name = schema::subcategory_name(name)?;
                let mut p = strings(&to["parent"]);
                if !p.is_empty() && !contains(&existing, &p) {
                    bail!("The new target's parent doesn't exist.");
                }
                p.push(name);
                if contains(&existing, &p) { bail!("Target already exists; select it instead."); }
                let target_folder = p.iter().fold(lib.root().join(v["folder"].as_str().unwrap_or("")), |d, n| d.join(n));
                if target_folder.exists() { bail!("A folder already occupies the proposed new target; choose or rename it explicitly."); }
                schema::check_path(lib, v["folder"].as_str().unwrap_or(""), &existing, &p)?;
                check_links(lib, &target_folder)?;
                target_path = p;
                let mut new_paths = existing;
                new_paths.push(target_path.clone());
                validate_tree(&new_paths)?;
                schema::set_subcategories(after.get_mut(&target_id).unwrap(), &new_paths);
            } else {
                target_path = strings(&to["path"]);
                if !target_path.is_empty() && !contains(&existing, &target_path) {
                    bail!("Choose an existing target subcategory.");
                }
                let target_folder = target_path.iter().fold(lib.root().join(v["folder"].as_str().unwrap_or("")), |d, n| d.join(n));
                check_links(lib, &target_folder)?;
            }
        }
        for s in &sources {
            if s.schema == target_id && schema::starts_with(&target_path, &s.path)
                && !same(&target_path, &s.path) {
                bail!("A merge target can't be inside a source subtree.");
            }
        }
    } else if operation == "remove-up" {
        target_path = parent(&sources[0].path);
    }
    // Remove selected sources other than a chosen existing target. That target
    // remains intact, keeping both its identity and existing child hierarchy.
    for s in &sources {
        if merging && s.schema == target_id && same(&s.path, &target_path) { continue; }
        let v = after.get_mut(&s.schema).unwrap();
        let tree = schema::subcategories(v);
        nodes += tree.iter().filter(|p| schema::starts_with(p, &s.path)).count() as u64;
        if s.path.is_empty() {
            *v = Value::Null;
        } else {
            let paths: Vec<Vec<String>> = tree.into_iter().filter(|p| !schema::starts_with(p, &s.path)).collect();
            schema::set_subcategories(v, &paths);
        }
    }
    if merging {
        let policy = change["child_conflicts"].as_str().unwrap_or("cancel");
        if !matches!(policy, "cancel" | "merge" | "rename") { bail!("Unknown child-name conflict choice."); }
        for s in &sources {
            if s.schema == target_id && same(&s.path, &target_path) { continue; }
            let mut branches: Vec<Vec<String>> = schema::subcategories(&old[&s.schema])
                .into_iter().filter(|p| p.len() > s.path.len() && schema::starts_with(p, &s.path)).collect();
            branches.sort_by(|a, b| a.len().cmp(&b.len()).then(a.cmp(b)));
            let mut local = vec![(s.path.clone(), target_path.clone())];
            for p in branches {
                let base = local.iter().find(|(from, _)| same(from, &parent(&p)))
                    .map(|(_, to)| to.clone()).ok_or_else(|| anyhow!("An old subcategory's parent is missing."))?;
                let mut wanted = base;
                wanted.push(p.last().unwrap().clone());
                let mut paths = schema::subcategories(&after[&target_id]);
                if contains(&paths, &wanted) {
                    node_collisions += 1;
                    match policy {
                        "merge" => {}
                        "rename" => {
                            let stem = wanted.last().unwrap().clone();
                            let mut n = 2;
                            loop {
                                *wanted.last_mut().unwrap() = schema::subcategory_name(&format!("{stem} ({n})"))?;
                                if !contains(&paths, &wanted)
                                    && !wanted.iter().fold(lib.root().join(after[&target_id]["folder"].as_str().unwrap_or("")), |p, n| p.join(n)).exists() {
                                    break;
                                }
                                n += 1;
                            }
                            paths.push(wanted.clone());
                        }
                        _ => bail!("Child subcategory {} already exists in the target. Choose Merge matching names or Rename incoming children.", wanted.join(" › ")),
                    }
                } else { paths.push(wanted.clone()); }
                validate_tree(&paths)?;
                schema::set_subcategories(after.get_mut(&target_id).unwrap(), &paths);
                local.push((p, wanted));
            }
            mapped.extend(local.into_iter().map(|(from, to)| (s.schema.clone(), from, to)));
        }
    } else if operation == "remove-up" {
        let s = &sources[0];
        let mut paths = schema::subcategories(&after[&s.schema]);
        let mut branches: Vec<Vec<String>> = schema::subcategories(&old[&s.schema])
            .into_iter().filter(|p| p.len() > s.path.len() && schema::starts_with(p, &s.path)).collect();
        branches.sort_by_key(Vec::len);
        for p in branches {
            let mut new_path = target_path.clone();
            new_path.extend_from_slice(&p[s.path.len()..]);
            if contains(&paths, &new_path) {
                bail!("Subcategory {} already exists one level up. Rename or merge it before removing its parent.", new_path.join(" › "));
            }
            paths.push(new_path.clone());
            mapped.push((s.schema.clone(), p, new_path));
        }
        validate_tree(&paths)?;
        schema::set_subcategories(after.get_mut(&s.schema).unwrap(), &paths);
        mapped.push((s.schema.clone(), s.path.clone(), target_path.clone()));
    }
    let node_mappings: Vec<Value> = if operation == "remove-unsorted" {
        vec![json!({ "from": format!("{} › {}", sources[0].schema, sources[0].path.join(" › ")), "to": "Unsorted" })]
    } else {
        mapped.iter().map(|(sid, from, to)| json!({
            "from": format!("{} › {}", sid, from.join(" › ")),
            "to": format!("{} › {}", target_id, to.join(" › "))
        })).collect()
    };
    let mut moves = vec![];
    let mut taken: HashSet<PathBuf> = HashSet::new();
    for m in &ix.models {
        let Some(s) = sources.iter().find(|s| on_node(m, s)) else { continue };
        if merging && s.schema == target_id && same(&s.path, &target_path) { continue; }
        let model_path = strings(&m.v["path"]);
        let (sid, values) = if operation == "remove-unsorted" {
            (None, vec![])
        } else if merging {
            let (_, from, to) = mapped.iter().filter(|(sid, p, _)| sid == &s.schema && schema::starts_with(&model_path, p))
                .max_by_key(|(_, p, _)| p.len()).ok_or_else(|| anyhow!("Missing model relocation mapping."))?;
            let mut values = to.clone();
            values.extend_from_slice(&model_path[from.len()..]);
            (Some(target_id.as_str()), values)
        } else {
            let mut values = target_path.clone();
            values.extend_from_slice(&model_path[s.path.len()..]);
            (Some(target_id.as_str()), values)
        };
        let dest_schema = sid.and_then(|id| Schema::from_value(&after[id]));
        let src = lib.resolve(m.rel())?;
        let dest = model_destination(lib, dest_schema.as_ref(), &values, &file_name(&src), &src, &taken)?;
        taken.insert(dest.clone());
        let dest_rel = lib.relative(&dest).ok_or_else(|| anyhow!("Destination is outside the library."))?;
        let actual_path = place_of(lib, dest_schema.as_ref(), &dest);
        moves.push(json!({ "id": m.id(), "keep_id": true, "name": m.v["name"],
            "authors": m.v["authors"], "from": m.rel(), "to": dest_rel,
            "schema_before": m.v["schema"], "path_before": model_path,
            "schema_after": sid, "path_after": actual_path }));
    }
    let mut schemas = vec![];
    for (id, before) in &old {
        if after.get(id) != Some(before) {
            schemas.push(json!({ "id": id, "before": before, "after": after.get(id).unwrap_or(&Value::Null) }));
        }
    }
    for (id, v) in &after {
        if !old.contains_key(id) {
            schemas.push(json!({ "id": id, "before": null, "after": v }));
        }
    }
    let label = if merging {
        format!("Merged {} categories or subcategories", sources.len())
    } else if operation == "remove-up" {
        format!("Removed {} (contents moved up one level)", sources[0].path.join(" › "))
    } else {
        format!("Removed {} (models moved to Unsorted)", sources[0].path.join(" › "))
    };
    Ok(json!({ "kind": "restructure", "label": label,
        "schema": target_id, "schema_before": null, "schema_after": null,
        "operation": operation, "schemas": schemas, "moves": moves, "nodes": nodes,
        "files": sizes.0, "bytes": sizes.1, "child_collisions": node_collisions,
        "target_path": target_path, "node_mappings": node_mappings }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::tests::temp_dir;
    use crate::{model, relayout};
    use std::sync::atomic::AtomicBool;

    fn file(root: &Path, rel: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"solid sample").unwrap();
    }
    fn execute(lib: &Library, c: Value) -> String {
        let ix = Index::build(lib, None, true);
        let p = plan(lib, &ix, &c).unwrap();
        let id = relayout::start(lib, &p).unwrap();
        let r = relayout::apply(lib, &id, &AtomicBool::new(false), &|_,_,_|{}).unwrap();
        assert_eq!(r["state"], "done", "{r}");
        id
    }
    #[test]
    fn remove_moves_children_up_and_undo_restores_tree_and_ids() {
        let root = temp_dir("remove-up");
        let lib = Library::open(&root).unwrap();
        schema::create(&lib, &json!({"name":"Items", "subcategories":[
            {"name":"A","subcategories":[{"name":"B","subcategories":[{"name":"C"}]}]},
            {"name":"Other"}]})).unwrap();
        file(&root, "Items/A/B/Direct/x.stl");
        file(&root, "Items/A/B/C/Deep/y.stl");
        let before = Index::build(&lib,None,true);
        let ids: Vec<_> = before.models.iter().map(|m|m.id().to_string()).collect();
        let c = json!({"operation":"remove-up", "source":{"schema":"items","path":["A","B"]}});
        let id = execute(&lib,c);
        assert!(root.join("Items/A/Direct/x.stl").exists() && root.join("Items/A/C/Deep/y.stl").exists());
        assert!(schema::list(&lib)[0].tree().iter().any(|p|p==&vec!["A".to_string(),"C".to_string()]));
        let after = Index::build(&lib,None,true);
        assert_eq!(after.models.iter().map(|m|m.id()).collect::<HashSet<_>>(),
            ids.iter().map(String::as_str).collect::<HashSet<_>>());
        relayout::undo(&lib,&id,&AtomicBool::new(false),&|_,_,_|{}).unwrap();
        assert!(root.join("Items/A/B/C/Deep/y.stl").exists());
        assert_eq!(model::read_sidecar(&root.join("Items/A/B/C/Deep"))["path"], json!(["A","B","C"]));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn unsorted_removal_preserves_descendants_and_rejects_unknown_files() {
        let root = temp_dir("remove-unsorted");
        let lib = Library::open(&root).unwrap();
        schema::create(&lib,&json!({"name":"Items","subcategories":[{"name":"A","subcategories":[{"name":"B"}]}]})).unwrap();
        file(&root,"Items/A/B/Deep/y.stl");
        std::fs::write(root.join("Items/A/B/loose.txt"),"keep").unwrap();
        let c = json!({"operation":"remove-unsorted","source":{"schema":"items","path":["A"]}});
        assert!(plan(&lib,&Index::build(&lib,None,true),&c).is_err());
        std::fs::remove_file(root.join("Items/A/B/loose.txt")).unwrap();
        let id = execute(&lib,c);
        assert!(root.join("Unsorted/Deep/y.stl").exists());
        assert!(!root.join("Items/A").exists());
        relayout::undo(&lib,&id,&AtomicBool::new(false),&|_,_,_|{}).unwrap();
        assert!(root.join("Items/A/B/Deep/y.stl").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn cross_category_merge_requires_explicit_collision_choice() {
        let root = temp_dir("merge-tree");
        let lib = Library::open(&root).unwrap();
        for n in ["One","Two","Three"] {
            schema::create(&lib,&json!({"name":n,"subcategories":[{"name":"Same"}]})).unwrap();
            file(&root,&format!("{n}/Same/Model_{n}/part.stl"));
        }
        let base = json!({"operation":"merge","sources":[
            {"schema":"one","path":[]},{"schema":"two","path":[]}],
            "target":{"schema":"three","path":[]}});
        assert!(plan(&lib,&Index::build(&lib,None,true),&base).is_err());
        let mut merging = base.clone();
        merging["child_conflicts"]=json!("rename");
        let id = execute(&lib,merging);
        assert!(root.join("Three/Same (2)/Model_One/part.stl").exists());
        assert!(root.join("Three/Same (3)/Model_Two/part.stl").exists());
        assert_eq!(schema::list(&lib).len(),1);
        relayout::undo(&lib,&id,&AtomicBool::new(false),&|_,_,_|{}).unwrap();
        assert_eq!(schema::list(&lib).len(),3);
        assert!(root.join("One/Same/Model_One/part.stl").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn merges_into_selected_existing_category_without_replacing_its_identity() {
        let root = temp_dir("selected-target-merge");
        let lib = Library::open(&root).unwrap();
        schema::create(&lib,&json!({"name":"One","subcategories":[{"name":"Inside"}]})).unwrap();
        schema::create(&lib,&json!({"name":"Two","subcategories":[]})).unwrap();
        file(&root,"One/Inside/Existing/ex.stl");
        file(&root,"Two/Incoming/in.stl");
        let before = Index::build(&lib,None,true);
        let existing = before.models.iter().find(|m| m.rel().contains("Existing")).unwrap().id().to_string();
        let id = execute(&lib,json!({"operation":"merge","sources":[
            {"schema":"one","path":[]},{"schema":"two","path":[]}],
            "target":{"schema":"one","path":[]}}));
        assert!(root.join("One/Inside/Existing/ex.stl").is_file());
        assert!(root.join("One/Incoming/in.stl").is_file());
        assert_eq!(schema::list(&lib).len(),1);
        assert_eq!(schema::list(&lib)[0].id,"one");
        assert!(Index::build(&lib,None,true).models.iter().any(|m|m.id()==existing));
        relayout::undo(&lib,&id,&AtomicBool::new(false),&|_,_,_|{}).unwrap();
        assert!(root.join("Two/Incoming/in.stl").is_file());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn reject_cycles_and_ancestor_sources_without_mutation() {
        let root=temp_dir("bad-restructure");
        let lib=Library::open(&root).unwrap();
        schema::create(&lib,&json!({"name":"One","subcategories":[{"name":"Parent","subcategories":[{"name":"Child"}]}]})).unwrap();
        let ix=Index::build(&lib,None,true);
        assert!(plan(&lib,&ix,&json!({"operation":"merge","sources":[
            {"schema":"one","path":["Parent"]},{"schema":"one","path":["Parent","Child"]}],
            "target":{"schema":"one","path":[]}})).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
