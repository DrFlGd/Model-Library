//! Thumbnails (docs/PLAN.md, "Phase 3 design"): a small software renderer draws
//! a model's main 3D file from the viewer's three-quarter view into
//! `_thumbs/model.png` in the model folder, so the library carries its previews
//! and the Docker server can make them without a browser.

use crate::{archive, mesh, model};
use anyhow::{anyhow, bail, Result};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const THUMB: &str = "_thumbs/model.png";
const W: usize = 400;
const H: usize = 300;
/// Files bigger than this aren't drawn.
pub const MAX_BYTES: u64 = 500 << 20;
const COLOR: [f32; 3] = [242.0, 183.0, 5.0];

/// Variant folders that hold the model without supports or with them, by name.
pub fn variant_of(segment: &str) -> Option<&'static str> {
    let s: String = segment
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect();
    match s.as_str() {
        "presupported" | "supported" | "withsupports" | "supports" => Some("supported"),
        "unsupported" | "nosupports" | "nosupport" | "withoutsupports" => Some("unsupported"),
        _ => None,
    }
}

/// The file a model's thumbnail is drawn from: (file in the folder, entry inside it
/// if it's a ZIP). The largest 3D file at the shallowest depth, avoiding
/// "unsupported" folders when there's another choice.
pub fn pick_main(dir: &Path) -> Option<(String, Option<String>)> {
    let files: Vec<(String, u64, PathBuf)> = model::list_files(dir)
        .into_iter()
        .map(|(r, s)| {
            let p = dir.join(&r);
            (r, s, p)
        })
        .collect();
    pick_main_in(&files)
}

/// As [`pick_main`], from a list of (path in the model, size, where it is).
pub fn pick_main_in(files: &[(String, u64, PathBuf)]) -> Option<(String, Option<String>)> {
    let score = |rel: &str, size: u64| {
        let depth = rel.matches('/').count();
        let unsup = rel.split('/').any(|s| variant_of(s) == Some("unsupported"));
        (unsup, depth, std::cmp::Reverse(size))
    };
    if let Some((rel, _, _)) = files
        .iter()
        .filter(|(r, s, _)| mesh::readable(r) && *s <= MAX_BYTES && !r.starts_with("_thumbs/"))
        .min_by_key(|(r, s, _)| score(r, *s))
    {
        return Some((rel.clone(), None));
    }
    for (rel, _, at) in files.iter().filter(|(r, _, _)| archive::is_zip(r)) {
        let Ok(entries) = archive::list(at) else {
            continue;
        };
        let best = entries
            .iter()
            .filter_map(|e| Some((e["name"].as_str()?.to_string(), e["size"].as_u64()?)))
            .filter(|(n, s)| mesh::readable(n) && *s <= MAX_BYTES)
            .min_by_key(|(n, s)| score(n, *s));
        if let Some((name, _)) = best {
            return Some((rel.clone(), Some(name)));
        }
    }
    None
}

/// A file's bytes, or an entry's inside a ZIP.
pub fn file_bytes(dir: &Path, rel: &str, entry: Option<&str>) -> Result<Vec<u8>> {
    bytes_at(&dir.join(crate::library::rel_inside(rel)?), entry)
}

/// The bytes of the file at `path`, or of an entry inside it (a ZIP).
pub fn bytes_at(path: &Path, entry: Option<&str>) -> Result<Vec<u8>> {
    match entry {
        Some(e) => archive::read(path, e, MAX_BYTES),
        None => {
            let len = std::fs::metadata(path)?.len();
            if len > MAX_BYTES {
                bail!(
                    "{} is too big to show ({} MB)",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    len >> 20
                );
            }
            Ok(std::fs::read(path)?)
        }
    }
}

/// A preview of one 3D file (or an entry in a ZIP), drawn once and kept in
/// `cache` (the app's data folder, not the library) under a name made from the
/// file's path, size and time. Returns that name.
pub fn file_preview(cache: &Path, path: &Path, entry: Option<&str>) -> Result<String> {
    use sha2::{Digest, Sha256};
    let meta = std::fs::metadata(path)?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut h = Sha256::new();
    h.update(path.to_string_lossy().as_bytes());
    h.update([0]);
    h.update(entry.unwrap_or("").as_bytes());
    h.update(format!("\0{}\0{modified}", meta.len()).as_bytes());
    let name = format!("{}.png", &hex::encode(h.finalize())[..32]);
    let out = cache.join(&name);
    if out.is_file() {
        return Ok(name);
    }
    let name_in = entry.map(String::from).unwrap_or_else(|| {
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    });
    if !mesh::readable(&name_in) {
        bail!("{name_in} can't be drawn");
    }
    let bytes = bytes_at(path, entry)?;
    let png = render(&mesh::read(&name_in, &bytes)?)?;
    crate::config::write_atomic(&out, &png)?;
    prune_cache(cache);
    Ok(name)
}

/// Keep the preview cache to its newest few thousand pictures.
fn prune_cache(cache: &Path) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static WRITES: AtomicU32 = AtomicU32::new(0);
    if WRITES.fetch_add(1, Ordering::Relaxed) % 200 != 199 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(cache) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = rd
        .flatten()
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    const KEEP: usize = 5000;
    if files.len() > KEEP {
        files.sort();
        for (_, p) in &files[..files.len() - KEEP] {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Draw the model in `dir` into its `_thumbs/model.png`. Ok(false) when it has no
/// 3D file this app can read.
pub fn make(dir: &Path) -> Result<bool> {
    let Some((rel, entry)) = pick_main(dir) else {
        return Ok(false);
    };
    let bytes = file_bytes(dir, &rel, entry.as_deref())?;
    let tris = mesh::read(entry.as_deref().unwrap_or(&rel), &bytes)?;
    let png = render(&tris)?;
    let out: PathBuf = dir.join(THUMB);
    crate::config::write_atomic(&out, &png)?;
    Ok(true)
}

/// Whether the model in `dir` has a thumbnail.
pub fn has(dir: &Path) -> bool {
    dir.join(THUMB).is_file()
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt().max(1e-12);
    [a[0] / l, a[1] / l, a[2] / l]
}

/// Render triangles (Z up) as a PNG: orthographic three-quarter view, flat shaded,
/// transparent background, drawn at twice the size and scaled down for smooth edges.
pub fn render(tris: &[f32]) -> Result<Vec<u8>> {
    if tris.len() < 9 {
        bail!("nothing to draw");
    }
    let (sw, sh) = (W * 2, H * 2);
    let view = norm([0.62, -0.95, 0.75]); // towards the camera
    let right = norm(cross([-view[0], -view[1], -view[2]], [0.0, 0.0, 1.0]));
    let up = cross(right, [-view[0], -view[1], -view[2]]);
    let light = norm([1.0, -1.5, 2.5]);
    let fill = norm([-2.0, 2.0, 1.0]);
    // projected bounds
    let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for p in tris.chunks_exact(3) {
        let p = [p[0], p[1], p[2]];
        if !p.iter().all(|v| v.is_finite()) {
            continue;
        }
        let (x, y) = (dot(p, right), dot(p, up));
        x0 = x0.min(x);
        x1 = x1.max(x);
        y0 = y0.min(y);
        y1 = y1.max(y);
    }
    if x0 > x1 {
        bail!("nothing to draw");
    }
    let scale =
        ((sw as f32 * 0.9) / (x1 - x0).max(1e-6)).min((sh as f32 * 0.9) / (y1 - y0).max(1e-6));
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let mut depth = vec![f32::MIN; sw * sh];
    let mut color = vec![[0u8; 4]; sw * sh];
    for t in tris.chunks_exact(9) {
        let v = [[t[0], t[1], t[2]], [t[3], t[4], t[5]], [t[6], t[7], t[8]]];
        if !t.iter().all(|x| x.is_finite()) {
            continue;
        }
        let n = norm(cross(sub(v[1], v[0]), sub(v[2], v[0])));
        // either side may face us (meshes from the wild aren't always wound right)
        let n = if dot(n, view) < 0.0 {
            [-n[0], -n[1], -n[2]]
        } else {
            n
        };
        let shade = (0.38
            + 0.55 * dot(n, light).max(0.0)
            + 0.2 * dot(n, fill).max(0.0)
            + 0.12 * dot(n, view).max(0.0))
        .min(1.15);
        let rgba = [
            (COLOR[0] * shade).min(255.0) as u8,
            (COLOR[1] * shade).min(255.0) as u8,
            (COLOR[2] * shade).min(255.0) as u8,
            255,
        ];
        let s: Vec<[f32; 3]> = v
            .iter()
            .map(|p| {
                [
                    sw as f32 / 2.0 + (dot(*p, right) - cx) * scale,
                    sh as f32 / 2.0 - (dot(*p, up) - cy) * scale,
                    dot(*p, view),
                ]
            })
            .collect();
        let area =
            (s[1][0] - s[0][0]) * (s[2][1] - s[0][1]) - (s[2][0] - s[0][0]) * (s[1][1] - s[0][1]);
        if area.abs() < 1e-9 {
            continue;
        }
        let minx = s
            .iter()
            .map(|p| p[0])
            .fold(f32::MAX, f32::min)
            .floor()
            .max(0.0) as usize;
        let maxx =
            (s.iter().map(|p| p[0]).fold(f32::MIN, f32::max).ceil() as isize).min(sw as isize - 1);
        let miny = s
            .iter()
            .map(|p| p[1])
            .fold(f32::MAX, f32::min)
            .floor()
            .max(0.0) as usize;
        let maxy =
            (s.iter().map(|p| p[1]).fold(f32::MIN, f32::max).ceil() as isize).min(sh as isize - 1);
        if maxx < 0 || maxy < 0 {
            continue;
        }
        for y in miny..=maxy as usize {
            for x in minx..=maxx as usize {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let w0 = ((s[1][0] - px) * (s[2][1] - py) - (s[2][0] - px) * (s[1][1] - py)) / area;
                let w1 = ((s[2][0] - px) * (s[0][1] - py) - (s[0][0] - px) * (s[2][1] - py)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                    continue;
                }
                let z = w0 * s[0][2] + w1 * s[1][2] + w2 * s[2][2];
                let i = y * sw + x;
                if z > depth[i] {
                    depth[i] = z;
                    color[i] = rgba;
                }
            }
        }
    }
    // scale down 2x2 -> 1 (premultiplied, so edges blend into the transparent background)
    let mut px = vec![0u8; W * H * 4];
    for y in 0..H {
        for x in 0..W {
            let mut acc = [0u32; 4];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let c = color[(y * 2 + dy) * sw + x * 2 + dx];
                let a = c[3] as u32;
                for k in 0..3 {
                    acc[k] += c[k] as u32 * a / 255;
                }
                acc[3] += a;
            }
            let o = (y * W + x) * 4;
            let a = acc[3] / 4;
            for k in 0..3 {
                px[o + k] = (acc[k] * 255).checked_div(acc[3]).unwrap_or(0).min(255) as u8;
            }
            px[o + 3] = a as u8;
        }
    }
    png(W, H, &px)
}

/// An RGBA PNG.
pub fn png(w: usize, h: usize, rgba: &[u8]) -> Result<Vec<u8>> {
    if rgba.len() != w * h * 4 {
        return Err(anyhow!("wrong pixel count"));
    }
    let mut raw = Vec::with_capacity(h * (w * 4 + 1));
    for row in rgba.chunks_exact(w * 4) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = flate2::write::ZlibEncoder::new(vec![], flate2::Compression::default());
    z.write_all(&raw)?;
    let idat = z.finish()?;
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let chunk = |out: &mut Vec<u8>, kind: &[u8], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let mut c = crc32fast::Hasher::new();
        c.update(kind);
        c.update(data);
        out.extend_from_slice(&c.finalize().to_be_bytes());
    };
    let mut ihdr = vec![];
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &idat);
    chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_a_model_into_its_folder() {
        let d = std::env::temp_dir().join(format!("modlib-thumb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("Unsupported")).unwrap();
        std::fs::create_dir_all(d.join("Supported")).unwrap();
        let cube = mesh::tests::cube_stl_text();
        std::fs::write(d.join("Unsupported/big.stl"), cube.repeat(3)).unwrap();
        std::fs::write(d.join("Supported/small.stl"), &cube).unwrap();
        std::fs::write(d.join("Supported/tiny.stl"), "solid x\nendsolid\n").unwrap();
        assert_eq!(pick_main(&d), Some(("Supported/small.stl".into(), None)));
        assert!(make(&d).unwrap());
        let png = std::fs::read(d.join(THUMB)).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        assert_eq!(
            u32::from_be_bytes(png[16..20].try_into().unwrap()),
            W as u32
        );
        // something was drawn, not a blank image
        let mut z = flate2::read::ZlibDecoder::new(&png[41..png.len() - 12]);
        let mut raw = vec![];
        std::io::Read::read_to_end(&mut z, &mut raw).unwrap();
        let solid = raw
            .chunks(W * 4 + 1)
            .flat_map(|r| r[1..].chunks(4))
            .filter(|p| p[3] == 255)
            .count();
        assert!(solid > 1000, "{solid}");
        // a ZIP with a 3MF inside, and nothing else
        let z = d.join("z");
        std::fs::create_dir_all(&z).unwrap();
        {
            use std::io::Write;
            let mut w = zip::ZipWriter::new(std::fs::File::create(z.join("model.zip")).unwrap());
            w.start_file("inner/m.3mf", zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(&mesh::tests::three_mf(true)).unwrap();
            w.finish().unwrap();
        }
        assert_eq!(
            pick_main(&z),
            Some(("model.zip".into(), Some("inner/m.3mf".into())))
        );
        assert!(make(&z).unwrap());
        assert!(!make(&d.join("Supported/..").join("nothing-here")).unwrap_or(false));
        let _ = std::fs::remove_dir_all(&d);
    }
}
