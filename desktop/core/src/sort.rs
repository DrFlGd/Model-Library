//! The sorting workspace (docs/PLAN.md, "Phase 5 design"): a folder tree read as
//! it is on disk, with the models the app proposes in it all the way down. The
//! owner sends them to categories, groups files and folders into one model, makes
//! a folder one model or splits one, and imports what's sorted, over more than
//! one sitting: the session is kept on this computer (the app's data folder, one
//! per library), and reading the folders again keeps what was decided.

use crate::import::{self, file_name, is_main, stem, Ctx, Found};
use crate::model::{file_kind, SIDECAR};
use crate::schema::clean_folder_name;
use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub const FORMAT: u64 = 1;

/// A folder or file added to the workspace. `contents`: its contents are sorted
/// (Sort a folder…); otherwise it's one model (Add a model folder…, Add a file…).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Root {
    pub path: String,
    pub contents: bool,
}

/// A folder that isn't a model: it's shown as it is, with what's in it.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Folder {
    pub path: String,
    pub name: String,
    pub parent: Option<String>,
}

/// A file that belongs to no model (shown greyed; it can be grouped into one).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Left {
    pub path: String,
    pub name: String,
    pub parent: String,
    pub size: u64,
    pub kind: String,
}

/// A proposed model.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Item {
    pub id: String,
    /// "folder" (a folder is the model), "files" (loose files that belong together)
    /// or "group" (what the owner grouped: files, and folders as its parts)
    pub kind: String,
    /// the folder, or the first file: what identifies it when the folders are read again
    pub path: String,
    /// the folder it's shown in (None for one added on its own)
    pub parent: Option<String>,
    /// what goes into its model folder
    pub sources: Vec<String>,
    pub name: String,
    pub author: String,
    pub tags: String,
    /// where it goes: a category (None: Unsorted) and subcategories, once `placed`
    pub schema: Option<String>,
    pub values: Vec<String>,
    pub placed: bool,
    pub skip: bool,
    /// once imported: { rel, id }
    pub done: Option<Value>,
    pub error: Option<String>,
    pub summary: Value,
    pub warnings: Vec<Value>,
    /// where its folder names (or its model.json) say it goes
    pub guess: Value,
    pub has_sidecar: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Session {
    pub format: u64,
    /// the library's id
    pub library: String,
    pub roots: Vec<Root>,
    pub folders: Vec<Folder>,
    pub items: Vec<Item>,
    pub left: Vec<Left>,
    /// folders the owner split (read as folders of models), and made one model
    pub split: Vec<String>,
    pub joined: Vec<String>,
    pub next: u64,
    /// when the folders were last read
    pub read: String,
}

fn s(p: &Path) -> String {
    p.display().to_string()
}

/// Folders a NAS or an OS keeps for itself, and files that are never a model's.
fn junk(name: &str) -> bool {
    name.starts_with('.')
        || matches!(
            name.to_lowercase().as_str(),
            "@eadir"
                | "#recycle"
                | "#snapshot"
                | "$recycle.bin"
                | "system volume information"
                | "__macosx"
                | "thumbs.db"
                | "desktop.ini"
        )
}

/// Sub-folders named for a kind of file: a folder holding only these (and
/// variants) is one model.
fn kind_folder(name: &str) -> bool {
    let n: String = name
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect();
    matches!(
        n.as_str(),
        "stl"
            | "stls"
            | "stlfiles"
            | "3mf"
            | "obj"
            | "step"
            | "files"
            | "models"
            | "parts"
            | "printfiles"
            | "images"
            | "image"
            | "pictures"
            | "pics"
            | "photos"
            | "renders"
            | "render"
            | "previews"
            | "preview"
            | "docs"
            | "documents"
            | "instructions"
            | "lychee"
            | "lys"
            | "chitubox"
            | "gcode"
            | "slicer"
            | "source"
            | "textures"
    )
}

fn words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}

/// Whether a folder name is a variant (as the page's isVariant: one of the
/// names, or holds one as whole words, case and dashes ignored).
pub fn is_variant(segment: &str, names: &[String]) -> bool {
    let fw = words(segment);
    names.iter().any(|n| {
        let tw = words(n);
        if tw.is_empty() {
            return false;
        }
        if fw.concat() == tw.concat() {
            return true;
        }
        fw.windows(tw.len()).any(|w| w == tw.as_slice())
    })
}

/// A folder's sub-folders and files, without junk and links.
fn entries(dir: &Path) -> (Vec<PathBuf>, Vec<(PathBuf, u64)>) {
    let (mut dirs, mut files) = (vec![], vec![]);
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if junk(&e.file_name().to_string_lossy()) {
                continue;
            }
            match e.file_type() {
                Ok(t) if t.is_symlink() => {}
                Ok(t) if t.is_dir() => dirs.push(e.path()),
                Ok(t) if t.is_file() => {
                    files.push((e.path(), e.metadata().map(|m| m.len()).unwrap_or(0)))
                }
                _ => {}
            }
        }
    }
    dirs.sort();
    files.sort();
    (dirs, files)
}

/// A folder name as a subcategory, or None when nothing's left of it.
fn as_subcategory(name: &str) -> Option<String> {
    let n = name.trim().trim_start_matches('_').trim();
    (!n.is_empty()).then(|| clean_folder_name(n, 80))
}

/// Reads folders into the session's lists.
struct Walker<'a> {
    ctx: &'a Ctx,
    variants: Vec<String>,
    split: HashSet<PathBuf>,
    joined: HashSet<PathBuf>,
    /// what kept items (groups, imported ones) hold: not proposed again
    claimed: HashSet<PathBuf>,
    /// folders with something claimed inside: read as folders, not models
    above_claimed: HashSet<PathBuf>,
    cancel: &'a AtomicBool,
    progress: &'a dyn Fn(usize, &str),
    read: usize,
    folders: Vec<Folder>,
    items: Vec<Item>,
    left: Vec<Left>,
}

impl<'a> Walker<'a> {
    fn new(
        session: &Session,
        ctx: &'a Ctx,
        cancel: &'a AtomicBool,
        progress: &'a dyn Fn(usize, &str),
    ) -> Self {
        let claimed: HashSet<PathBuf> = session
            .items
            .iter()
            .flat_map(|i| i.sources.iter().map(PathBuf::from))
            .collect();
        let above_claimed = claimed
            .iter()
            .flat_map(|c| c.ancestors().skip(1).map(Path::to_path_buf))
            .collect();
        Walker {
            ctx,
            variants: ctx.lib.variant_folders(),
            split: session.split.iter().map(PathBuf::from).collect(),
            joined: session.joined.iter().map(PathBuf::from).collect(),
            claimed,
            above_claimed,
            cancel,
            progress,
            read: 0,
            folders: vec![],
            items: vec![],
            left: vec![],
        }
    }

    fn stopped(&self) -> Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            bail!("Stopped.");
        }
        Ok(())
    }

    /// Whether a folder is a model by what's in it, the owner's choices aside: it
    /// has a model.json; or 3D, slicer or archive files of its own (its sub-folders
    /// are its parts), unless a sub-folder is plainly a model itself; or all its
    /// sub-folders are variants or named for a kind of file (STL, Images…).
    fn looks_like_model(&self, dirs: &[PathBuf], files: &[(PathBuf, u64)]) -> bool {
        if files.iter().any(|(f, _)| file_name(f) == SIDECAR) {
            return true;
        }
        if files.iter().any(|(f, _)| is_main(&file_name(f))) {
            return !dirs.iter().any(|d| {
                let (dd, ff) = entries(d);
                ff.iter().any(|(f, _)| file_name(f) == SIDECAR) || self.parts_by_kind(&dd)
            });
        }
        self.parts_by_kind(dirs)
    }

    /// Sub-folders that are all variants or named for a kind of file, one with 3D files.
    fn parts_by_kind(&self, dirs: &[PathBuf]) -> bool {
        !dirs.is_empty()
            && dirs.iter().all(|d| {
                let n = file_name(d);
                is_variant(&n, &self.variants) || kind_folder(&n)
            })
            && dirs
                .iter()
                .any(|d| entries(d).1.iter().any(|(f, _)| is_main(&file_name(f))))
    }

    fn is_model(&self, dir: &Path, dirs: &[PathBuf], files: &[(PathBuf, u64)]) -> bool {
        if self.joined.contains(dir) {
            return true;
        }
        if self.split.contains(dir) || self.above_claimed.contains(dir) {
            return false;
        }
        self.looks_like_model(dirs, files)
    }

    fn item(&mut self, kind: &str, f: Found, parent: Option<&Path>) {
        let d = import::describe(self.ctx, &f);
        let sources = if f.files.is_empty() {
            vec![s(&f.path)]
        } else {
            f.files.iter().map(|p| s(p)).collect()
        };
        let mut it = Item {
            kind: kind.into(),
            path: s(&f.path),
            parent: parent.map(s),
            sources,
            ..Default::default()
        };
        describe_into(&mut it, &d);
        if self.joined.contains(&f.path) {
            it.warnings.retain(|w| w["kind"] != "several");
        }
        // a model.json that says where it goes: that's where it goes
        if d["guess"]["sure"] == json!(true) {
            it.schema = d["guess"]["schema"].as_str().map(String::from);
            it.values = strings(&d["guess"]["values"]);
            it.placed = true;
        }
        self.items.push(it);
    }

    /// A sub-folder of `parent`: a model, or a folder read in turn. Returns how
    /// many things it adds.
    fn add_dir(&mut self, dir: &Path, parent: Option<&Path>, depth: usize) -> Result<usize> {
        self.stopped()?;
        let (dirs, files) = entries(dir);
        if self.is_model(dir, &dirs, &files) {
            self.item(
                "folder",
                Found {
                    path: dir.to_path_buf(),
                    files: vec![],
                },
                parent,
            );
            return Ok(1);
        }
        self.container(dir, parent, dirs, files, depth)
    }

    /// A folder of models: its sub-folders, then its loose files grouped by name.
    fn container(
        &mut self,
        dir: &Path,
        parent: Option<&Path>,
        dirs: Vec<PathBuf>,
        files: Vec<(PathBuf, u64)>,
        depth: usize,
    ) -> Result<usize> {
        self.stopped()?;
        self.read += 1;
        (self.progress)(self.read, &s(dir));
        let at = self.folders.len();
        self.folders.push(Folder {
            path: s(dir),
            name: file_name(dir),
            parent: parent.map(s),
        });
        let mut n = 0;
        if depth < 40 {
            for d in &dirs {
                if !self.claimed.contains(d) {
                    n += self.add_dir(d, Some(dir), depth + 1)?;
                }
            }
        }
        n += self.loose(dir, &files);
        // a folder with nothing in it isn't shown (one that was added is)
        if n == 0 && parent.is_some() {
            self.folders.remove(at);
        }
        Ok(n)
    }

    /// Loose files: 3D, slicer and archive files grouped by name, with the
    /// pictures and documents whose names start the same way; the rest are left.
    fn loose(&mut self, dir: &Path, files: &[(PathBuf, u64)]) -> usize {
        let files: Vec<&(PathBuf, u64)> = files
            .iter()
            .filter(|(f, _)| !self.claimed.contains(f))
            .collect();
        let mut groups: Vec<(String, Vec<PathBuf>)> = vec![];
        for (f, _) in files.iter().filter(|(f, _)| is_main(&file_name(f))) {
            let st = stem(&file_name(f));
            match groups.iter_mut().find(|g| g.0 == st) {
                Some(g) => g.1.push(f.clone()),
                None => groups.push((st, vec![f.clone()])),
            }
        }
        let mut n = 0;
        for (f, size) in files.iter().filter(|(f, _)| !is_main(&file_name(f))) {
            let st = stem(&file_name(f));
            match groups
                .iter_mut()
                .filter(|g| st.starts_with(&g.0))
                .max_by_key(|g| g.0.len())
            {
                Some(g) => g.1.push(f.clone()),
                None => {
                    let name = file_name(f);
                    self.left.push(Left {
                        path: s(f),
                        kind: file_kind(&name).into(),
                        name,
                        parent: s(dir),
                        size: *size,
                    });
                    n += 1;
                }
            }
        }
        for (_, fs) in groups {
            self.item(
                "files",
                Found {
                    path: fs[0].clone(),
                    files: fs,
                },
                Some(dir),
            );
            n += 1;
        }
        n
    }

    /// A root: a folder of models, or one model (a folder or a file).
    fn root(&mut self, r: &Root) -> Result<()> {
        let p = PathBuf::from(&r.path);
        if self.claimed.contains(&p) {
            return Ok(());
        }
        if p.is_dir() {
            let (dirs, files) = entries(&p);
            let folder_of_models = if r.contents {
                !self.joined.contains(&p)
            } else {
                self.split.contains(&p) || self.above_claimed.contains(&p)
            };
            if folder_of_models {
                self.container(&p, None, dirs, files, 0)?;
            } else {
                self.item(
                    "folder",
                    Found {
                        path: p,
                        files: vec![],
                    },
                    None,
                );
            }
        } else if p.is_file() {
            self.item(
                "files",
                Found {
                    path: p.clone(),
                    files: vec![p],
                },
                None,
            );
        }
        Ok(())
    }
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(String::from)
        .collect()
}

/// What `import::describe` says about a candidate, into an item (its name and
/// the owner's choices aside).
fn describe_into(it: &mut Item, d: &Value) {
    it.name = d["name"].as_str().unwrap_or("").to_string();
    it.author = d["author"].as_str().unwrap_or("").to_string();
    it.tags = d["tags"].as_str().unwrap_or("").to_string();
    refresh_from(it, d);
}

fn refresh_from(it: &mut Item, d: &Value) {
    it.summary = d["summary"].clone();
    it.warnings = d["warnings"].as_array().cloned().unwrap_or_default();
    it.guess = d["guess"].clone();
    it.has_sidecar = d["has_sidecar"] == json!(true);
}

/// The place (and name, and so on) an item had, onto the item that replaces it.
fn carry(from: &Item, to: &mut Item) {
    to.id = from.id.clone();
    to.name = from.name.clone();
    to.author = from.author.clone();
    to.tags = from.tags.clone();
    to.schema = from.schema.clone();
    to.values = from.values.clone();
    to.placed = from.placed;
    to.skip = from.skip;
    to.error = from.error.clone();
}

fn under(p: &str, dir: &str) -> bool {
    Path::new(p).starts_with(dir)
}

impl Session {
    pub fn new(library: &str) -> Session {
        Session {
            format: FORMAT,
            library: library.into(),
            ..Default::default()
        }
    }

    /// The session kept at `path`, or a new one for `library`.
    pub fn load(path: &Path, library: &str) -> Session {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Session>(&b).ok())
            .filter(|s| s.library == library)
            .unwrap_or_else(|| Session::new(library))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        crate::config::write_atomic(path, &serde_json::to_vec(self)?)
    }

    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    fn new_id(&mut self) -> String {
        self.next += 1;
        format!("s{}", self.next)
    }

    fn item(&self, id: &str) -> Result<&Item> {
        self.items.iter().find(|i| i.id == id).ok_or_else(|| {
            anyhow!("That model isn't in the workspace any more: read the folders again.")
        })
    }

    /// Take what a walk found: new ids, or the ids and choices of the items they
    /// replace (by their folder or first file).
    fn take(&mut self, w: Walker, old: Vec<Item>) {
        let mut by_path: HashMap<String, Item> =
            old.into_iter().map(|i| (i.path.clone(), i)).collect();
        for mut it in w.items {
            match by_path.remove(&it.path).filter(|o| o.kind == it.kind) {
                Some(o) => carry(&o, &mut it),
                None => it.id = self.new_id(),
            }
            self.items.push(it);
        }
        let mut shown: HashSet<String> = self.folders.iter().map(|f| f.path.clone()).collect();
        for f in w.folders {
            if shown.insert(f.path.clone()) {
                self.folders.push(f);
            }
        }
        self.left.extend(w.left);
        self.read = crate::library::now();
    }

    /// Add folders or files. `contents`: sort what's in them; otherwise each is a model.
    pub fn add(
        &mut self,
        ctx: &Ctx,
        paths: &[PathBuf],
        contents: bool,
        cancel: &AtomicBool,
        progress: &dyn Fn(usize, &str),
    ) -> Result<()> {
        let prev = self.items.clone();
        let mut added = vec![];
        for p in paths {
            import::check_source(&ctx.lib, p)?;
            if !p.exists() {
                bail!("{} isn't there.", p.display());
            }
            let ps = s(p);
            if self.roots.iter().any(|r| under(&ps, &r.path)) {
                continue; // already in the workspace
            }
            // a folder that holds roots takes their place
            let inside: Vec<String> = self
                .roots
                .iter()
                .filter(|r| under(&r.path, &ps))
                .map(|r| r.path.clone())
                .collect();
            for r in &inside {
                self.forget(r);
            }
            self.roots.retain(|r| !inside.contains(&r.path));
            let r = Root {
                path: ps,
                contents: contents && p.is_dir(),
            };
            self.roots.push(r.clone());
            added.push(r);
        }
        let mut w = Walker::new(self, ctx, cancel, progress);
        for r in &added {
            w.root(r)?;
        }
        // what was read before under a new folder keeps what was decided
        let still: HashSet<&str> = self.items.iter().map(|i| i.id.as_str()).collect();
        let old: Vec<Item> = prev
            .into_iter()
            .filter(|i| !still.contains(i.id.as_str()))
            .collect();
        self.take(w, old);
        self.place_groups();
        Ok(())
    }

    /// Drop what's at or under `dir` from the lists (imported items stay).
    fn forget(&mut self, dir: &str) {
        self.folders.retain(|f| !under(&f.path, dir));
        self.left.retain(|l| !under(&l.path, dir));
        self.items.retain(|i| {
            i.done.is_some() || i.kind == "group" || !i.sources.iter().all(|x| under(x, dir))
        });
    }

    /// Read every folder again: new files and folders show up, gone ones go, and
    /// what was decided stays with each model (by its folder or first file).
    pub fn rescan(
        &mut self,
        ctx: &Ctx,
        cancel: &AtomicBool,
        progress: &dyn Fn(usize, &str),
    ) -> Result<()> {
        // groups keep the sources still there; imported items stay as they are
        for it in self
            .items
            .iter_mut()
            .filter(|i| i.kind == "group" && i.done.is_none())
        {
            it.sources.retain(|x| Path::new(x).exists());
        }
        let (keep, old): (Vec<Item>, Vec<Item>) = std::mem::take(&mut self.items)
            .into_iter()
            .filter(|i| i.done.is_some() || !i.sources.is_empty())
            .partition(|i| i.kind == "group" || i.done.is_some());
        self.items = keep;
        self.roots.retain(|r| Path::new(&r.path).exists());
        self.folders.clear();
        self.left.clear();
        let mut w = Walker::new(self, ctx, cancel, progress);
        for r in self.roots.clone() {
            w.root(&r)?;
        }
        for it in self
            .items
            .iter_mut()
            .filter(|i| i.kind == "group" && i.done.is_none())
        {
            let f = Found {
                path: PathBuf::from(&it.sources[0]),
                files: it.sources.iter().map(PathBuf::from).collect(),
            };
            let d = import::describe(ctx, &f);
            refresh_from(it, &d);
        }
        self.take(w, old);
        self.place_groups();
        Ok(())
    }

    /// The deepest folder shown that holds all of `paths`.
    fn container_of(&self, paths: &[String]) -> Option<String> {
        self.folders
            .iter()
            .filter(|f| paths.iter().all(|p| p != &f.path && under(p, &f.path)))
            .max_by_key(|f| Path::new(&f.path).components().count())
            .map(|f| f.path.clone())
    }

    /// Groups are shown in the deepest folder that holds what's in them.
    fn place_groups(&mut self) {
        let parents: Vec<(usize, Option<String>)> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, i)| i.kind == "group")
            .map(|(n, i)| (n, self.container_of(&i.sources)))
            .collect();
        for (n, p) in parents {
            self.items[n].parent = p;
        }
    }

    /// Items chosen by id, and every item in the chosen folders (with the folders
    /// between the chosen one and it).
    fn chosen(&self, ids: &[String], folders: &[String]) -> Result<Vec<(usize, Vec<String>)>> {
        let mut out: Vec<(usize, Vec<String>)> = vec![];
        let mut seen = HashSet::new();
        for id in ids {
            let n = self.items.iter().position(|i| &i.id == id).ok_or_else(|| {
                anyhow!("That model isn't in the workspace any more: read the folders again.")
            })?;
            if seen.insert(n) {
                out.push((n, vec![]));
            }
        }
        for f in folders {
            let base = Path::new(f);
            for (n, it) in self.items.iter().enumerate() {
                let Some(p) = &it.parent else { continue };
                let Ok(rel) = Path::new(p).strip_prefix(base) else {
                    continue;
                };
                if !seen.insert(n) {
                    continue;
                }
                let mut segs = vec![file_name(base)];
                segs.extend(
                    rel.components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned()),
                );
                out.push((n, segs));
            }
        }
        Ok(out)
    }

    /// Send items, and everything in chosen folders, to a category (None: Unsorted)
    /// and subcategory. `keep`: the folders from the chosen one down to each model
    /// become subcategories below it (`keep_self`: the chosen folder too).
    #[allow(clippy::too_many_arguments)]
    pub fn send(
        &mut self,
        ids: &[String],
        folders: &[String],
        schema: Option<&str>,
        values: &[String],
        keep: bool,
        keep_self: bool,
    ) -> Result<usize> {
        let values = values
            .iter()
            .map(|v| crate::schema::subcategory_name(v))
            .collect::<Result<Vec<String>>>()?;
        let chosen = self.chosen(ids, folders)?;
        let mut n = 0;
        for (i, segs) in chosen {
            let it = &mut self.items[i];
            if it.done.is_some() {
                continue;
            }
            let mut v = values.clone();
            if keep && schema.is_some() {
                let from = if keep_self { 0 } else { 1 };
                v.extend(segs.iter().skip(from).filter_map(|x| as_subcategory(x)));
            }
            it.schema = schema.map(String::from);
            it.values = if schema.is_some() { v } else { vec![] };
            it.placed = true;
            it.skip = false;
            it.error = None;
            n += 1;
        }
        Ok(n)
    }

    /// Change items: name, author, tags; skip; clear (not sorted again); use the
    /// suggested place.
    pub fn update(&mut self, ids: &[String], folders: &[String], patch: &Value) -> Result<usize> {
        let chosen = self.chosen(ids, folders)?;
        let mut n = 0;
        for (i, _) in chosen {
            let it = &mut self.items[i];
            if it.done.is_some() {
                continue;
            }
            if let Some(v) = patch["name"].as_str() {
                if v.trim().is_empty() {
                    bail!("Give it a name.");
                }
                it.name = v.trim().to_string();
            }
            if let Some(v) = patch["author"].as_str() {
                it.author = v.trim().to_string();
            }
            if let Some(v) = patch["tags"].as_str() {
                it.tags = v.trim().to_string();
            }
            if let Some(v) = patch["skip"].as_bool() {
                it.skip = v;
            }
            if patch["clear"] == json!(true) {
                it.schema = None;
                it.values = vec![];
                it.placed = false;
                it.error = None;
            }
            if patch["accept"] == json!(true) {
                if let Some(sc) = it.guess["schema"].as_str() {
                    it.schema = Some(sc.to_string());
                    it.values = strings(&it.guess["values"]);
                    it.placed = true;
                    it.skip = false;
                    it.error = None;
                }
            }
            n += 1;
        }
        Ok(n)
    }

    /// The place most of `items` had, when they agree.
    fn shared_place(items: &[Item]) -> Option<(Option<String>, Vec<String>)> {
        let placed: Vec<&Item> = items.iter().filter(|i| i.placed).collect();
        let first = placed.first()?;
        placed
            .iter()
            .all(|i| i.schema == first.schema && i.values == first.values)
            .then(|| (first.schema.clone(), first.values.clone()))
    }

    /// Group items, folders and left files into one model. Folders become its
    /// part folders, files go at its top. When that's everything in one folder,
    /// the folder becomes the model. Returns the new item's id.
    pub fn group(
        &mut self,
        ctx: &Ctx,
        ids: &[String],
        folders: &[String],
        files: &[String],
        name: Option<&str>,
    ) -> Result<String> {
        let mut sources: Vec<String> = vec![];
        for id in ids {
            let it = self.item(id)?;
            if it.done.is_some() {
                bail!("{} is imported already.", it.name);
            }
            sources.extend(it.sources.iter().cloned());
        }
        for f in folders {
            if !self.folders.iter().any(|x| &x.path == f) {
                bail!("That folder isn't in the workspace any more: read the folders again.");
            }
            sources.push(f.clone());
        }
        for f in files {
            if !self.left.iter().any(|x| &x.path == f) {
                bail!("That file isn't in the workspace any more: read the folders again.");
            }
            sources.push(f.clone());
        }
        // what's inside another chosen folder comes with it
        let all = sources.clone();
        sources.retain(|x| !all.iter().any(|o| o != x && under(x, o)));
        sources.sort();
        sources.dedup();
        if sources.is_empty() {
            bail!("Choose what to group.");
        }
        let mut names = HashSet::new();
        for x in &sources {
            let n = file_name(Path::new(x)).to_lowercase();
            if !names.insert(n) {
                bail!(
                    "Two of them are called {}: they can't sit side by side in one model folder.",
                    file_name(Path::new(x))
                );
            }
        }
        // everything in one folder: that folder is the model
        let parents: HashSet<PathBuf> = sources
            .iter()
            .filter_map(|x| Path::new(x).parent().map(Path::to_path_buf))
            .collect();
        if parents.len() == 1 {
            let c = s(parents.iter().next().unwrap());
            let covered = |p: &str| sources.iter().any(|x| under(p, x));
            let everything = self.folders.iter().any(|f| f.path == c)
                && self
                    .folders
                    .iter()
                    .filter(|f| f.path != c && under(&f.path, &c))
                    .all(|f| covered(&f.path))
                && self
                    .left
                    .iter()
                    .filter(|l| under(&l.path, &c))
                    .all(|l| covered(&l.path))
                && self
                    .items
                    .iter()
                    .filter(|i| i.done.is_none() && i.sources.iter().all(|x| under(x, &c)))
                    .all(|i| i.sources.iter().all(|x| covered(x)));
            if everything && self.roots.iter().all(|r| r.path != c || r.contents) {
                let id = self.join(ctx, &c)?;
                if let Some(n) = name.filter(|n| !n.trim().is_empty()) {
                    self.update(std::slice::from_ref(&id), &[], &json!({ "name": n }))?;
                }
                return Ok(id);
            }
        }
        let covered = |p: &str| sources.iter().any(|x| under(p, x));
        let (gone, kept): (Vec<Item>, Vec<Item>) = std::mem::take(&mut self.items)
            .into_iter()
            .partition(|i| i.done.is_none() && i.sources.iter().all(|x| covered(x)));
        self.items = kept;
        let dirs: Vec<&String> = sources.iter().filter(|x| Path::new(x).is_dir()).collect();
        self.folders
            .retain(|f| !dirs.iter().any(|d| under(&f.path, d)));
        self.left.retain(|l| !covered(&l.path));
        let f = Found {
            path: PathBuf::from(&sources[0]),
            files: sources.iter().map(PathBuf::from).collect(),
        };
        let d = import::describe(ctx, &f);
        let mut it = Item {
            id: self.new_id(),
            kind: "group".into(),
            path: sources[0].clone(),
            parent: self.container_of(&sources),
            sources: sources.clone(),
            ..Default::default()
        };
        describe_into(&mut it, &d);
        it.name = match name.map(str::trim).filter(|n| !n.is_empty()) {
            Some(n) => n.to_string(),
            None if parents.len() == 1 => {
                import::guess_name(&file_name(parents.iter().next().unwrap())).0
            }
            None => gone
                .first()
                .map(|g| g.name.clone())
                .unwrap_or_else(|| it.name.clone()),
        };
        if it.author.is_empty() {
            it.author = gone
                .iter()
                .find(|g| !g.author.is_empty())
                .map(|g| g.author.clone())
                .unwrap_or_default();
        }
        if let Some((schema, values)) = Self::shared_place(&gone) {
            it.schema = schema;
            it.values = values;
            it.placed = true;
        }
        let id = it.id.clone();
        self.items.push(it);
        Ok(id)
    }

    /// Make a folder one model: what's in it becomes its parts.
    pub fn join(&mut self, ctx: &Ctx, dir: &str) -> Result<String> {
        let f = self
            .folders
            .iter()
            .find(|f| f.path == dir)
            .cloned()
            .ok_or_else(|| {
                anyhow!("That folder isn't in the workspace any more: read the folders again.")
            })?;
        if self.items.iter().any(|i| {
            i.done.is_none()
                && i.sources.iter().any(|x| under(x, dir))
                && !i.sources.iter().all(|x| under(x, dir))
        }) {
            bail!("Some of this folder is grouped with files outside it: ungroup that first.");
        }
        let (gone, kept): (Vec<Item>, Vec<Item>) = std::mem::take(&mut self.items)
            .into_iter()
            .partition(|i| i.done.is_none() && i.sources.iter().all(|x| under(x, dir)));
        self.items = kept;
        self.folders.retain(|x| !under(&x.path, dir));
        self.left.retain(|l| !under(&l.path, dir));
        let p = PathBuf::from(dir);
        self.split.retain(|x| x != dir);
        let (dirs, files) = entries(&p);
        let (cancel, quiet) = (AtomicBool::new(false), |_: usize, _: &str| {});
        let w = Walker::new(self, ctx, &cancel, &quiet);
        if !w.looks_like_model(&dirs, &files) && !self.joined.iter().any(|x| x == dir) {
            self.joined.push(dir.to_string());
        }
        for r in self.roots.iter_mut().filter(|r| r.path == dir) {
            r.contents = false;
        }
        let d = import::describe(
            ctx,
            &Found {
                path: p.clone(),
                files: vec![],
            },
        );
        let mut it = Item {
            id: self.new_id(),
            kind: "folder".into(),
            path: dir.to_string(),
            parent: f.parent.clone(),
            sources: vec![dir.to_string()],
            ..Default::default()
        };
        describe_into(&mut it, &d);
        it.warnings.retain(|w| w["kind"] != "several");
        if let Some((schema, values)) = Self::shared_place(&gone) {
            it.schema = schema;
            it.values = values;
            it.placed = true;
        }
        let id = it.id.clone();
        self.items.push(it);
        Ok(id)
    }

    /// Split a model: a folder becomes a model per sub-folder and per group of
    /// loose files; a group goes back to what it was made of.
    pub fn split(&mut self, ctx: &Ctx, id: &str) -> Result<()> {
        let it = self.item(id)?.clone();
        if it.done.is_some() {
            bail!("{} is imported already.", it.name);
        }
        let place = it.placed.then(|| (it.schema.clone(), it.values.clone()));
        let before: HashSet<String> = self.items.iter().map(|i| i.id.clone()).collect();
        self.items.retain(|i| i.id != id);
        let (cancel, quiet) = (AtomicBool::new(false), |_: usize, _: &str| {});
        match it.kind.as_str() {
            "folder" => {
                let dir = PathBuf::from(&it.path);
                self.joined.retain(|x| x != &it.path);
                let (dirs, files) = entries(&dir);
                let mut w = Walker::new(self, ctx, &cancel, &quiet);
                if w.looks_like_model(&dirs, &files) {
                    self.split.push(it.path.clone());
                    w.split.insert(dir.clone());
                }
                for r in self.roots.iter_mut().filter(|r| r.path == it.path) {
                    r.contents = true;
                }
                let parent = it.parent.as_ref().map(PathBuf::from);
                w.container(&dir, parent.as_deref(), dirs, files, 0)?;
                if !w.folders.iter().any(|f| f.path == it.path) {
                    // nothing came out of it: it stays a folder (to group again)
                    w.folders.push(Folder {
                        path: it.path.clone(),
                        name: file_name(&dir),
                        parent: it.parent.clone(),
                    });
                }
                self.take(w, vec![]);
            }
            "group" => {
                let mut w = Walker::new(self, ctx, &cancel, &quiet);
                let mut loose_in: Vec<PathBuf> = vec![];
                for x in &it.sources {
                    let p = PathBuf::from(x);
                    if let Some(r) = self.roots.iter().find(|r| &r.path == x) {
                        w.root(&r.clone())?;
                        continue;
                    }
                    let parent = p.parent().map(Path::to_path_buf);
                    if let Some(pp) = &parent {
                        self.ensure_folder(pp);
                    }
                    if p.is_dir() {
                        w.add_dir(&p, parent.as_deref(), 0)?;
                    } else if let Some(pp) = parent {
                        if !loose_in.contains(&pp) {
                            loose_in.push(pp);
                        }
                    }
                }
                // loose files are grouped by name again with what's beside them
                let mut old = vec![];
                for dir in &loose_in {
                    let ds = s(dir);
                    let (again, kept): (Vec<Item>, Vec<Item>) =
                        std::mem::take(&mut self.items).into_iter().partition(|i| {
                            i.kind == "files"
                                && i.done.is_none()
                                && i.parent.as_deref() == Some(ds.as_str())
                        });
                    self.items = kept;
                    old.extend(again);
                    self.left.retain(|l| l.parent != ds);
                    for i in &old {
                        for x in &i.sources {
                            w.claimed.remove(&PathBuf::from(x));
                        }
                    }
                    let files = entries(dir).1;
                    w.loose(dir, &files);
                }
                self.take(w, old);
            }
            _ => bail!("Loose files can't be split: group the ones that belong together instead."),
        }
        // what came out of it goes where it was going
        if let Some((schema, values)) = place {
            for i in self
                .items
                .iter_mut()
                .filter(|i| !before.contains(&i.id) && !i.placed)
            {
                i.schema = schema.clone();
                i.values = values.clone();
                i.placed = true;
            }
        }
        self.place_groups();
        Ok(())
    }

    /// Show `dir` (and the folders above it, up to a root) as a folder.
    fn ensure_folder(&mut self, dir: &Path) {
        let ds = s(dir);
        if self.folders.iter().any(|f| f.path == ds) {
            return;
        }
        let parent = dir.parent().filter(|p| {
            let ps = s(p);
            self.roots.iter().any(|r| under(&ps, &r.path))
        });
        if let Some(p) = parent {
            self.ensure_folder(p);
        }
        self.folders.push(Folder {
            path: ds,
            name: file_name(dir),
            parent: parent.map(s),
        });
    }

    /// What's ready to import (placed, not skipped, not imported yet), optionally
    /// only `ids`: (item id, the import item).
    pub fn ready(&self, ids: Option<&[String]>) -> Vec<(String, Value)> {
        self.items
            .iter()
            .filter(|i| i.placed && !i.skip && i.done.is_none())
            .filter(|i| ids.is_none_or(|ids| ids.contains(&i.id)))
            .map(|i| {
                let files: Vec<&String> = if i.kind == "folder" {
                    vec![]
                } else {
                    i.sources.iter().collect()
                };
                (
                    i.id.clone(),
                    json!({ "source": i.path, "files": files, "name": i.name, "author": i.author, "tags": i.tags, "schema": i.schema, "values": i.values }),
                )
            })
            .collect()
    }

    /// Mark what an import did: { id, result } with the result's `rel` and `id`,
    /// or its `error`.
    pub fn mark(&mut self, results: &[(String, Value)], moved: bool) {
        let mut emptied: Vec<PathBuf> = vec![];
        for (id, r) in results {
            let Some(it) = self.items.iter_mut().find(|i| &i.id == id) else {
                continue;
            };
            match r["error"].as_str() {
                Some(e) => it.error = Some(e.to_string()),
                None => {
                    it.error = None;
                    it.done = Some(json!({ "rel": r["rel"], "id": r["id"], "moved": moved }));
                    if moved {
                        emptied.extend(
                            it.sources
                                .iter()
                                .filter_map(|x| Path::new(x).parent().map(Path::to_path_buf)),
                        );
                    }
                }
            }
        }
        // folders a move left empty go (up to the folder that was added)
        let roots: Vec<PathBuf> = self.roots.iter().map(|r| PathBuf::from(&r.path)).collect();
        emptied.sort();
        emptied.dedup();
        for d in emptied.iter().rev() {
            let mut d = d.as_path();
            while roots.iter().any(|r| d.starts_with(r) && d != r) {
                if std::fs::remove_dir(d).is_err() {
                    break;
                }
                match d.parent() {
                    Some(p) => d = p,
                    None => break,
                }
            }
        }
    }

    /// Forget imported models (they're in the library now), or everything.
    pub fn clear(&mut self, imported_only: bool) {
        if imported_only {
            self.items.retain(|i| i.done.is_none());
            // and the folders a move emptied
            self.folders.retain(|f| Path::new(&f.path).is_dir());
        } else {
            let lib = std::mem::take(&mut self.library);
            *self = Session::new(&lib);
        }
    }

    /// Where an item's file is now (by its path in the model's folder).
    pub fn file_at(&self, id: &str, rel: &str) -> Result<PathBuf> {
        let it = self.item(id)?;
        let rel = crate::library::rel_inside(rel)?;
        if it.kind == "folder" {
            return Ok(PathBuf::from(&it.path).join(rel));
        }
        let mut comps = rel.components();
        let first = comps
            .next()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .unwrap_or_default();
        let rest: PathBuf = comps.collect();
        let src = it
            .sources
            .iter()
            .map(PathBuf::from)
            .find(|x| file_name(x) == first)
            .ok_or_else(|| anyhow!("{} isn't one of its files", rel.display()))?;
        Ok(if rest.as_os_str().is_empty() {
            src
        } else {
            src.join(rest)
        })
    }

    /// An item's files as they'd be in its model folder: [{ rel, size, kind }].
    pub fn files(&self, id: &str) -> Result<Vec<(String, u64, PathBuf)>> {
        let it = self.item(id)?;
        let f = if it.kind == "folder" {
            Found {
                path: PathBuf::from(&it.path),
                files: vec![],
            }
        } else {
            Found {
                path: PathBuf::from(&it.path),
                files: it.sources.iter().map(PathBuf::from).collect(),
            }
        };
        Ok(import::listing(&f))
    }
}

/// Junk names, exposed for the folder watcher.
pub fn is_junk(name: &str) -> bool {
    junk(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::Index;
    use crate::library::Library;
    use crate::schema;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("modlib-sort-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn put(p: &Path, body: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    fn names(se: &Session) -> Vec<String> {
        let mut v: Vec<String> = se.items.iter().map(|i| i.name.clone()).collect();
        v.sort();
        v
    }

    fn by_name<'a>(se: &'a Session, n: &str) -> &'a Item {
        se.items
            .iter()
            .find(|i| i.name == n)
            .unwrap_or_else(|| panic!("no {n} in {:?}", names(se)))
    }

    #[test]
    fn reads_a_folder_tree_as_it_is() {
        let d = tmp("read");
        let lib = Library::open(d.join("Lib")).unwrap();
        schema::create(&lib, &json!({ "name": "Home items", "subcategories": [{ "name": "Kitchen", "subcategories": [] }] })).unwrap();
        let src = d.join("Share");
        put(&src.join("Kitchen/Spoon rest (Jo)/spoon.stl"), "x");
        put(&src.join("Kitchen/Spoon rest (Jo)/photo.jpg"), "x");
        put(
            &src.join("Kitchen/Gadgets/Bag clip/Presupported/clip.stl"),
            "x",
        );
        put(
            &src.join("Kitchen/Gadgets/Bag clip/Unsupported/clip.stl"),
            "x",
        );
        put(&src.join("Kitchen/Gadgets/Funnel/STL/funnel.stl"), "x");
        put(&src.join("Kitchen/Gadgets/Funnel/Images/funnel.png"), "x");
        put(&src.join("Kitchen/Gadgets/hook.3mf"), "x");
        put(&src.join("Kitchen/Gadgets/hook_render.png"), "x");
        put(&src.join("Kitchen/Gadgets/readme.pdf"), "x");
        put(&src.join("Armour/Helmet/helmet.stl"), "x");
        put(&src.join("Armour/Arms/arm.stl"), "x");
        put(&src.join("Empty/Nothing/.keep"), "");
        put(&src.join("@eaDir/junk.stl"), "x");
        put(&src.join("loose.stl"), "x");
        let ix = Index::build(&lib, None, false);
        let ctx = Ctx::new(&lib, &ix);
        let mut se = Session::new("lib1");
        let cancel = AtomicBool::new(false);
        se.add(&ctx, std::slice::from_ref(&src), true, &cancel, &|_, _| {})
            .unwrap();
        assert_eq!(
            names(&se),
            [
                "Arms",
                "Bag clip",
                "Funnel",
                "Helmet",
                "Spoon rest",
                "hook",
                "loose"
            ]
        );
        let spoon = by_name(&se, "Spoon rest");
        assert_eq!(
            (spoon.kind.as_str(), spoon.author.as_str()),
            ("folder", "Jo")
        );
        assert_eq!(
            spoon.parent.as_deref(),
            Some(s(&src.join("Kitchen")).as_str())
        );
        // its folder's name says where it goes
        assert_eq!(
            spoon.guess,
            json!({ "schema": "home-items", "values": ["Kitchen"] })
        );
        assert!(!spoon.placed);
        let hook = by_name(&se, "hook");
        assert_eq!((hook.kind.as_str(), hook.sources.len()), ("files", 2));
        // folders shown: the share, Kitchen, Gadgets, Armour (not the empty ones or the NAS's)
        let mut f: Vec<&str> = se.folders.iter().map(|f| f.name.as_str()).collect();
        f.sort();
        assert_eq!(f, ["Armour", "Gadgets", "Kitchen", "Share"]);
        assert_eq!(se.left.len(), 1);
        assert_eq!(se.left[0].name, "readme.pdf");
        // the library, and a folder holding it, can't be added
        assert!(se
            .add(&ctx, &[lib.root().to_path_buf()], true, &cancel, &|_, _| {})
            .is_err());
        assert!(se
            .add(&ctx, std::slice::from_ref(&d), true, &cancel, &|_, _| {})
            .is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn sends_groups_joins_splits_and_keeps_choices() {
        let d = tmp("ops");
        let lib = Library::open(d.join("Lib")).unwrap();
        schema::create(&lib, &json!({ "name": "Home items" })).unwrap();
        let src = d.join("Share");
        put(&src.join("Office/Desk/Pen pot/pot.stl"), "x");
        put(&src.join("Office/Desk/Tray/tray.stl"), "x");
        put(&src.join("Office/Lamp/lamp.stl"), "x");
        put(&src.join("Armour/Helmet/helmet.stl"), "x");
        put(&src.join("Armour/Arms/arm.stl"), "x");
        put(&src.join("Armour/notes.txt"), "x");
        put(&src.join("Bits/a.stl"), "x");
        put(&src.join("Bits/b.stl"), "x");
        put(&src.join("Bits/c.png"), "x");
        let ix = Index::build(&lib, None, false);
        let ctx = Ctx::new(&lib, &ix);
        let mut se = Session::new("lib1");
        let cancel = AtomicBool::new(false);
        se.add(&ctx, std::slice::from_ref(&src), true, &cancel, &|_, _| {})
            .unwrap();
        // send a folder with its folders kept as subcategories
        let office = s(&src.join("Office"));
        let n = se
            .send(
                &[],
                std::slice::from_ref(&office),
                Some("home-items"),
                &["Rooms".into()],
                true,
                true,
            )
            .unwrap();
        assert_eq!(n, 3);
        assert_eq!(by_name(&se, "Pen pot").values, ["Rooms", "Office", "Desk"]);
        assert_eq!(by_name(&se, "Lamp").values, ["Rooms", "Office"]);
        se.send(
            &[],
            std::slice::from_ref(&office),
            Some("home-items"),
            &[],
            true,
            false,
        )
        .unwrap();
        assert_eq!(by_name(&se, "Pen pot").values, ["Desk"]);
        assert_eq!(by_name(&se, "Lamp").values, Vec::<String>::new());
        // a folder read as several models becomes one: everything in it is its parts
        let armour = s(&src.join("Armour"));
        let helmet = by_name(&se, "Helmet").id.clone();
        se.send(
            &[helmet],
            &[],
            Some("home-items"),
            &["Wear".into()],
            false,
            false,
        )
        .unwrap();
        let id = se.join(&ctx, &armour).unwrap();
        let a = se.item(&id).unwrap().clone();
        assert_eq!((a.name.as_str(), a.kind.as_str()), ("Armour", "folder"));
        assert_eq!(a.values, ["Wear"]);
        assert!(se.left.is_empty() && !se.folders.iter().any(|f| f.path == armour));
        // and back
        se.split(&ctx, &id).unwrap();
        assert!(se.items.iter().any(|i| i.name == "Helmet") && se.left.len() == 1);
        assert_eq!(by_name(&se, "Arms").values, ["Wear"]);
        // a folder of loose files is one model until it's split
        let bits = by_name(&se, "Bits").id.clone();
        se.split(&ctx, &bits).unwrap();
        // group loose files and a left one into one model, named after their folder
        let (a, b) = (by_name(&se, "a").id.clone(), by_name(&se, "b").id.clone());
        let png = s(&src.join("Bits/c.png"));
        let g = se
            .group(
                &ctx,
                std::slice::from_ref(&a),
                &[],
                std::slice::from_ref(&png),
                None,
            )
            .unwrap();
        let gi = se.item(&g).unwrap().clone();
        assert_eq!(
            (gi.kind.as_str(), gi.name.as_str(), gi.sources.len()),
            ("group", "Bits", 2)
        );
        assert_eq!(gi.parent.as_deref(), Some(s(&src.join("Bits")).as_str()));
        assert!(se.left.iter().all(|l| l.path != png) && se.item(&a).is_err());
        // ungroup: the files are read again with what's beside them
        se.split(&ctx, &g).unwrap();
        assert!(se.items.iter().any(|i| i.name == "a") && se.left.iter().any(|l| l.path == png));
        // grouping everything in a folder makes the folder the model
        let a = by_name(&se, "a").id.clone();
        let g = se
            .group(
                &ctx,
                &[a, b],
                &[],
                std::slice::from_ref(&png),
                Some("Spare bits"),
            )
            .unwrap();
        let gi = se.item(&g).unwrap();
        assert_eq!(
            (gi.kind.as_str(), gi.name.as_str()),
            ("folder", "Spare bits")
        );
        // reading again keeps what was decided
        let before = by_name(&se, "Pen pot").clone();
        let lamp = by_name(&se, "Lamp").id.clone();
        se.update(
            std::slice::from_ref(&lamp),
            &[],
            &json!({ "skip": true, "name": "Desk lamp" }),
        )
        .unwrap();
        put(&src.join("Office/New one/new.stl"), "x");
        se.rescan(&ctx, &cancel, &|_, _| {}).unwrap();
        let after = by_name(&se, "Pen pot");
        assert_eq!((&after.id, &after.values), (&before.id, &before.values));
        assert!(by_name(&se, "Desk lamp").skip && by_name(&se, "Spare bits").kind == "folder");
        assert!(se.items.iter().any(|i| i.name == "New one" && !i.placed));
        // what's ready: placed and not skipped
        let ready: Vec<String> = se
            .ready(None)
            .iter()
            .map(|(_, v)| v["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(ready.len(), 4, "{ready:?}"); // Pen pot, Tray, and Armour's parts went to Wear
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn files_of_a_group_are_found_by_their_place_in_it() {
        let d = tmp("files");
        let lib = Library::open(d.join("Lib")).unwrap();
        let src = d.join("Share");
        put(&src.join("X/Part A/a.stl"), "x");
        put(&src.join("X/b.stl"), "xy");
        let ix = Index::build(&lib, None, false);
        let ctx = Ctx::new(&lib, &ix);
        let mut se = Session::new("lib1");
        se.add(
            &ctx,
            &[src.join("X/Part A"), src.join("X/b.stl")],
            false,
            &AtomicBool::new(false),
            &|_, _| {},
        )
        .unwrap();
        assert_eq!(se.items.len(), 2);
        let ids: Vec<String> = se.items.iter().map(|i| i.id.clone()).collect();
        let g = se.group(&ctx, &ids, &[], &[], Some("X")).unwrap();
        let files: Vec<(String, u64)> = se
            .files(&g)
            .unwrap()
            .into_iter()
            .map(|(r, s, _)| (r, s))
            .collect();
        assert_eq!(
            files,
            [("Part A/a.stl".to_string(), 1), ("b.stl".to_string(), 2)]
        );
        assert_eq!(
            se.file_at(&g, "Part A/a.stl").unwrap(),
            src.join("X/Part A/a.stl")
        );
        assert!(se.file_at(&g, "../X/b.stl").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
