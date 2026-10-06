//! The library's index (docs/PLAN.md, "Phase 1 design"): every model folder found
//! under the schemas' folders and `Unsorted/`, with its details, category and file
//! summary, searched in memory. It's cached in the app's data folder on each
//! computer (never in the library), keyed by the library's id, so opening the
//! library again rereads only the models whose folder or model.json changed.

use crate::library::Library;
use crate::model::{self, SIDECAR};
use crate::schema::{self, Schema};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub const UNSORTED: &str = "Unsorted";
const CACHE_FORMAT: u64 = 1;

#[derive(Clone, Debug)]
pub struct Model {
    /// What the page gets: id, rel, schema, category, name, authors, tags, files…
    pub v: Value,
    json_mtime: u64,
    dir_mtime: u64,
    /// Folded words for search.
    words: Vec<String>,
}

impl Model {
    pub fn id(&self) -> &str {
        self.v["id"].as_str().unwrap_or("")
    }
    pub fn rel(&self) -> &str {
        self.v["rel"].as_str().unwrap_or("")
    }
}

pub struct Index {
    /// The library folder it was read from.
    pub root: PathBuf,
    pub schemas: Vec<Schema>,
    pub models: Vec<Model>,
    by_id: HashMap<String, usize>,
    pub ms: u128,
    /// Models read from their folders this time (the rest came from the cache).
    pub read: usize,
}

fn mtime(p: &Path) -> u64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Lowercase, common accents dropped: "Café Größe" -> "cafe grosse".
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => out.push('a'),
            'ç' | 'č' | 'ć' => out.push('c'),
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' => out.push('e'),
            'ì' | 'í' | 'î' | 'ï' => out.push('i'),
            'ñ' | 'ń' => out.push('n'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' => out.push('o'),
            'ù' | 'ú' | 'û' | 'ü' | 'ū' => out.push('u'),
            'ý' | 'ÿ' => out.push('y'),
            'ß' => out.push_str("ss"),
            'ž' | 'ź' | 'ż' => out.push('z'),
            'š' | 'ś' => out.push('s'),
            'ł' => out.push('l'),
            _ => out.push(c),
        }
    }
    out
}

fn words_of(s: &str) -> impl Iterator<Item = String> + '_ {
    fold(s)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect::<Vec<_>>()
        .into_iter()
}

/// A model folder as the index keeps it.
fn read_model(lib: &Library, dir: &Path, schema: Option<&Schema>, cats: &[String]) -> Model {
    let rel = lib.relative(dir).unwrap_or_default();
    let side = model::read_sidecar(dir);
    let folder_name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (parsed_name, parsed_author) = match schema {
        Some(s) => s.parse_folder(&folder_name),
        None => schema::parse_folder_name("{name} ({author})", &folder_name),
    };
    let files = model::list_files(dir);
    let summary = model::summarise(&files, &side);
    let id = side["id"]
        .as_str()
        .map(String::from)
        .unwrap_or_else(|| derived_id(&rel));
    let authors: Vec<String> = match side["authors"].as_array() {
        Some(a) => a
            .iter()
            .filter_map(|x| x["name"].as_str().or(x.as_str()))
            .map(String::from)
            .collect(),
        None => parsed_author.into_iter().collect(),
    };
    let mut category = Map::new();
    if let Some(s) = schema {
        for ((key, _), value) in s.levels.iter().zip(cats) {
            category.insert(key.clone(), json!(value));
        }
    }
    let dir_mtime = mtime(dir);
    let v = json!({
        "id": id,
        "rel": rel,
        "folder": folder_name,
        "schema": schema.map(|s| s.id.clone()),
        "category": category,
        "path": cats,
        "name": side["name"].as_str().map(String::from).unwrap_or(parsed_name),
        "authors": authors,
        "tags": side["tags"].as_array().cloned().unwrap_or_default(),
        "released": side["released"],
        "source": side["source"],
        "license": side["license"],
        "fields": side["fields"].as_object().cloned().unwrap_or_default(),
        "added": side["added"].as_str().map(String::from).unwrap_or_else(|| crate::library::iso_from_unix((dir_mtime / 1000) as i64)),
        "sidecar": side.get("id").is_some(),
        "files": summary,
    });
    let mut words: Vec<String> = vec![];
    let mut add = |s: &str| words.extend(words_of(s));
    for k in ["name", "folder"] {
        add(v[k].as_str().unwrap_or(""));
    }
    for a in &authors {
        add(a);
    }
    for t in v["tags"].as_array().into_iter().flatten() {
        add(t.as_str().unwrap_or(""));
    }
    for c in cats {
        add(c);
    }
    for f in v["fields"].as_object().into_iter().flatten() {
        if let Some(s) = f.1.as_str() {
            add(s);
        }
    }
    for (f, _) in &files {
        add(f.rsplit('/').next().unwrap_or(f));
    }
    if let Some(s) = schema {
        add(&s.name);
    }
    words.sort();
    words.dedup();
    Model {
        v,
        json_mtime: mtime(&dir.join(SIDECAR)),
        dir_mtime,
        words,
    }
}

/// The id of a model folder that has no model.json yet: from its path.
pub fn derived_id(rel: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("p{}", &hex::encode(Sha256::digest(rel.as_bytes()))[..15])
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .filter(|e| !e.file_name().to_string_lossy().starts_with(['_', '.']))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// Every model folder: (folder, schema, category values).
fn find_models(lib: &Library, schemas: &[Schema]) -> Vec<(PathBuf, Option<usize>, Vec<String>)> {
    fn under(
        dir: &Path,
        si: usize,
        depth: usize,
        levels: usize,
        cats: &mut Vec<String>,
        out: &mut Vec<(PathBuf, Option<usize>, Vec<String>)>,
    ) {
        for d in subdirs(dir) {
            if depth >= levels || d.join(SIDECAR).is_file() {
                out.push((d, Some(si), cats.clone()));
            } else {
                cats.push(d.file_name().unwrap().to_string_lossy().into_owned());
                under(&d, si, depth + 1, levels, cats, out);
                cats.pop();
            }
        }
    }
    let mut out = vec![];
    for (si, s) in schemas.iter().enumerate() {
        under(
            &lib.root().join(&s.folder),
            si,
            0,
            s.levels.len(),
            &mut vec![],
            &mut out,
        );
    }
    for d in subdirs(&lib.root().join(UNSORTED)) {
        out.push((d, None, vec![]));
    }
    out
}

impl Index {
    /// Read the library. With `cache` (a path in the app's data folder), models whose
    /// folder and model.json haven't changed come from it, and the result is saved there.
    pub fn build(lib: &Library, cache: Option<&Path>, full: bool) -> Index {
        let t = std::time::Instant::now();
        let schemas = schema::list(lib);
        let mut cached: HashMap<String, Model> = HashMap::new();
        if let (Some(c), false) = (cache, full) {
            if let Ok(v) = serde_json::from_slice::<Value>(&std::fs::read(c).unwrap_or_default()) {
                if v["format"] == CACHE_FORMAT {
                    for m in v["models"].as_array().into_iter().flatten() {
                        let words = m["w"]
                            .as_str()
                            .unwrap_or("")
                            .split(' ')
                            .map(String::from)
                            .collect();
                        let model = Model {
                            v: m["v"].clone(),
                            json_mtime: m["j"].as_u64().unwrap_or(0),
                            dir_mtime: m["d"].as_u64().unwrap_or(0),
                            words,
                        };
                        cached.insert(model.rel().to_string(), model);
                    }
                }
            }
        }
        let mut read = 0;
        let mut models = vec![];
        for (dir, si, cats) in find_models(lib, &schemas) {
            let schema = si.map(|i| &schemas[i]);
            let rel = lib.relative(&dir).unwrap_or_default();
            let fresh = cached.remove(&rel).filter(|m| {
                m.dir_mtime == mtime(&dir)
                    && m.json_mtime == mtime(&dir.join(SIDECAR))
                    && m.v["schema"] == json!(schema.map(|s| &s.id))
                    && m.v["path"] == json!(cats)
            });
            models.push(match fresh {
                Some(m) => m,
                None => {
                    read += 1;
                    read_model(lib, &dir, schema, &cats)
                }
            });
        }
        let mut ix = Index {
            root: lib.root().to_path_buf(),
            schemas,
            models,
            by_id: HashMap::new(),
            ms: 0,
            read,
        };
        ix.reindex();
        if let Some(c) = cache {
            if let Err(e) = ix.save(c) {
                eprintln!("index cache: {e:#}");
            }
        }
        ix.ms = t.elapsed().as_millis();
        ix
    }

    fn reindex(&mut self) {
        self.by_id = self
            .models
            .iter()
            .enumerate()
            .map(|(i, m)| (m.id().to_string(), i))
            .collect();
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let models: Vec<Value> = self.models.iter().map(|m| json!({ "v": m.v, "j": m.json_mtime, "d": m.dir_mtime, "w": m.words.join(" ") })).collect();
        let bytes = serde_json::to_vec(&json!({ "format": CACHE_FORMAT, "models": models }))?;
        crate::config::write_atomic(path, &bytes)
    }

    pub fn get(&self, id: &str) -> Option<&Model> {
        self.by_id.get(id).map(|&i| &self.models[i])
    }

    pub fn schema(&self, id: &str) -> Option<&Schema> {
        self.schemas.iter().find(|s| s.id == id)
    }

    /// Read one model again (after its details changed). Its id may change (a first
    /// model.json gives it a real one); returns the new entry.
    pub fn refresh(&mut self, lib: &Library, old_id: &str) -> Option<Value> {
        let i = *self.by_id.get(old_id)?;
        let m = &self.models[i];
        let dir = lib.resolve(m.rel()).ok()?;
        let cats: Vec<String> = serde_json::from_value(m.v["path"].clone()).unwrap_or_default();
        let schema = m.v["schema"].as_str().and_then(|s| self.schema(s));
        let fresh = read_model(lib, &dir, schema, &cats);
        self.models[i] = fresh;
        self.reindex();
        Some(self.models[i].v.clone())
    }

    /// Counts and the category tree of each schema.
    pub fn overview(&self) -> Value {
        let schemas: Vec<Value> = self
            .schemas
            .iter()
            .map(|s| {
                let mut tree = Tree::default();
                let mut count = 0;
                for m in self.models.iter().filter(|m| m.v["schema"] == json!(s.id)) {
                    count += 1;
                    let path: Vec<String> =
                        serde_json::from_value(m.v["path"].clone()).unwrap_or_default();
                    tree.add(&path);
                }
                let mut v = s.to_json();
                v["count"] = json!(count);
                v["tree"] = tree.to_json();
                v
            })
            .collect();
        let unsorted = self
            .models
            .iter()
            .filter(|m| m.v["schema"].is_null())
            .count();
        json!({ "schemas": schemas, "all": self.models.len(), "unsorted": unsorted, "ms": self.ms as u64 })
    }

    /// Search one place of the library. `scope`: all | unsorted | favs | schema:<id>[/<value>…];
    /// `q`: words and typed filters (author:, tag:, schema:, kind:, a level key:).
    pub fn query(
        &self,
        scope: &str,
        q: &str,
        sort: &str,
        offset: usize,
        limit: usize,
        favs: &[String],
    ) -> Value {
        let t = std::time::Instant::now();
        let (place, sub) = scope.split_once(':').unwrap_or((scope, ""));
        let mut sub_path = sub.split('/');
        let schema_id = sub_path.next().unwrap_or("");
        let sub_path: Vec<String> = sub_path
            .filter(|p| !p.is_empty())
            .map(|p| fold(&crate::api::percent_decode(p)))
            .collect();
        let (words, filters) = parse_query(q);
        let mut hits: Vec<&Model> = self
            .models
            .iter()
            .filter(|m| match place {
                "unsorted" => m.v["schema"].is_null(),
                "favs" => favs.iter().any(|f| f == m.id()),
                "schema" => {
                    m.v["schema"] == json!(schema_id)
                        && sub_path.len() <= m.v["path"].as_array().map_or(0, Vec::len)
                        && sub_path
                            .iter()
                            .zip(m.v["path"].as_array().into_iter().flatten())
                            .all(|(want, have)| fold(have.as_str().unwrap_or("")) == *want)
                }
                _ => true,
            })
            .filter(|m| {
                words
                    .iter()
                    .all(|w| m.words.iter().any(|mw| mw.starts_with(w.as_str())))
            })
            .filter(|m| filters.iter().all(|(k, v)| self.matches_filter(m, k, v)))
            .collect();
        match sort {
            "added" => hits.sort_by(|a, b| b.v["added"].as_str().cmp(&a.v["added"].as_str())),
            "size" => {
                hits.sort_by_key(|m| std::cmp::Reverse(m.v["files"]["bytes"].as_u64().unwrap_or(0)))
            }
            _ => hits.sort_by_cached_key(|m| fold(m.v["name"].as_str().unwrap_or(""))),
        }
        let mut authors: HashMap<&str, usize> = HashMap::new();
        let mut tags: HashMap<&str, usize> = HashMap::new();
        for m in &hits {
            for a in m.v["authors"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                *authors.entry(a).or_default() += 1;
            }
            for t in m.v["tags"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                *tags.entry(t).or_default() += 1;
            }
        }
        let top = |m: HashMap<&str, usize>| {
            let mut v: Vec<(&str, usize)> = m.into_iter().collect();
            v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
            v.into_iter()
                .take(30)
                .map(|(k, n)| json!({ "value": k, "count": n }))
                .collect::<Vec<_>>()
        };
        let total = hits.len();
        let items: Vec<&Value> = hits.iter().skip(offset).take(limit).map(|m| &m.v).collect();
        json!({ "total": total, "items": items, "facets": { "authors": top(authors), "tags": top(tags) }, "us": t.elapsed().as_micros() as u64 })
    }

    fn matches_filter(&self, m: &Model, key: &str, want: &str) -> bool {
        let has = |vals: &mut dyn Iterator<Item = &str>| {
            vals.map(fold).any(|v| v == want || v.starts_with(want))
        };
        match key {
            "author" => has(&mut m.v["authors"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)),
            "tag" => has(&mut m.v["tags"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)),
            "schema" => m.v["schema"].as_str().is_some_and(|s| {
                fold(s) == want
                    || self
                        .schema(s)
                        .is_some_and(|sc| fold(&sc.name).starts_with(want))
            }),
            "kind" => m.v["files"]["kinds"].get(want).is_some(),
            other => {
                m.v["category"]
                    .get(other)
                    .and_then(Value::as_str)
                    .is_some_and(|v| fold(v).starts_with(want))
                    || m.v["fields"].get(other).is_some_and(|v| {
                        fold(
                            &v.as_str()
                                .map(String::from)
                                .unwrap_or_else(|| v.to_string()),
                        )
                        .starts_with(want)
                    })
            }
        }
    }
}

/// Words and `key:value` filters (values may be quoted: `faction:"blood angels"`), folded.
pub fn parse_query(q: &str) -> (Vec<String>, Vec<(String, String)>) {
    let mut words = vec![];
    let mut filters = vec![];
    let mut tokens = vec![];
    let mut cur = String::new();
    let mut quoted = false;
    for c in q.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    tokens.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    for t in tokens {
        match t.split_once(':') {
            Some((k, v))
                if !k.is_empty()
                    && !v.is_empty()
                    && k.chars().all(|c| c.is_alphanumeric() || c == '_') =>
            {
                filters.push((fold(k), fold(v.trim())))
            }
            _ => words.extend(words_of(&t)),
        }
    }
    (words, filters)
}

#[derive(Default)]
struct Tree {
    count: usize,
    children: Vec<(String, Tree)>,
}

impl Tree {
    fn add(&mut self, path: &[String]) {
        self.count += 1;
        if let Some((first, rest)) = path.split_first() {
            let i = match self.children.iter().position(|(v, _)| v == first) {
                Some(i) => i,
                None => {
                    self.children.push((first.clone(), Tree::default()));
                    self.children.len() - 1
                }
            };
            self.children[i].1.add(rest);
        }
    }
    fn to_json(&self) -> Value {
        let mut kids: Vec<&(String, Tree)> = self.children.iter().collect();
        kids.sort_by_key(|(v, _)| fold(v));
        json!(kids
            .iter()
            .map(|(v, t)| json!({ "value": v, "count": t.count, "children": t.to_json() }))
            .collect::<Vec<_>>())
    }
}

/// A library of `n` made-up models for timing (`modlib-cli make-test-library`):
/// one schema, Game > Faction, each model a folder with a model.json and two small files.
pub fn make_test_library(dir: &Path, n: usize) -> anyhow::Result<()> {
    let lib = Library::open(dir)?;
    if schema::list(&lib).is_empty() {
        schema::create(
            &lib,
            &json!({ "name": "Wargames", "levels": [{ "label": "Game" }, { "label": "Faction" }], "fields": [{ "label": "Scale", "type": "choice", "choices": "28mm, 32mm, 75mm" }] }),
        )?;
    }
    let games = [
        "Warhammer 40k",
        "Age of Sigmar",
        "Kill Team",
        "Necromunda",
        "Bolt Action",
    ];
    let factions = [
        "Tyranids",
        "Orks",
        "Eldar",
        "Necrons",
        "Space Marines",
        "Chaos",
        "Tau",
        "Imperial Guard",
    ];
    let nouns = [
        "Warrior", "Tyrant", "Walker", "Tank", "Hero", "Beast", "Squad", "Drone", "Lord", "Swarm",
    ];
    let authors = [
        "Jo Smith",
        "Artisan Guild",
        "Titan Forge",
        "Lost Kingdom",
        "Station Forge",
        "Dakka Forge",
    ];
    for i in 0..n {
        let (g, f) = (games[i % games.len()], factions[(i / 5) % factions.len()]);
        let name = format!(
            "{} {} {i}",
            nouns[i % nouns.len()],
            nouns[(i / 10) % nouns.len()]
        );
        let author = authors[(i / 3) % authors.len()];
        let d = dir
            .join("Wargames")
            .join(g)
            .join(f)
            .join(format!("{name} ({author})"));
        std::fs::create_dir_all(&d)?;
        std::fs::write(d.join(format!("{name}.stl")), b"solid x\nendsolid x\n")?;
        std::fs::write(d.join("readme.txt"), b"test")?;
        let (tag, scale) = (
            if i % 4 == 0 { "presupported" } else { "" },
            ["28mm", "32mm", "75mm"][i % 3],
        );
        model::update(
            &d,
            &json!({ "tags": tag, "fields": { "scale": scale } }),
            &json!({ "name": name, "authors": [{ "name": author }], "schema": "wargames" }),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("modlib-index-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn lib_with_models(dir: &Path) -> Library {
        let lib = Library::open(dir).unwrap();
        schema::create(
            &lib,
            &json!({ "name": "Wargames", "levels": [{ "label": "Game" }, { "label": "Faction" }] }),
        )
        .unwrap();
        let m = |rel: &str, files: &[&str]| {
            let d = dir.join(rel);
            std::fs::create_dir_all(&d).unwrap();
            for f in files {
                let p = d.join(f);
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(p, "x").unwrap();
            }
        };
        m(
            "Wargames/Warhammer 40k/Tyranid/Hive Tyrant (Jo Smith)",
            &["body.stl", "Arms/left.stl", "_media/cover.jpg"],
        );
        m(
            "Wargames/Warhammer 40k/Tyranid/Carnifex",
            &["carnifex.3mf", "carnifex.lys"],
        );
        m("Wargames/Warhammer 40k/Orks/Warboss (Dakka)", &["boss.stl"]);
        m(
            "Wargames/Age of Sigmar/Skaven/Rat Ogre (Jo Smith)",
            &["ogre.obj"],
        );
        m("Wargames/Kill Team/Loose Model", &["x.stl"]); // a model.json makes this folder a model above the usual depth
        std::fs::write(
            dir.join("Wargames/Kill Team/Loose Model/model.json"),
            r#"{"id":"mloose","name":"Loose","tags":["café"]}"#,
        )
        .unwrap();
        m("Wargames/_drafts/Not A Model", &["x.stl"]);
        m("Unsorted/Some Download", &["thing.zip"]);
        lib
    }

    #[test]
    fn finds_models_by_schema_depth_and_sidecars() {
        let dir = temp("find");
        let lib = lib_with_models(&dir);
        let ix = Index::build(&lib, None, false);
        let mut rels: Vec<&str> = ix.models.iter().map(Model::rel).collect();
        rels.sort();
        assert_eq!(
            rels,
            [
                "Unsorted/Some Download",
                "Wargames/Age of Sigmar/Skaven/Rat Ogre (Jo Smith)",
                "Wargames/Kill Team/Loose Model",
                "Wargames/Warhammer 40k/Orks/Warboss (Dakka)",
                "Wargames/Warhammer 40k/Tyranid/Carnifex",
                "Wargames/Warhammer 40k/Tyranid/Hive Tyrant (Jo Smith)",
            ]
        );
        let ht = ix
            .models
            .iter()
            .find(|m| m.rel().ends_with("(Jo Smith)") && m.rel().contains("Hive"))
            .unwrap();
        assert_eq!(ht.v["name"], "Hive Tyrant");
        assert_eq!(ht.v["authors"], json!(["Jo Smith"]));
        assert_eq!(
            ht.v["category"],
            json!({ "game": "Warhammer 40k", "faction": "Tyranid" })
        );
        assert_eq!(ht.v["files"]["kinds"], json!({ "model": 2, "image": 1 }));
        assert_eq!(ht.v["files"]["cover"], "_media/cover.jpg");
        assert_eq!(ht.v["sidecar"], false);
        assert!(ht.id().starts_with('p'));
        let loose = ix.get("mloose").unwrap();
        assert_eq!(loose.v["category"], json!({ "game": "Kill Team" }));
        let ov = ix.overview();
        assert_eq!(ov["all"], 6);
        assert_eq!(ov["unsorted"], 1);
        let tree = &ov["schemas"][0]["tree"];
        assert_eq!(tree[2]["value"], "Warhammer 40k");
        assert_eq!(tree[2]["count"], 3);
        assert_eq!(tree[2]["children"][1]["value"], "Tyranid");
        assert_eq!(tree[2]["children"][1]["count"], 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn searches_words_filters_and_places() {
        let dir = temp("search");
        let lib = lib_with_models(&dir);
        let ix = Index::build(&lib, None, false);
        let names = |r: Value| {
            r["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| m["name"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(ix.query("all", "tyrant", "name", 0, 50, &[])),
            ["Hive Tyrant"]
        );
        assert_eq!(
            names(ix.query("all", "author:jo", "name", 0, 50, &[])),
            ["Hive Tyrant", "Rat Ogre"]
        );
        assert_eq!(
            names(ix.query("all", "faction:tyranid kind:slicer", "name", 0, 50, &[])),
            ["Carnifex"]
        );
        assert_eq!(
            names(ix.query("all", "game:\"warhammer 40k\"", "name", 0, 50, &[])),
            ["Carnifex", "Hive Tyrant", "Warboss"]
        );
        assert_eq!(
            names(ix.query("all", "cafe", "name", 0, 50, &[])),
            ["Loose"]
        ); // accents folded
        assert_eq!(
            names(ix.query("all", "left", "name", 0, 50, &[])),
            ["Hive Tyrant"]
        ); // file names
        assert_eq!(
            names(ix.query(
                "schema:wargames/Warhammer%2040k/tyranid",
                "",
                "name",
                0,
                50,
                &[]
            )),
            ["Carnifex", "Hive Tyrant"]
        );
        assert_eq!(
            names(ix.query("unsorted", "", "name", 0, 50, &[])),
            ["Some Download"]
        );
        assert_eq!(
            names(ix.query("favs", "", "name", 0, 50, &["mloose".into()])),
            ["Loose"]
        );
        let r = ix.query("all", "", "name", 2, 2, &[]);
        assert_eq!(r["total"], 6);
        assert_eq!(names(r), ["Loose", "Rat Ogre"]);
        assert_eq!(
            ix.query("all", "", "name", 0, 50, &[])["facets"]["authors"][0],
            json!({ "value": "Jo Smith", "count": 2 })
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_rereads_only_what_changed() {
        let dir = temp("cache");
        let lib = lib_with_models(&dir);
        let cache = dir.with_extension("cache.json");
        let first = Index::build(&lib, Some(&cache), false);
        assert_eq!(first.read, 6);
        let again = Index::build(&lib, Some(&cache), false);
        assert_eq!(again.read, 0);
        assert_eq!(again.query("all", "tyrant", "name", 0, 5, &[])["total"], 1);
        std::thread::sleep(std::time::Duration::from_millis(20));
        let d = dir.join("Wargames/Warhammer 40k/Orks/Warboss (Dakka)");
        model::update(
            &d,
            &json!({ "tags": "boss" }),
            &json!({ "name": "Warboss" }),
        )
        .unwrap();
        let third = Index::build(&lib, Some(&cache), false);
        assert_eq!(third.read, 1);
        assert_eq!(
            third.query("all", "tag:boss", "name", 0, 5, &[])["total"],
            1
        );
        assert_eq!(Index::build(&lib, Some(&cache), true).read, 6);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&cache);
    }

    #[test]
    fn refresh_gives_a_new_sidecar_its_id() {
        let dir = temp("refresh");
        let lib = lib_with_models(&dir);
        let mut ix = Index::build(&lib, None, false);
        let m = ix
            .models
            .iter()
            .find(|m| m.rel().ends_with("Carnifex"))
            .unwrap();
        let old = m.id().to_string();
        model::update(
            &lib.resolve(m.rel()).unwrap(),
            &json!({ "authors": "Al" }),
            &json!({ "name": "Carnifex" }),
        )
        .unwrap();
        let v = ix.refresh(&lib, &old).unwrap();
        assert_ne!(v["id"].as_str().unwrap(), old);
        assert!(ix.get(&old).is_none());
        assert_eq!(
            ix.get(v["id"].as_str().unwrap()).unwrap().v["authors"],
            json!(["Al"])
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ten_thousand_models() {
        let dir = temp("big");
        make_test_library(&dir, 10_000).unwrap();
        let lib = Library::open(&dir).unwrap();
        let cache = dir.with_extension("cache.json");
        let t = std::time::Instant::now();
        let ix = Index::build(&lib, Some(&cache), false);
        let cold = t.elapsed();
        let t = std::time::Instant::now();
        let ix2 = Index::build(&lib, Some(&cache), false);
        let warm = t.elapsed();
        assert_eq!(ix.models.len(), 10_000);
        assert_eq!(ix2.read, 0);
        let r = ix2.query("all", "tyrant scale:32mm", "name", 0, 100, &[]);
        eprintln!(
            "10,000 models: cold {cold:?}, warm {warm:?}, search {} µs, {} hits",
            r["us"], r["total"]
        );
        assert!(r["total"].as_u64().unwrap() > 0);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&cache);
    }
}
