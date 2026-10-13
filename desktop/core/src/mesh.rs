//! Reading 3D files into triangles (docs/PLAN.md, "Phase 3 design"): STL (binary
//! and text), OBJ, and 3MF (including projects whose meshes live in separate
//! files, with component and build transforms). The page gets binary STL, so it
//! needs one loader; thumbnails are drawn from the same triangles. A 3MF's
//! colours (docs/PLAN.md, "Group 3 design") are read per triangle and travel in
//! the STL the way Materialise Magics writes them.

use anyhow::{anyhow, bail, Context, Result};
use std::collections::HashMap;
use std::io::{Cursor, Read};

/// At most this many triangles are read from one file.
pub const MAX_TRIANGLES: usize = 30_000_000;

/// Triangles as flat positions: 9 floats each.
pub type Tris = Vec<f32>;

/// The 3D formats this module reads.
pub fn readable(name: &str) -> bool {
    matches!(ext(name).as_str(), "stl" | "obj" | "3mf")
}

fn ext(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// A colour, red green blue.
pub type Rgb = [u8; 3];

/// Triangles and, when the file has them, a colour for each (None: no colour of
/// its own).
pub struct Mesh {
    pub tris: Tris,
    pub colours: Option<Vec<Option<Rgb>>>,
}

/// Read a 3D file's triangles from its bytes, by the file's name.
pub fn read(name: &str, bytes: &[u8]) -> Result<Tris> {
    Ok(read_mesh(name, bytes)?.tris)
}

/// Read a 3D file's triangles and its colours (only 3MF files have them).
pub fn read_mesh(name: &str, bytes: &[u8]) -> Result<Mesh> {
    let tris = match ext(name).as_str() {
        "stl" => read_stl(bytes)?,
        "obj" => read_obj(bytes)?,
        "3mf" => return read_3mf(bytes),
        e => bail!("can't show .{e} files in 3D"),
    };
    Ok(Mesh { tris, colours: None })
}

/// "#rrggbb" or "#rrggbbaa" (the alpha is dropped).
pub fn parse_colour(s: &str) -> Option<Rgb> {
    let h = s.trim().trim_matches('"').strip_prefix('#')?;
    if !(h.len() == 6 || h.len() == 8) || !h.is_ascii() {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([b(0)?, b(2)?, b(4)?])
}

/// "#rrggbb".
pub fn colour_hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn read_stl(b: &[u8]) -> Result<Tris> {
    let binary_len = |n: usize| 84 + 50 * n;
    let n = if b.len() >= 84 {
        u32::from_le_bytes([b[80], b[81], b[82], b[83]]) as usize
    } else {
        0
    };
    let looks_text = b.len() >= 5
        && b[..5].eq_ignore_ascii_case(b"solid")
        && b.windows(5)
            .take(4096)
            .any(|w| w.eq_ignore_ascii_case(b"facet"));
    if b.len() >= 84 && (binary_len(n) == b.len() || (!looks_text && binary_len(n) <= b.len())) {
        if n > MAX_TRIANGLES {
            bail!("too many triangles ({n})");
        }
        let mut out = Vec::with_capacity(n * 9);
        for i in 0..n {
            let at = 84 + i * 50 + 12;
            for k in 0..9 {
                let o = at + k * 4;
                out.push(f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]));
            }
        }
        return Ok(out);
    }
    let text = String::from_utf8_lossy(b);
    let mut out = vec![];
    let mut words = text.split_ascii_whitespace();
    while let Some(w) = words.next() {
        if w.eq_ignore_ascii_case("vertex") {
            for _ in 0..3 {
                out.push(
                    words
                        .next()
                        .and_then(|x| x.parse().ok())
                        .ok_or_else(|| anyhow!("a vertex without three numbers"))?,
                );
            }
            if out.len() > MAX_TRIANGLES * 9 {
                bail!("too many triangles");
            }
        }
    }
    out.truncate(out.len() / 9 * 9);
    if out.is_empty() {
        bail!("no triangles in this STL");
    }
    Ok(out)
}

fn read_obj(b: &[u8]) -> Result<Tris> {
    let text = String::from_utf8_lossy(b);
    let mut verts: Vec<[f32; 3]> = vec![];
    let mut out = vec![];
    for line in text.lines() {
        let mut it = line.split_ascii_whitespace();
        match it.next() {
            Some("v") => {
                let v: Vec<f32> = it.take(3).filter_map(|x| x.parse().ok()).collect();
                if v.len() == 3 {
                    verts.push([v[0], v[1], v[2]]);
                }
            }
            Some("f") => {
                let idx: Vec<usize> = it
                    .filter_map(|t| t.split('/').next()?.parse::<i64>().ok())
                    .filter_map(|i| {
                        if i < 0 {
                            verts.len().checked_sub((-i) as usize)
                        } else {
                            (i as usize).checked_sub(1)
                        }
                    })
                    .collect();
                for k in 1..idx.len().saturating_sub(1) {
                    for &i in &[idx[0], idx[k], idx[k + 1]] {
                        out.extend_from_slice(
                            verts
                                .get(i)
                                .ok_or_else(|| anyhow!("a face uses a missing vertex"))?,
                        );
                    }
                }
                if out.len() > MAX_TRIANGLES * 9 {
                    bail!("too many triangles");
                }
            }
            _ => {}
        }
    }
    if out.is_empty() {
        bail!("no faces in this OBJ");
    }
    Ok(out)
}

// ------------------------------------------------------------ 3MF

/// One XML tag: its name (namespace prefix dropped), attributes (prefixes dropped),
/// and whether it opens, closes or is both.
struct Tag<'a> {
    name: &'a str,
    attrs: Vec<(&'a str, &'a str)>,
    closing: bool,
    empty: bool,
}

fn local(s: &str) -> &str {
    s.rsplit_once(':').map(|(_, l)| l).unwrap_or(s)
}

/// The tags of an XML document, in order (enough for 3MF: no entities in numbers).
fn tags(xml: &str) -> impl Iterator<Item = Tag<'_>> {
    let mut rest = xml;
    std::iter::from_fn(move || loop {
        let start = rest.find('<')?;
        rest = &rest[start + 1..];
        if rest.starts_with('?') || rest.starts_with('!') {
            let end = if rest.starts_with("!--") {
                rest.find("-->").map(|i| i + 3)
            } else {
                rest.find('>').map(|i| i + 1)
            };
            rest = &rest[end.unwrap_or(rest.len())..];
            continue;
        }
        let end = rest.find('>')?;
        let body = &rest[..end];
        rest = &rest[end + 1..];
        let closing = body.starts_with('/');
        let empty = body.ends_with('/');
        let body = body.trim_start_matches('/').trim_end_matches('/');
        let name_end = body.find(|c: char| c.is_whitespace()).unwrap_or(body.len());
        let name = local(&body[..name_end]);
        let mut attrs = vec![];
        let mut a = &body[name_end..];
        while let Some(eq) = a.find('=') {
            let key = local(a[..eq].trim());
            let after = a[eq + 1..].trim_start();
            let Some(q) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
                break;
            };
            let Some(close) = after[1..].find(q) else {
                break;
            };
            attrs.push((key, &after[1..1 + close]));
            a = &after[close + 2..];
        }
        return Some(Tag {
            name,
            attrs,
            closing,
            empty,
        });
    })
}

impl Tag<'_> {
    fn get(&self, k: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(key, _)| *key == k)
            .map(|(_, v)| *v)
    }
    fn num(&self, k: &str) -> f32 {
        self.get(k)
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0.0)
    }
}

type Transform = [f32; 12];
const IDENTITY: Transform = [1., 0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0.];

fn parse_transform(s: Option<&str>) -> Transform {
    let v: Vec<f32> = s
        .unwrap_or("")
        .split_ascii_whitespace()
        .filter_map(|x| x.parse().ok())
        .collect();
    if v.len() == 12 {
        v.try_into().unwrap()
    } else {
        IDENTITY
    }
}

fn apply(m: &Transform, p: [f32; 3]) -> [f32; 3] {
    [
        p[0] * m[0] + p[1] * m[3] + p[2] * m[6] + m[9],
        p[0] * m[1] + p[1] * m[4] + p[2] * m[7] + m[10],
        p[0] * m[2] + p[1] * m[5] + p[2] * m[8] + m[11],
    ]
}

/// `outer` after `inner` (a component's transform inside its parent's).
fn compose(outer: &Transform, inner: &Transform) -> Transform {
    let mut m = [0f32; 12];
    for r in 0..4 {
        let row = if r < 3 {
            [inner[r * 3], inner[r * 3 + 1], inner[r * 3 + 2]]
        } else {
            [inner[9], inner[10], inner[11]]
        };
        for c in 0..3 {
            m[r * 3 + c] = row[0] * outer[c]
                + row[1] * outer[3 + c]
                + row[2] * outer[6 + c]
                + if r == 3 { outer[9 + c] } else { 0.0 };
        }
    }
    m
}

#[derive(Default)]
struct Object {
    verts: Vec<[f32; 3]>,
    tris: Vec<[u32; 3]>,
    /// Each triangle's property (group id, index), when any triangle has one.
    props: Vec<Option<(String, usize)>>,
    /// The object's own property (`pid`, `pindex`).
    prop: Option<(String, usize)>,
    /// (model file, object id, transform)
    comps: Vec<(Option<String>, String, Transform)>,
}

struct ModelFile {
    objects: HashMap<String, Object>,
    build: Vec<(Option<String>, String, Transform)>,
    /// Colour groups and base materials by id: their colours in order.
    groups: HashMap<String, Vec<Rgb>>,
}

impl ModelFile {
    fn colour(&self, prop: &Option<(String, usize)>) -> Option<Rgb> {
        let (g, i) = prop.as_ref()?;
        self.groups.get(g)?.get(*i).copied()
    }
}

fn parse_model(xml: &str) -> ModelFile {
    let mut objects = HashMap::new();
    let mut build = vec![];
    let mut groups: HashMap<String, Vec<Rgb>> = HashMap::new();
    let mut group: Option<String> = None;
    let mut cur: Option<(String, Object)> = None;
    let mut in_build = false;
    let index = |t: &Tag, k: &str| t.get(k).and_then(|x| x.trim().parse::<usize>().ok());
    for t in tags(xml) {
        match (t.name, t.closing) {
            ("basematerials" | "colorgroup", false) if !t.empty => {
                let id = t.get("id").unwrap_or("").to_string();
                groups.entry(id.clone()).or_default();
                group = Some(id);
            }
            ("basematerials" | "colorgroup", true) => group = None,
            ("base" | "color", false) => {
                if let Some(g) = &group {
                    let c = t.get("displaycolor").or(t.get("color")).and_then(parse_colour);
                    groups.get_mut(g).unwrap().push(c.unwrap_or(GRAY));
                }
            }
            ("object", false) => {
                let id = t.get("id").unwrap_or("").to_string();
                let o = Object {
                    prop: t.get("pid").map(|p| (p.to_string(), index(&t, "pindex").unwrap_or(0))),
                    ..Object::default()
                };
                if t.empty {
                    objects.insert(id, o);
                } else {
                    cur = Some((id, o));
                }
            }
            ("object", true) => {
                if let Some((id, o)) = cur.take() {
                    objects.insert(id, o);
                }
            }
            ("vertex", false) => {
                if let Some((_, o)) = cur.as_mut() {
                    o.verts.push([t.num("x"), t.num("y"), t.num("z")]);
                }
            }
            ("triangle", false) => {
                if let Some((_, o)) = cur.as_mut() {
                    let v = |k| t.get(k).and_then(|x| x.parse().ok()).unwrap_or(u32::MAX);
                    o.tris.push([v("v1"), v("v2"), v("v3")]);
                    // a property of its own: `pid` (else the object's) and `p1`
                    if let Some(p1) = index(&t, "p1") {
                        let pid = t.get("pid").map(String::from).or_else(|| o.prop.as_ref().map(|p| p.0.clone()));
                        if let Some(pid) = pid {
                            o.props.resize(o.tris.len() - 1, None);
                            o.props.push(Some((pid, p1)));
                        }
                    } else if let Some(pid) = t.get("pid") {
                        o.props.resize(o.tris.len() - 1, None);
                        o.props.push(Some((pid.to_string(), 0)));
                    }
                }
            }
            ("component", false) => {
                if let Some((_, o)) = cur.as_mut() {
                    o.comps.push((
                        t.get("path").map(|p| p.trim_start_matches('/').to_string()),
                        t.get("objectid").unwrap_or("").to_string(),
                        parse_transform(t.get("transform")),
                    ));
                }
            }
            ("build", closing) => in_build = !closing && !t.empty,
            ("item", false) if in_build => {
                build.push((
                    t.get("path").map(|p| p.trim_start_matches('/').to_string()),
                    t.get("objectid").unwrap_or("").to_string(),
                    parse_transform(t.get("transform")),
                ));
            }
            _ => {}
        }
    }
    ModelFile {
        objects,
        build,
        groups,
    }
}

/// The colour a 3D file is drawn in when nothing else says.
pub const GRAY: Rgb = [0xa3, 0xa9, 0xae];

/// What a slicer's project says about colours: its filament (extruder) colours,
/// and which extruder each object, part (by component object id) and range of
/// triangles prints with. Extruders count from 1.
#[derive(Default)]
struct Slicer {
    palette: Vec<Rgb>,
    objects: HashMap<String, usize>,
    parts: HashMap<(String, String), usize>,
    /// PrusaSlicer's volumes: (first triangle, last triangle, extruder) in an object
    volumes: HashMap<String, Vec<(usize, usize, usize)>>,
}

impl Slicer {
    fn pick(&self, extruder: usize) -> Option<Rgb> {
        self.palette.get(extruder.max(1) - 1).copied()
    }
    fn object(&self, id: &str) -> Option<Rgb> {
        if self.palette.is_empty() {
            return None;
        }
        self.pick(self.objects.get(id).copied().unwrap_or(1))
    }
    fn part(&self, object: &str, part: &str) -> Option<Rgb> {
        self.pick(*self.parts.get(&(object.to_string(), part.to_string()))?)
    }
    fn volume(&self, object: &str, tri: usize) -> Option<Rgb> {
        let v = self.volumes.get(object)?;
        let (_, _, e) = v.iter().find(|(a, b, _)| (*a..=*b).contains(&tri))?;
        self.pick(*e)
    }

    /// Bambu Studio and OrcaSlicer (`model_settings.config`, `project_settings.config`)
    /// or PrusaSlicer (`Slic3r_PE_model.config`, `Slic3r_PE.config`).
    fn read<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>) -> Slicer {
        let mut text = |name: &str| -> Option<String> {
            let real = zip.file_names().find(|n| n.eq_ignore_ascii_case(name))?.to_string();
            let mut f = zip.by_name(&real).ok()?;
            if f.size() > 32 << 20 {
                return None;
            }
            let mut s = String::new();
            f.read_to_string(&mut s).ok()?;
            Some(s)
        };
        let mut sl = Slicer::default();
        if let Some(cfg) = text("Metadata/project_settings.config") {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&cfg) {
                sl.palette = v["filament_colour"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|c| c.as_str().and_then(parse_colour).unwrap_or(GRAY))
                    .collect();
            }
            if let Some(xml) = text("Metadata/model_settings.config") {
                let (mut obj, mut part): (Option<String>, Option<String>) = (None, None);
                for t in tags(&xml) {
                    match (t.name, t.closing) {
                        ("object", false) if !t.empty => obj = t.get("id").map(String::from),
                        ("object", true) => obj = None,
                        ("part", false) if !t.empty => part = t.get("id").map(String::from),
                        ("part", true) => part = None,
                        ("metadata", false) if t.get("key") == Some("extruder") => {
                            let Some(e) = t.get("value").and_then(|v| v.trim().parse().ok()) else {
                                continue;
                            };
                            match (&obj, &part) {
                                (Some(o), Some(p)) => {
                                    sl.parts.insert((o.clone(), p.clone()), e);
                                }
                                (Some(o), None) => {
                                    sl.objects.insert(o.clone(), e);
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }
            }
        } else if let Some(cfg) = text("Metadata/Slic3r_PE.config") {
            let list = |key: &str| -> Vec<String> {
                cfg.lines()
                    .filter_map(|l| l.trim_start_matches(';').trim().strip_prefix(key))
                    .filter_map(|r| r.trim_start().strip_prefix('='))
                    .next()
                    .map(|v| v.split(';').map(|x| x.trim().trim_matches('"').to_string()).collect())
                    .unwrap_or_default()
            };
            let (ext, fil) = (list("extruder_colour"), list("filament_colour"));
            sl.palette = (0..ext.len().max(fil.len()))
                .map(|i| {
                    ext.get(i)
                        .and_then(|c| parse_colour(c))
                        .or_else(|| fil.get(i).and_then(|c| parse_colour(c)))
                        .unwrap_or(GRAY)
                })
                .collect();
            if let Some(xml) = text("Metadata/Slic3r_PE_model.config") {
                let (mut obj, mut vol): (Option<String>, Option<(usize, usize)>) = (None, None);
                for t in tags(&xml) {
                    match (t.name, t.closing) {
                        ("object", false) if !t.empty => obj = t.get("id").map(String::from),
                        ("object", true) => obj = None,
                        ("volume", false) if !t.empty => {
                            let n = |k: &str| t.get(k).and_then(|v| v.trim().parse().ok());
                            vol = n("firstid").zip(n("lastid"));
                        }
                        ("volume", true) => vol = None,
                        ("metadata", false) if t.get("key") == Some("extruder") => {
                            let Some(e) = t.get("value").and_then(|v| v.trim().parse::<usize>().ok()) else {
                                continue;
                            };
                            let Some(o) = &obj else { continue };
                            match vol {
                                Some((a, b)) if e > 0 => sl.volumes.entry(o.clone()).or_default().push((a, b, e)),
                                None if e > 0 => {
                                    sl.objects.insert(o.clone(), e);
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        sl
    }
}

struct ThreeMf<R: Read + std::io::Seek> {
    zip: zip::ZipArchive<R>,
    files: HashMap<String, ModelFile>,
    slicer: Slicer,
}

/// Where emitted triangles go: their positions and colours.
#[derive(Default)]
struct Out {
    tris: Tris,
    colours: Vec<Option<Rgb>>,
}

impl<R: Read + std::io::Seek> ThreeMf<R> {
    fn entry_name(&self, path: &str) -> Option<String> {
        let want = path.trim_start_matches('/').to_lowercase();
        self.zip
            .file_names()
            .find(|n| n.to_lowercase() == want)
            .map(String::from)
    }

    fn load(&mut self, path: &str) -> Result<()> {
        if self.files.contains_key(path) {
            return Ok(());
        }
        let name = self
            .entry_name(path)
            .ok_or_else(|| anyhow!("the 3MF has no {path}"))?;
        let mut xml = String::new();
        self.zip.by_name(&name)?.read_to_string(&mut xml)?;
        self.files.insert(path.to_string(), parse_model(&xml));
        Ok(())
    }

    /// Emit object `id` of `file` and its components. `top`: the build item's
    /// object, when this is one of its components; `inherited`: the colour it
    /// passes down.
    #[allow(clippy::too_many_arguments)]
    fn emit(
        &mut self,
        file: &str,
        id: &str,
        m: &Transform,
        out: &mut Out,
        top: Option<&str>,
        inherited: Option<Rgb>,
        depth: usize,
    ) -> Result<()> {
        if depth > 16 {
            bail!("the 3MF's components nest too deep");
        }
        self.load(file)?;
        let mf = &self.files[file];
        let Some(o) = mf.objects.get(id) else {
            return Ok(());
        };
        let own = match top {
            None => self.slicer.object(id),
            Some(t) => self.slicer.part(t, id),
        }
        .or(inherited);
        let objects = mf.colour(&o.prop);
        for (k, t) in o.tris.iter().enumerate() {
            for &i in t {
                let v = o
                    .verts
                    .get(i as usize)
                    .ok_or_else(|| anyhow!("a triangle uses a missing vertex"))?;
                out.tris.extend_from_slice(&apply(m, *v));
            }
            let c = o
                .props
                .get(k)
                .and_then(|p| mf.colour(p))
                .or(objects)
                .or_else(|| if top.is_none() { self.slicer.volume(id, k) } else { None })
                .or(own);
            out.colours.push(c);
        }
        if out.tris.len() > MAX_TRIANGLES * 9 {
            bail!("too many triangles");
        }
        let comps = o.comps.clone();
        let top = top.unwrap_or(id).to_string();
        for (path, cid, t) in comps {
            let f = path.unwrap_or_else(|| file.to_string());
            self.emit(&f, &cid, &compose(m, &t), out, Some(&top), own, depth + 1)?;
        }
        Ok(())
    }
}

fn read_3mf(b: &[u8]) -> Result<Mesh> {
    let mut zip = zip::ZipArchive::new(Cursor::new(b)).context("this 3MF isn't a valid ZIP")?;
    let slicer = Slicer::read(&mut zip);
    let mut m = ThreeMf {
        zip,
        files: HashMap::new(),
        slicer,
    };
    // the root model part, from _rels/.rels
    let mut root = "3D/3dmodel.model".to_string();
    if let Some(t) = m.relationship("/3dmodel") {
        root = t;
    }
    m.load(&root)?;
    let mut out = Out::default();
    let build = m.files[&root].build.clone();
    if build.is_empty() {
        let ids: Vec<String> = m.files[&root].objects.keys().cloned().collect();
        for id in ids {
            m.emit(&root.clone(), &id, &IDENTITY, &mut out, None, None, 0)?;
        }
    } else {
        for (path, id, t) in build {
            let f = path.unwrap_or_else(|| root.clone());
            m.emit(&f, &id, &t, &mut out, None, None, 0)?;
        }
    }
    if out.tris.is_empty() {
        bail!("no meshes in this 3MF");
    }
    let coloured = out.colours.iter().any(Option::is_some);
    Ok(Mesh {
        tris: out.tris,
        colours: coloured.then_some(out.colours),
    })
}

impl<R: Read + std::io::Seek> ThreeMf<R> {
    /// The target of the package relationship whose type ends with `kind`.
    fn relationship(&mut self, kind: &str) -> Option<String> {
        let rels = self.entry_name("_rels/.rels")?;
        let mut xml = String::new();
        self.zip.by_name(&rels).ok()?.read_to_string(&mut xml).ok()?;
        let target = tags(&xml)
            .find(|t| t.name == "Relationship" && t.get("Type").is_some_and(|ty| ty.ends_with(kind)))
            .and_then(|t| t.get("Target").map(|v| v.trim_start_matches('/').to_string()));
        target
    }
}

/// The picture a slicer saved in a 3MF (docs/PLAN.md, "Group 3 design", R11):
/// the package's thumbnail, else Bambu and Orca's plate pictures. PNG only.
pub fn picture_3mf(b: &[u8]) -> Option<Vec<u8>> {
    let zip = zip::ZipArchive::new(Cursor::new(b)).ok()?;
    let mut m = ThreeMf {
        zip,
        files: HashMap::new(),
        slicer: Slicer::default(),
    };
    let mut names: Vec<String> = m.relationship("/thumbnail").into_iter().collect();
    names.extend(
        ["Metadata/thumbnail.png", "Metadata/plate_1.png", "Metadata/top_1.png"].map(String::from),
    );
    for n in names {
        let Some(real) = m.entry_name(&n) else { continue };
        let Ok(mut f) = m.zip.by_name(&real) else { continue };
        if f.size() > 16 << 20 {
            continue;
        }
        let mut png = vec![];
        if f.read_to_end(&mut png).is_ok() && png.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Some(png);
        }
    }
    None
}

/// Binary STL of the triangles (normals worked out), for the page's viewer.
pub fn to_stl(t: &[f32]) -> Vec<u8> {
    to_stl_coloured(t, None)
}

/// As [`to_stl`], with each triangle's colour when there are some, the way
/// Materialise Magics writes them: "COLOR=" and a default colour in the header,
/// then in each triangle's attribute word 5 bits each of red, green and blue
/// (bit 15 set: the default colour).
pub fn to_stl_coloured(t: &[f32], colours: Option<&[Option<Rgb>]>) -> Vec<u8> {
    let n = t.len() / 9;
    let mut out = Vec::with_capacity(84 + n * 50);
    let mut header = [0u8; 80];
    if colours.is_some() {
        header[..6].copy_from_slice(b"COLOR=");
        header[6..9].copy_from_slice(&GRAY);
        header[9] = 255;
    }
    out.extend_from_slice(&header);
    out.extend_from_slice(&(n as u32).to_le_bytes());
    for (k, tri) in t.chunks_exact(9).enumerate() {
        for v in normal(tri) {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for v in tri {
            out.extend_from_slice(&v.to_le_bytes());
        }
        let word: u16 = match colours.and_then(|c| c.get(k).copied().flatten()) {
            Some([r, g, b]) => (r as u16 >> 3) | ((g as u16 >> 3) << 5) | ((b as u16 >> 3) << 10),
            None if colours.is_some() => 0x8000,
            None => 0,
        };
        out.extend_from_slice(&word.to_le_bytes());
    }
    out
}

/// A triangle's unit normal (zero for a degenerate one).
pub fn normal(t: &[f32]) -> [f32; 3] {
    let (a, b) = (
        [t[3] - t[0], t[4] - t[1], t[5] - t[2]],
        [t[6] - t[0], t[7] - t[1], t[8] - t[2]],
    );
    let n = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if l > 0.0 {
        [n[0] / l, n[1] / l, n[2] / l]
    } else {
        [0.0; 3]
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::io::Write;

    pub fn cube_stl_text() -> String {
        let mut s = String::from("solid cube\n");
        for t in [
            [0., 0., 0., 1., 0., 0., 1., 1., 0.],
            [0., 0., 0., 1., 1., 0., 0., 1., 0.],
            [0., 0., 1., 1., 0., 1., 1., 1., 1.],
        ] {
            s += "facet normal 0 0 0\nouter loop\n";
            for v in t.chunks(3) {
                s += &format!("vertex {} {} {}\n", v[0], v[1], v[2]);
            }
            s += "endloop\nendfacet\n";
        }
        s + "endsolid cube\n"
    }

    pub fn three_mf(split: bool) -> Vec<u8> {
        let mut buf = Cursor::new(vec![]);
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let o = zip::write::SimpleFileOptions::default();
            let mesh = r#"<object id="1" type="model"><mesh><vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices><triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object>"#;
            z.start_file("_rels/.rels", o).unwrap();
            z.write_all(br#"<?xml version="1.0"?><Relationships><Relationship Target="/3D/3dmodel.model" Id="r0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/></Relationships>"#).unwrap();
            if split {
                z.start_file("3D/Objects/object_1.model", o).unwrap();
                z.write_all(format!(r#"<model><resources>{mesh}</resources></model>"#).as_bytes())
                    .unwrap();
                z.start_file("3D/3dmodel.model", o).unwrap();
                z.write_all(br#"<model xmlns:p="x"><resources><object id="2"><components><component p:path="/3D/Objects/object_1.model" objectid="1" transform="1 0 0 0 1 0 0 0 1 10 0 0"/></components></object></resources><build><item objectid="2" transform="2 0 0 0 2 0 0 0 2 0 0 5"/></build></model>"#).unwrap();
            } else {
                z.start_file("3D/3dmodel.model", o).unwrap();
                z.write_all(format!(r#"<model><resources>{mesh}</resources><build><item objectid="1"/></build></model>"#).as_bytes()).unwrap();
            }
            z.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn reads_stl_both_ways() {
        let t = read("a.STL", cube_stl_text().as_bytes()).unwrap();
        assert_eq!(t.len(), 27);
        let b = to_stl(&t);
        assert_eq!(b.len(), 84 + 3 * 50);
        assert_eq!(read("b.stl", &b).unwrap(), t);
        // a binary STL whose header starts with "solid" is still binary
        let mut b2 = b.clone();
        b2[..5].copy_from_slice(b"solid");
        assert_eq!(read("c.stl", &b2).unwrap(), t);
    }

    #[test]
    fn reads_obj() {
        let t = read(
            "q.obj",
            b"v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nf 1/1 2/2 3/3 4/4\nf -4 -3 -2\n",
        )
        .unwrap();
        assert_eq!(t.len(), 27);
        assert_eq!(&t[9..18], &[0., 0., 0., 1., 1., 0., 0., 1., 0.]);
    }

    #[test]
    fn reads_3mf_with_components_in_other_files() {
        let t = read("a.3mf", &three_mf(false)).unwrap();
        assert_eq!(t, vec![0., 0., 0., 1., 0., 0., 0., 1., 0.]);
        // the component moves it 10 along x, then the build item doubles it and lifts it 5
        let t = read("b.3mf", &three_mf(true)).unwrap();
        assert_eq!(t, vec![20., 0., 5., 22., 0., 5., 20., 2., 5.]);
    }

    fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
        let mut buf = Cursor::new(vec![]);
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let o = zip::write::SimpleFileOptions::default();
            for (n, body) in files {
                z.start_file(*n, o).unwrap();
                z.write_all(body.as_bytes()).unwrap();
            }
            z.finish().unwrap();
        }
        buf.into_inner()
    }

    const TRI: &str = r#"<vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices>"#;

    #[test]
    fn reads_3mf_colour_groups_and_base_materials() {
        let model = format!(
            r##"<model><resources><basematerials id="5"><base name="PLA" displaycolor="#FF0000FF"/><base name="b" displaycolor="#00ff00"/></basematerials><m:colorgroup id="6"><m:color color="#0000FF"/></m:colorgroup><object id="1" pid="5" pindex="1"><mesh>{TRI}<triangles><triangle v1="0" v2="1" v3="2"/><triangle v1="0" v2="1" v3="2" pid="6" p1="0"/><triangle v1="0" v2="1" v3="2" p1="0"/></triangles></mesh></object><object id="2"><mesh>{TRI}<triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object></resources><build><item objectid="1"/><item objectid="2"/></build></model>"##
        );
        let m = read_mesh("c.3mf", &zip_of(&[("3D/3dmodel.model", &model)])).unwrap();
        assert_eq!(m.tris.len(), 36);
        // the object's own colour, a colour group's, the object's group by p1, none
        assert_eq!(m.colours.unwrap(), vec![Some([0, 255, 0]), Some([0, 0, 255]), Some([255, 0, 0]), None]);
        // a 3MF without colours has none
        assert!(read_mesh("a.3mf", &three_mf(false)).unwrap().colours.is_none());
    }

    #[test]
    fn reads_bambu_and_prusa_extruder_colours() {
        // Bambu/Orca: object 2's components are parts; part 1 prints with extruder 2
        let root = r#"<model xmlns:p="x"><resources><object id="2"><components><component p:path="/3D/Objects/object_1.model" objectid="1"/><component p:path="/3D/Objects/object_1.model" objectid="3"/></components></object></resources><build><item objectid="2"/></build></model>"#;
        let parts = format!(r#"<model><resources><object id="1"><mesh>{TRI}<triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object><object id="3"><mesh>{TRI}<triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object></resources></model>"#);
        let settings = r#"<config><object id="2"><metadata key="extruder" value="1"/><part id="1" subtype="normal_part"><metadata key="extruder" value="2"/></part><part id="3"/></object><plate><metadata key="extruder" value="9"/></plate></config>"#;
        let b = zip_of(&[
            ("3D/3dmodel.model", root),
            ("3D/Objects/object_1.model", &parts),
            ("Metadata/model_settings.config", settings),
            ("Metadata/project_settings.config", r##"{"filament_colour": ["#FFFFFF", "#FF8000FF"]}"##),
        ]);
        let m = read_mesh("b.3mf", &b).unwrap();
        assert_eq!(m.colours.unwrap(), vec![Some([255, 128, 0]), Some([255, 255, 255])]);

        // PrusaSlicer: the object prints with extruder 2, triangles 1-1 with extruder 1
        let model = format!(r#"<model><resources><object id="1"><mesh>{TRI}<triangles><triangle v1="0" v2="1" v3="2"/><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object></resources><build><item objectid="1"/></build></model>"#);
        let cfg = r#"<config><object id="1" instances_count="1"><metadata type="object" key="extruder" value="2"/><volume firstid="1" lastid="1"><metadata type="volume" key="extruder" value="1"/></volume></object></config>"#;
        let b = zip_of(&[
            ("3D/3dmodel.model", &model),
            ("Metadata/Slic3r_PE_model.config", cfg),
            ("Metadata/Slic3r_PE.config", "; extruder_colour = \"\";\"#00FF00\"\n; filament_colour = #FF0000;#0000FF\n"),
        ]);
        let m = read_mesh("p.3mf", &b).unwrap();
        assert_eq!(m.colours.unwrap(), vec![Some([0, 255, 0]), Some([255, 0, 0])]);
    }

    #[test]
    fn colours_travel_in_the_stl() {
        let t = read("a.3mf", &three_mf(false)).unwrap();
        let b = to_stl_coloured(&t, Some(&[Some([255, 0, 0])]));
        assert_eq!(&b[..6], b"COLOR=");
        assert_eq!(u16::from_le_bytes([b[84 + 48], b[84 + 49]]), 0x1f);
        let b = to_stl_coloured(&t, Some(&[None]));
        assert_eq!(u16::from_le_bytes([b[84 + 48], b[84 + 49]]), 0x8000);
        // still read as the same triangles
        assert_eq!(read("x.stl", &b).unwrap(), t);
    }

    #[test]
    fn finds_a_3mf_picture() {
        let png = "\u{89}PNG";
        assert!(picture_3mf(&three_mf(false)).is_none());
        let mut real = vec![0x89u8];
        real.extend_from_slice(b"PNG\r\n\x1a\nrest");
        let mut buf = Cursor::new(vec![]);
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let o = zip::write::SimpleFileOptions::default();
            z.start_file("Metadata/thumbnail.png", o).unwrap();
            z.write_all(png.as_bytes()).unwrap(); // not really a PNG: skipped
            z.start_file("Metadata/plate_1.png", o).unwrap();
            z.write_all(&real).unwrap();
            z.finish().unwrap();
        }
        assert_eq!(picture_3mf(&buf.into_inner()), Some(real));
    }

}
