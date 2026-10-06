//! Reading 3D files into triangles (docs/PLAN.md, "Phase 3 design"): STL (binary
//! and text), OBJ, and 3MF (including projects whose meshes live in separate
//! files, with component and build transforms). The page gets binary STL, so it
//! needs one loader; thumbnails are drawn from the same triangles.

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

/// Read a 3D file's triangles from its bytes, by the file's name.
pub fn read(name: &str, bytes: &[u8]) -> Result<Tris> {
    match ext(name).as_str() {
        "stl" => read_stl(bytes),
        "obj" => read_obj(bytes),
        "3mf" => read_3mf(bytes),
        e => bail!("can't show .{e} files in 3D"),
    }
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
    /// (model file, object id, transform)
    comps: Vec<(Option<String>, String, Transform)>,
}

struct ModelFile {
    objects: HashMap<String, Object>,
    build: Vec<(Option<String>, String, Transform)>,
}

fn parse_model(xml: &str) -> ModelFile {
    let mut objects = HashMap::new();
    let mut build = vec![];
    let mut cur: Option<(String, Object)> = None;
    let mut in_build = false;
    for t in tags(xml) {
        match (t.name, t.closing) {
            ("object", false) => {
                let id = t.get("id").unwrap_or("").to_string();
                if t.empty {
                    objects.insert(id, Object::default());
                } else {
                    cur = Some((id, Object::default()));
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
    ModelFile { objects, build }
}

struct ThreeMf<R: Read + std::io::Seek> {
    zip: zip::ZipArchive<R>,
    files: HashMap<String, ModelFile>,
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

    fn emit(
        &mut self,
        file: &str,
        id: &str,
        m: &Transform,
        out: &mut Tris,
        depth: usize,
    ) -> Result<()> {
        if depth > 16 {
            bail!("the 3MF's components nest too deep");
        }
        self.load(file)?;
        let Some(o) = self.files[file].objects.get(id) else {
            return Ok(());
        };
        for t in &o.tris {
            for &i in t {
                let v = o
                    .verts
                    .get(i as usize)
                    .ok_or_else(|| anyhow!("a triangle uses a missing vertex"))?;
                out.extend_from_slice(&apply(m, *v));
            }
        }
        if out.len() > MAX_TRIANGLES * 9 {
            bail!("too many triangles");
        }
        let comps = o.comps.clone();
        for (path, cid, t) in comps {
            let f = path.unwrap_or_else(|| file.to_string());
            self.emit(&f, &cid, &compose(m, &t), out, depth + 1)?;
        }
        Ok(())
    }
}

fn read_3mf(b: &[u8]) -> Result<Tris> {
    let zip = zip::ZipArchive::new(Cursor::new(b)).context("this 3MF isn't a valid ZIP")?;
    let mut m = ThreeMf {
        zip,
        files: HashMap::new(),
    };
    // the root model part, from _rels/.rels
    let mut root = "3D/3dmodel.model".to_string();
    if let Some(rels) = m.entry_name("_rels/.rels") {
        let mut xml = String::new();
        m.zip.by_name(&rels)?.read_to_string(&mut xml)?;
        let target = tags(&xml)
            .find(|t| {
                t.name == "Relationship" && t.get("Type").is_some_and(|ty| ty.ends_with("/3dmodel"))
            })
            .and_then(|t| {
                t.get("Target")
                    .map(|v| v.trim_start_matches('/').to_string())
            });
        if let Some(t) = target {
            root = t;
        }
    }
    m.load(&root)?;
    let mut out = vec![];
    let build = m.files[&root].build.clone();
    if build.is_empty() {
        let ids: Vec<String> = m.files[&root].objects.keys().cloned().collect();
        for id in ids {
            m.emit(&root.clone(), &id, &IDENTITY, &mut out, 0)?;
        }
    } else {
        for (path, id, t) in build {
            let f = path.unwrap_or_else(|| root.clone());
            m.emit(&f, &id, &t, &mut out, 0)?;
        }
    }
    if out.is_empty() {
        bail!("no meshes in this 3MF");
    }
    Ok(out)
}

/// Binary STL of the triangles (normals worked out), for the page's viewer.
pub fn to_stl(t: &[f32]) -> Vec<u8> {
    let n = t.len() / 9;
    let mut out = Vec::with_capacity(84 + n * 50);
    out.extend_from_slice(&[0u8; 80]);
    out.extend_from_slice(&(n as u32).to_le_bytes());
    for tri in t.chunks_exact(9) {
        for v in normal(tri) {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for v in tri {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&[0, 0]);
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
}
