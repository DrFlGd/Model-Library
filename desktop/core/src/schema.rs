//! Schemas (docs/PLAN.md, "Schemas and categories"): one file per schema in
//! `_library/schemas/<id>.json`. A schema names a top folder, the category levels
//! below it ("Game", "Faction"), the template for each model's folder name
//! ("{name} ({author})") and the fields its models have.

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
    /// Level keys and labels, top first: [("game", "Game"), ("faction", "Faction")].
    pub levels: Vec<(String, String)>,
    pub model_folder: String,
    /// The schema file as stored (fields, and anything a newer app added).
    pub raw: Value,
}

impl Schema {
    pub fn from_value(v: &Value) -> Option<Schema> {
        let id = v["id"].as_str()?.to_string();
        let levels = v["levels"]
            .as_array()?
            .iter()
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
        v["levels"] = json!(self
            .levels
            .iter()
            .map(|(k, l)| json!({ "key": k, "label": l }))
            .collect::<Vec<_>>());
        v
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

/// Make a new schema from the form (name, folder, levels [{label}], model_folder, fields).
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
    v.insert("levels".into(), json!(levels));
    v.insert("model_folder".into(), json!(template));
    v.insert("fields".into(), json!(fields));
    v.insert("created".into(), json!(crate::library::now()));
    let v = Value::Object(v);
    write_json(&schemas_dir(lib).join(format!("{id}.json")), &v)?;
    std::fs::create_dir_all(lib.root().join(&folder))?;
    Ok(Schema::from_value(&v).unwrap())
}

/// Where a level of an edited schema takes its values from: an old level (by
/// index), or a value given for every model already there (a new level).
#[derive(Clone, Debug, PartialEq)]
pub enum LevelFrom {
    Old(usize),
    New(String),
}

/// The schema file after an edit from the form (name, folder, levels
/// [{key?, label, value?}], model_folder, fields [{key?, label, type, choices}]),
/// and where each new level's values come from. Keys of kept levels and fields
/// stay, so model.json values stay attached.
pub fn edited(lib: &Library, old: &Schema, spec: &Value) -> Result<(Value, Vec<LevelFrom>)> {
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
    let mut levels: Vec<Value> = vec![];
    let mut from = vec![];
    for l in spec["levels"].as_array().into_iter().flatten() {
        let label = l["label"].as_str().unwrap_or("").trim().to_string();
        if label.is_empty() {
            continue;
        }
        let kept = l["key"]
            .as_str()
            .and_then(|k| old.levels.iter().position(|(ok, _)| ok == k));
        let key = match kept {
            Some(i) if !levels.iter().any(|x| x["key"] == json!(old.levels[i].0)) => {
                from.push(LevelFrom::Old(i));
                old.levels[i].0.clone()
            }
            _ => {
                let value = l["value"].as_str().unwrap_or("").trim().to_string();
                if value.is_empty() {
                    bail!("Give the new level {label} a value for the models already there.");
                }
                from.push(LevelFrom::New(value));
                unique_key(
                    &slug(&label).replace('-', "_"),
                    levels
                        .iter()
                        .map(|l| l["key"].as_str().unwrap_or(""))
                        .chain(old.levels.iter().map(|(k, _)| k.as_str())),
                )
            }
        };
        levels.push(json!({ "key": key, "label": label }));
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
    let mut v = old.to_json();
    v["name"] = json!(name);
    v["folder"] = json!(folder);
    v["levels"] = json!(levels);
    v["model_folder"] = json!(template);
    v["fields"] = json!(fields);
    v["updated"] = json!(crate::library::now());
    Ok((v, from))
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
