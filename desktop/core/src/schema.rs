//! Schemas (docs/PLAN.md, "Schemas and categories" and "Subcategory tree design"):
//! one file per schema (a category, in the app) in `_library/schemas/<id>.json`. A
//! schema names a top folder, the tree of subcategories below it (any depth), the
//! template for each model's folder name ("{name} ({author})") and the fields its
//! models have. Older schema files name fixed `levels` instead of a tree; they're
//! converted to a tree when the library can be written ([`upgrade`]).

use crate::config::{read_json_object, write_json};
use crate::library::{slug, Library, APP_DIR};
use anyhow::{bail, Result};
use serde_json::{json, Map, Value};
use std::path::PathBuf;

pub const FORMAT: u64 = 1;
/// Field types a schema can give its fields.
pub const FIELD_TYPES: [&str; 5] = ["text", "number", "choice", "yes-no", "date"];

#[derive(Clone, Debug)]
pub struct Schema {
    pub id: String,
    pub name: String,
    /// The schema's top folder, relative to the library ("Wargames").
    pub folder: String,
    /// Older schema files only: fixed level keys and labels, top first
    /// ([("game", "Game"), ("faction", "Faction")]). Empty for a tree.
    pub levels: Vec<(String, String)>,
    pub model_folder: String,
    /// The schema file as stored (fields, and anything a newer app added).
    pub raw: Value,
}

impl Schema {
    pub fn from_value(v: &Value) -> Option<Schema> {
        let id = v["id"].as_str()?.to_string();
        let levels = v["levels"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|l| {
                let key = l["key"].as_str()?.to_string();
                let label = l["label"]
                    .as_str()
                    .map(String::from)
                    .unwrap_or_else(|| key.clone());
                Some((key, label))
            })
            .collect();
        Some(Schema {
            name: v["name"].as_str().unwrap_or(&id).to_string(),
            folder: v["folder"].as_str().filter(|f| !f.is_empty())?.to_string(),
            model_folder: v["model_folder"].as_str().unwrap_or("{name}").to_string(),
            levels,
            id,
            raw: v.clone(),
        })
    }

    pub fn to_json(&self) -> Value {
        let mut v = self.raw.clone();
        v["id"] = json!(self.id);
        v["name"] = json!(self.name);
        v["folder"] = json!(self.folder);
        v["model_folder"] = json!(self.model_folder);
        if self.levels.is_empty() {
            if let Some(o) = v.as_object_mut() {
                o.shift_remove("levels");
            }
        } else {
            v["levels"] = json!(self
                .levels
                .iter()
                .map(|(k, l)| json!({ "key": k, "label": l }))
                .collect::<Vec<_>>());
        }
        v
    }

    /// Every subcategory's path, from the top (see [`subcategories`]).
    pub fn tree(&self) -> Vec<Vec<String>> {
        subcategories(&self.raw)
    }

    /// The schema's own fields: [{ key, label, type, choices? }].
    pub fn fields(&self) -> Vec<Value> {
        self.raw["fields"].as_array().cloned().unwrap_or_default()
    }

    /// Name and author from a model folder's name, by the template
    /// ("{name} ({author})": "Hive Tyrant (Jo Smith)" -> ("Hive Tyrant", Some("Jo Smith"))).
    pub fn parse_folder(&self, folder: &str) -> (String, Option<String>) {
        parse_folder_name(&self.model_folder, folder)
    }
}

/// Name and author from a folder name by a template with `{name}` and maybe `{author}`.
pub fn parse_folder_name(template: &str, folder: &str) -> (String, Option<String>) {
    let (Some(ni), Some(ai)) = (template.find("{name}"), template.find("{author}")) else {
        return (folder.to_string(), None);
    };
    // the literal text around the two placeholders
    let (first, second) = if ni < ai {
        ("{name}", "{author}")
    } else {
        ("{author}", "{name}")
    };
    let (pre, rest) = template.split_once(first).unwrap();
    let (mid, post) = rest.split_once(second).unwrap();
    let Some(inner) = folder.strip_prefix(pre).and_then(|f| f.strip_suffix(post)) else {
        return (folder.to_string(), None);
    };
    let Some((a, b)) = (if mid.is_empty() {
        None
    } else {
        inner.rsplit_once(mid)
    }) else {
        return (folder.to_string(), None);
    };
    let (name, author) = if ni < ai { (a, b) } else { (b, a) };
    if name.trim().is_empty() || author.trim().is_empty() {
        return (folder.to_string(), None);
    }
    (name.trim().to_string(), Some(author.trim().to_string()))
}

/// A folder name: Windows' forbidden characters replaced, no trailing dot or space,
/// not a reserved device name, at most `max` characters.
pub fn clean_folder_name(name: &str, max: usize) -> String {
    let mut s: String = name
        .chars()
        .map(|c| {
            if "<>:\"/\\|?*".contains(c) || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    s = s.chars().take(max).collect();
    let s = s.trim_end_matches(['.', ' ']).trim_start().to_string();
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    if s.is_empty() {
        "Untitled".into()
    } else if reserved {
        format!("{s}_")
    } else {
        s
    }
}

fn schemas_dir(lib: &Library) -> PathBuf {
    lib.root().join(APP_DIR).join("schemas")
}

/// Every schema in the library, by name.
pub fn list(lib: &Library) -> Vec<Schema> {
    let mut out: Vec<Schema> = std::fs::read_dir(schemas_dir(lib))
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
                .filter_map(|e| Schema::from_value(&read_json_object(&e.path())))
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|s| s.name.to_lowercase());
    out
}

/// Make a new schema from the form (name, folder, subcategories [{name,
/// subcategories}], model_folder, fields). `levels` [{label}] makes an older,
/// fixed-level schema (kept for reading older libraries in tests).
pub fn create(lib: &Library, spec: &Value) -> Result<Schema> {
    lib.writable()?;
    let name = spec["name"].as_str().unwrap_or("").trim().to_string();
    if name.is_empty() {
        bail!("A schema needs a name.");
    }
    let folder = clean_folder_name(
        spec["folder"]
            .as_str()
            .filter(|f| !f.trim().is_empty())
            .unwrap_or(&name),
        60,
    );
    if folder.starts_with('_') || folder.eq_ignore_ascii_case("Unsorted") {
        bail!("The folder {folder} is kept for the app; choose another.");
    }
    let existing = list(lib);
    if existing
        .iter()
        .any(|s| s.folder.eq_ignore_ascii_case(&folder))
    {
        bail!("Another schema already uses the folder {folder}.");
    }
    let mut levels: Vec<Value> = vec![];
    for l in spec["levels"].as_array().into_iter().flatten() {
        let label = l["label"]
            .as_str()
            .or(l.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if label.is_empty() {
            continue;
        }
        let key = unique_key(
            &slug(&label).replace('-', "_"),
            levels.iter().map(|l| l["key"].as_str().unwrap_or("")),
        );
        levels.push(json!({ "key": key, "label": label }));
    }
    let mut fields: Vec<Value> = vec![];
    for f in spec["fields"].as_array().into_iter().flatten() {
        let label = f["label"].as_str().unwrap_or("").trim().to_string();
        if label.is_empty() {
            continue;
        }
        let ty = f["type"]
            .as_str()
            .filter(|t| FIELD_TYPES.contains(t))
            .unwrap_or("text");
        let key = unique_key(
            &slug(&label).replace('-', "_"),
            fields.iter().map(|f| f["key"].as_str().unwrap_or("")),
        );
        let mut field = json!({ "key": key, "label": label, "type": ty });
        if ty == "choice" {
            let choices: Vec<String> = match &f["choices"] {
                Value::Array(a) => a
                    .iter()
                    .filter_map(|c| c.as_str())
                    .map(|c| c.trim().to_string())
                    .filter(|c| !c.is_empty())
                    .collect(),
                Value::String(s) => s
                    .split(',')
                    .map(|c| c.trim().to_string())
                    .filter(|c| !c.is_empty())
                    .collect(),
                _ => vec![],
            };
            field["choices"] = json!(choices);
        }
        fields.push(field);
    }
    let template = spec["model_folder"]
        .as_str()
        .map(str::trim)
        .filter(|t| t.contains("{name}"))
        .unwrap_or("{name} ({author})");
    let base = slug(&name);
    let mut id = if base.is_empty() {
        "schema".to_string()
    } else {
        base.clone()
    };
    let mut n = 2;
    while existing.iter().any(|s| s.id == id)
        || schemas_dir(lib).join(format!("{id}.json")).exists()
    {
        id = format!("{}-{n}", if base.is_empty() { "schema" } else { &base });
        n += 1;
    }
    let mut v = Map::new();
    v.insert("format".into(), json!(FORMAT));
    v.insert("id".into(), json!(id));
    v.insert("name".into(), json!(name));
    v.insert("folder".into(), json!(folder));
    if !levels.is_empty() {
        v.insert("levels".into(), json!(levels));
    }
    v.insert("model_folder".into(), json!(template));
    v.insert("fields".into(), json!(fields));
    v.insert("created".into(), json!(crate::library::now()));
    let mut v = Value::Object(v);
    let mut paths = vec![];
    tree_spec(&spec["subcategories"], &[], &mut paths, &mut vec![])?;
    for p in &paths {
        check_path(lib, &folder, &[], p)?;
    }
    set_subcategories(&mut v, &paths);
    write_json(&schemas_dir(lib).join(format!("{id}.json")), &v)?;
    std::fs::create_dir_all(lib.root().join(&folder))?;
    for p in subcategories(&v) {
        std::fs::create_dir_all(p.iter().fold(lib.root().join(&folder), |d, x| d.join(x)))?;
    }
    Ok(Schema::from_value(&v).unwrap())
}

/// A subcategory name as its folder will have it; refuses an empty one or one the
/// app keeps for itself.
pub fn subcategory_name(name: &str) -> Result<String> {
    if name.trim().is_empty() {
        bail!("Give every subcategory a name.");
    }
    let c = clean_folder_name(name.trim(), 80);
    if c.starts_with('_') {
        bail!("A subcategory can't start with _ (those folders are the app's): {c}.");
    }
    Ok(c)
}

/// Refuse a subcategory path that runs into a model's folder: one with a
/// model.json, or one with files that isn't a subcategory already (in `tree`).
pub fn check_path(
    lib: &Library,
    folder: &str,
    tree: &[Vec<String>],
    path: &[String],
) -> Result<()> {
    let mut dir = lib.root().join(folder);
    for i in 0..path.len() {
        dir.push(&path[i]);
        if !dir.is_dir() {
            return Ok(());
        }
        let known = tree
            .iter()
            .any(|p| p.len() == i + 1 && starts_with(p, &path[..=i]));
        let files = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.flatten()
                    .any(|e| e.file_type().is_ok_and(|t| t.is_file()))
            })
            .unwrap_or(false);
        if dir.join(crate::model::SIDECAR).is_file() || (!known && files) {
            bail!(
                "{} is a model's folder, not a subcategory.",
                path[..=i].join(" › ")
            );
        }
    }
    Ok(())
}

/// Whether `path` is in `tree` (names compared without case).
pub fn in_tree(tree: &[Vec<String>], path: &[String]) -> bool {
    tree.iter()
        .any(|p| p.len() == path.len() && starts_with(p, path))
}

/// The paths of a tree from the form ([{name, orig?, subcategories}]), with each
/// node that came from the library (`orig`: its path before) mapped to its path now.
fn tree_spec(
    nodes: &Value,
    parent: &[String],
    paths: &mut Vec<Vec<String>>,
    kept: &mut Vec<Remap>,
) -> Result<()> {
    for n in nodes.as_array().into_iter().flatten() {
        let mut path = parent.to_vec();
        path.push(subcategory_name(n["name"].as_str().unwrap_or(""))?);
        let orig = strings(&n["orig"]);
        if !orig.is_empty() {
            kept.push(Remap {
                from: orig,
                to: path.clone(),
                keep_below: true,
            });
        }
        paths.push(path.clone());
        tree_spec(&n["subcategories"], &path, paths, kept)?;
    }
    Ok(())
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(String::from)
        .collect()
}

/// Where a subcategory (and what's below it) goes in an edit: the longest `from`
/// that a path starts with decides; `keep_below` keeps the rest of the path under
/// `to` (a renamed or moved node), otherwise everything lands on `to` (a removed node).
#[derive(Clone, Debug, PartialEq)]
pub struct Remap {
    pub from: Vec<String>,
    pub to: Vec<String>,
    pub keep_below: bool,
}

/// Whether `path` starts with `prefix` (names compared without case).
pub fn starts_with(path: &[String], prefix: &[String]) -> bool {
    path.len() >= prefix.len()
        && path
            .iter()
            .zip(prefix)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

fn best<'a>(path: &[String], maps: &'a [Remap]) -> Option<&'a Remap> {
    maps.iter()
        .filter(|m| !m.from.is_empty() && starts_with(path, &m.from))
        .max_by_key(|m| m.from.len())
}

/// A path after an edit (unchanged when no `from` matches it).
pub fn remap(path: &[String], maps: &[Remap]) -> Vec<String> {
    let Some(m) = best(path, maps) else {
        return path.to_vec();
    };
    let mut out = m.to.clone();
    if m.keep_below {
        out.extend_from_slice(&path[m.from.len()..]);
    }
    out
}

// ------------------------------------------------------------ subcategories

/// Subcategories made in the app, kept in the schema file so they're listed (and
/// their folders kept) even with no models: every node's path of values.
/// Stored nested: "subcategories": [{"name": "A", "subcategories": [...]}].
pub fn subcategories(v: &Value) -> Vec<Vec<String>> {
    fn walk(nodes: &Value, prefix: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
        for n in nodes.as_array().into_iter().flatten() {
            let Some(name) = n["name"].as_str().filter(|s| !s.is_empty()) else {
                continue;
            };
            prefix.push(name.to_string());
            out.push(prefix.clone());
            walk(&n["subcategories"], prefix, out);
            prefix.pop();
        }
    }
    let mut out = vec![];
    walk(&v["subcategories"], &mut vec![], &mut out);
    out
}

/// Write the subcategory paths back, nested and sorted (every prefix of a path is kept too).
pub fn set_subcategories(v: &mut Value, paths: &[Vec<String>]) {
    fn insert(nodes: &mut Vec<Value>, path: &[String]) {
        let Some((first, rest)) = path.split_first() else {
            return;
        };
        let i = match nodes.iter().position(|n| {
            n["name"]
                .as_str()
                .is_some_and(|s| s.eq_ignore_ascii_case(first))
        }) {
            Some(i) => i,
            None => {
                nodes.push(json!({ "name": first, "subcategories": [] }));
                nodes.len() - 1
            }
        };
        let mut kids = nodes[i]["subcategories"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        insert(&mut kids, rest);
        nodes[i]["subcategories"] = json!(kids);
    }
    fn sort(nodes: &mut [Value]) {
        nodes.sort_by_key(|n| n["name"].as_str().unwrap_or("").to_lowercase());
        for n in nodes.iter_mut() {
            let mut kids = n["subcategories"].as_array().cloned().unwrap_or_default();
            sort(&mut kids);
            if kids.is_empty() {
                n.as_object_mut().unwrap().shift_remove("subcategories");
            } else {
                n["subcategories"] = json!(kids);
            }
        }
    }
    let mut nodes = vec![];
    for p in paths.iter().filter(|p| !p.is_empty()) {
        insert(&mut nodes, p);
    }
    sort(&mut nodes);
    if nodes.is_empty() {
        if let Some(o) = v.as_object_mut() {
            o.shift_remove("subcategories");
        }
    } else {
        v["subcategories"] = json!(nodes);
    }
}

/// Whether `dir` is a subcategory folder some schema keeps (not to be tidied away).
pub fn kept_folder(lib: &Library, dir: &std::path::Path) -> bool {
    list(lib).iter().any(|s| {
        subcategories(&s.raw).iter().any(|p| {
            let mut d = lib.root().join(&s.folder);
            for v in p {
                d.push(v);
            }
            d == dir
        })
    })
}

fn find(lib: &Library, id: &str) -> Result<Schema> {
    list(lib)
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| anyhow::anyhow!("There's no category {id} any more."))
}

/// The folders of an older, fixed-level schema that are category values (every
/// folder above its last level that isn't a model), as tree paths.
fn level_paths(lib: &Library, s: &Schema) -> Vec<Vec<String>> {
    fn walk(
        dir: &std::path::Path,
        depth: usize,
        levels: usize,
        prefix: &mut Vec<String>,
        out: &mut Vec<Vec<String>>,
    ) {
        if depth >= levels {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        let mut dirs: Vec<std::path::PathBuf> = rd
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter(|e| !e.file_name().to_string_lossy().starts_with(['_', '.']))
            .map(|e| e.path())
            .collect();
        dirs.sort();
        for d in dirs {
            if d.join(crate::model::SIDECAR).is_file() {
                continue;
            }
            prefix.push(d.file_name().unwrap().to_string_lossy().into_owned());
            out.push(prefix.clone());
            walk(&d, depth + 1, levels, prefix, out);
            prefix.pop();
        }
    }
    let mut out = vec![];
    walk(
        &lib.root().join(&s.folder),
        0,
        s.levels.len(),
        &mut vec![],
        &mut out,
    );
    out
}

/// The schema file of an older, fixed-level schema as a tree: its level folders
/// become subcategories and the levels go. Nothing on disk moves.
pub fn as_tree(lib: &Library, s: &Schema) -> Value {
    let mut v = s.to_json();
    if s.levels.is_empty() {
        return v;
    }
    let mut paths = s.tree();
    paths.extend(level_paths(lib, s));
    if let Some(o) = v.as_object_mut() {
        o.shift_remove("levels");
    }
    set_subcategories(&mut v, &paths);
    v
}

fn as_tree_paths(lib: &Library, s: &Schema) -> Vec<Vec<String>> {
    subcategories(&as_tree(lib, s))
}

/// Convert an older, fixed-level schema to a tree, when the library can be written.
pub fn upgrade(lib: &Library, s: Schema) -> Result<Schema> {
    if s.levels.is_empty() || lib.writable().is_err() {
        return Ok(s);
    }
    let v = as_tree(lib, &s);
    save(lib, &v)?;
    Schema::from_value(&v).ok_or_else(|| anyhow::anyhow!("That category isn't valid."))
}

/// Convert every older schema in a library that can be written; how many were.
pub fn upgrade_all(lib: &Library) -> usize {
    if lib.writable().is_err() {
        return 0;
    }
    list(lib)
        .into_iter()
        .filter(|s| !s.levels.is_empty())
        .filter(|s| upgrade(lib, s.clone()).is_ok())
        .count()
}

/// Make sure `path` (and every node above it) is in schema `id`'s tree, as a model
/// is placed there (an import, a move).
pub fn define_path(lib: &Library, id: &str, path: &[String]) -> Result<()> {
    let s = upgrade(lib, find(lib, id)?)?;
    let mut all = s.tree();
    if path.is_empty()
        || all
            .iter()
            .any(|p| p.len() == path.len() && starts_with(p, path))
    {
        return Ok(());
    }
    all.push(path.to_vec());
    let mut v = s.to_json();
    set_subcategories(&mut v, &all);
    save(lib, &v)
}

/// Add a subcategory under `path` (names from the top) as `name`; makes its folder.
pub fn add_subcategory(
    lib: &Library,
    id: &str,
    path: &[String],
    name: &str,
) -> Result<Vec<String>> {
    lib.writable()?;
    let s = find(lib, id)?;
    let mut full: Vec<String> = path.iter().map(|v| clean_folder_name(v, 80)).collect();
    full.push(subcategory_name(name)?);
    if !in_tree(&s.tree(), &full) {
        check_path(lib, &s.folder, &as_tree_paths(lib, &s), &full)?;
    }
    define_path(lib, id, &full)?;
    std::fs::create_dir_all(
        full.iter()
            .fold(lib.root().join(&s.folder), |d, x| d.join(x)),
    )?;
    Ok(full)
}

/// Remove an empty subcategory (and the ones below it): its folders go if they're empty.
pub fn remove_subcategory(lib: &Library, id: &str, path: &[String]) -> Result<()> {
    lib.writable()?;
    let s = upgrade(lib, find(lib, id)?)?;
    let mut v = s.to_json();
    let all: Vec<Vec<String>> = subcategories(&v)
        .into_iter()
        .filter(|p| !starts_with(p, path))
        .collect();
    set_subcategories(&mut v, &all);
    save(lib, &v)?;
    let mut dir = lib.root().join(&s.folder);
    for x in path {
        dir.push(x);
    }
    remove_empty_tree(&dir);
    Ok(())
}

/// Remove a folder and its sub-folders if there's nothing but empty folders in them.
pub fn remove_empty_tree(dir: &std::path::Path) -> bool {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return false;
    };
    let mut empty = true;
    for e in rd.flatten() {
        if !(e.path().is_dir() && remove_empty_tree(&e.path())) {
            empty = false;
        }
    }
    empty && std::fs::remove_dir(dir).is_ok()
}

/// The schema file after an edit from the form (name, folder, model_folder, fields
/// [{key?, label, type, choices}], and maybe the whole tree: subcategories [{name,
/// orig?, subcategories}] with removed [[orig path]]), and where every old path goes.
/// Keys of kept fields stay, so model.json values stay attached. An older schema
/// becomes a tree.
pub fn edited(lib: &Library, old: &Schema, spec: &Value) -> Result<(Value, Vec<Remap>)> {
    let name = spec["name"]
        .as_str()
        .unwrap_or(&old.name)
        .trim()
        .to_string();
    if name.is_empty() {
        bail!("A category needs a name.");
    }
    let folder = clean_folder_name(
        spec["folder"]
            .as_str()
            .filter(|f| !f.trim().is_empty())
            .unwrap_or(&old.folder),
        60,
    );
    if folder.starts_with('_') || folder.eq_ignore_ascii_case("Unsorted") {
        bail!("The folder {folder} is kept for the app; choose another.");
    }
    if list(lib)
        .iter()
        .any(|s| s.id != old.id && s.folder.eq_ignore_ascii_case(&folder))
    {
        bail!("Another category already uses the folder {folder}.");
    }
    let old_fields = old.fields();
    let mut fields: Vec<Value> = vec![];
    for f in spec["fields"].as_array().into_iter().flatten() {
        let label = f["label"].as_str().unwrap_or("").trim().to_string();
        if label.is_empty() {
            continue;
        }
        let ty = f["type"]
            .as_str()
            .filter(|t| FIELD_TYPES.contains(t))
            .unwrap_or("text");
        let key = match f["key"].as_str().filter(|k| {
            old_fields.iter().any(|o| o["key"] == json!(k))
                && !fields.iter().any(|x| x["key"] == json!(k))
        }) {
            Some(k) => k.to_string(),
            None => unique_key(
                &slug(&label).replace('-', "_"),
                fields
                    .iter()
                    .map(|f| f["key"].as_str().unwrap_or(""))
                    .chain(old_fields.iter().filter_map(|f| f["key"].as_str())),
            ),
        };
        let mut field = json!({ "key": key, "label": label, "type": ty });
        if ty == "choice" {
            field["choices"] = json!(choices(&f["choices"]));
        }
        fields.push(field);
    }
    let template = spec["model_folder"]
        .as_str()
        .map(str::trim)
        .filter(|t| t.contains("{name}"))
        .unwrap_or(&old.model_folder);
    let mut v = as_tree(lib, old);
    let old_tree = subcategories(&v);
    let mut maps = vec![];
    if spec.get("subcategories").is_some() {
        let mut paths = vec![];
        tree_spec(&spec["subcategories"], &[], &mut paths, &mut maps)?;
        // a removed node's models go up to the nearest node that's kept
        let removed: Vec<Vec<String>> = spec["removed"]
            .as_array()
            .into_iter()
            .flatten()
            .map(strings)
            .filter(|p| !p.is_empty())
            .collect();
        for r in &removed {
            let up = (0..r.len())
                .rev()
                .map(|n| &r[..n])
                .find_map(|a| {
                    maps.iter()
                        .find(|m| m.from.len() == a.len() && starts_with(&m.from, a))
                        .map(|m| m.to.clone())
                })
                .unwrap_or_default();
            maps.push(Remap {
                from: r.clone(),
                to: up,
                keep_below: false,
            });
        }
        // nodes the form didn't know of follow the node above them
        for p in &old_tree {
            if best(p, &maps).is_none_or(|m| m.keep_below) {
                paths.push(remap(p, &maps));
            }
        }
        let known: &[Vec<String>] = if folder == old.folder { &old_tree } else { &[] };
        for p in paths.iter().filter(|p| !in_tree(&old_tree, p)) {
            check_path(lib, &folder, known, p)?;
        }
        set_subcategories(&mut v, &paths);
    }
    v["name"] = json!(name);
    v["folder"] = json!(folder);
    v["model_folder"] = json!(template);
    v["fields"] = json!(fields);
    v["updated"] = json!(crate::library::now());
    Ok((v, maps))
}

fn choices(v: &Value) -> Vec<String> {
    match v {
        Value::Array(a) => a
            .iter()
            .filter_map(|c| c.as_str())
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
            .collect(),
        Value::String(s) => s
            .split(',')
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
            .collect(),
        _ => vec![],
    }
}

/// Write a schema file as it is (a re-layout's before or after).
pub fn save(lib: &Library, v: &Value) -> Result<()> {
    let id = v["id"].as_str().unwrap_or("");
    crate::library::valid_id(id)?;
    write_json(&schemas_dir(lib).join(format!("{id}.json")), v)
}

/// Remove a schema file (its folders are left to the caller).
pub fn remove(lib: &Library, id: &str) -> Result<()> {
    crate::library::valid_id(id)?;
    let p = schemas_dir(lib).join(format!("{id}.json"));
    if p.exists() {
        std::fs::remove_file(p)?;
    }
    Ok(())
}

fn unique_key<'a>(base: &str, taken: impl Iterator<Item = &'a str> + Clone) -> String {
    let base = if base.is_empty() { "level" } else { base };
    let mut key = base.to_string();
    let mut n = 2;
    while taken.clone().any(|t| t == key) {
        key = format!("{base}_{n}");
        n += 1;
    }
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_names_parse_by_template() {
        let t = "{name} ({author})";
        assert_eq!(
            parse_folder_name(t, "Hive Tyrant (Jo Smith)"),
            ("Hive Tyrant".into(), Some("Jo Smith".into()))
        );
        assert_eq!(
            parse_folder_name(t, "Carnifex (v2) (Jo)"),
            ("Carnifex (v2)".into(), Some("Jo".into()))
        );
        assert_eq!(
            parse_folder_name(t, "Hive Tyrant"),
            ("Hive Tyrant".into(), None)
        );
        assert_eq!(
            parse_folder_name("{author} - {name}", "Jo - Tyrant"),
            ("Tyrant".into(), Some("Jo".into()))
        );
        assert_eq!(
            parse_folder_name("{name}", "Tyrant (Jo)"),
            ("Tyrant (Jo)".into(), None)
        );
    }

    #[test]
    fn folder_names_are_cleaned() {
        assert_eq!(clean_folder_name("A: B/C?  ", 60), "A- B-C-");
        assert_eq!(clean_folder_name("dots...", 60), "dots");
        assert_eq!(clean_folder_name("CON", 60), "CON_");
        assert_eq!(clean_folder_name("com1.txt", 60), "com1.txt_");
        assert_eq!(clean_folder_name("", 60), "Untitled");
        assert_eq!(clean_folder_name("abcdef", 3), "abc");
    }

    #[test]
    fn paths_follow_an_edit_of_the_tree() {
        let v = |x: &[&str]| x.iter().map(|s| s.to_string()).collect::<Vec<String>>();
        // Office removed, but Desk moved out of it first
        let maps = [
            Remap {
                from: v(&["Office", "Desk"]),
                to: v(&["Desk"]),
                keep_below: true,
            },
            Remap {
                from: v(&["office"]),
                to: vec![],
                keep_below: false,
            },
        ];
        assert_eq!(
            remap(&v(&["Office", "Desk", "Lamps"]), &maps),
            v(&["Desk", "Lamps"])
        );
        assert_eq!(
            remap(&v(&["Office", "Pens", "Blue"]), &maps),
            Vec::<String>::new()
        );
        assert_eq!(remap(&v(&["Kitchen"]), &maps), v(&["Kitchen"]));
    }

    #[test]
    fn schemas_are_created_and_listed() {
        let dir = std::env::temp_dir().join(format!("modlib-schema-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let lib = Library::open(&dir).unwrap();
        let s = create(&lib, &json!({
            "name": "Wargames", "levels": [{ "label": "Game" }, { "label": "Faction" }, { "label": "" }],
            "fields": [{ "label": "Scale", "type": "choice", "choices": "28mm, 32mm" }, { "label": "Base size" }, { "label": "Presupported", "type": "yes-no" }]
        }))
        .unwrap();
        assert_eq!(s.id, "wargames");
        assert_eq!(s.folder, "Wargames");
        assert_eq!(
            s.levels,
            vec![
                ("game".into(), "Game".into()),
                ("faction".into(), "Faction".into())
            ]
        );
        assert_eq!(s.model_folder, "{name} ({author})");
        let fields = s.fields();
        assert_eq!(fields[0]["choices"], json!(["28mm", "32mm"]));
        assert_eq!(fields[1]["key"], "base_size");
        assert!(dir.join("Wargames").is_dir());
        assert!(create(&lib, &json!({ "name": "Other", "folder": "wargames" })).is_err());
        assert!(create(&lib, &json!({ "name": "X", "folder": "Unsorted" })).is_err());
        assert!(create(&lib, &json!({ "name": " " })).is_err());
        let s2 = create(&lib, &json!({ "name": "Wargames", "folder": "Wargames 2" })).unwrap();
        assert_eq!(s2.id, "wargames-2");
        assert_eq!(list(&lib).len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
