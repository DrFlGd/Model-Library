//! Reading inside ZIP archives without unpacking them (docs/PLAN.md, "Phase 3
//! design"): listing the entries, and reading one (a 3D file or a picture).

use crate::model::file_kind;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::io::Read;
use std::path::Path;

/// Whether a file is an archive this app can look inside.
pub fn is_zip(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".zip")
}

// ZIP dates have no timezone. Treat their calendar values consistently for sorting.
fn modified_millis(d: zip::DateTime) -> i64 {
    let month = i64::from(d.month());
    let year = i64::from(d.year()) - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let y = year - era * 400;
    let m = month + if month > 2 { -3 } else { 9 };
    let days = era * 146097 + y * 365 + y / 4 - y / 100
        + (153 * m + 2) / 5 + i64::from(d.day()) - 1 - 719468;
    (days * 86400 + i64::from(d.hour()) * 3600 + i64::from(d.minute()) * 60 + i64::from(d.second())) * 1000
}

/// The files in a ZIP: [{ name, size, kind, modified }], folders left out, sorted by name.
pub fn list(path: &Path) -> Result<Vec<Value>> {
    let f = std::fs::File::open(path).with_context(|| format!("can't open {}", path.display()))?;
    let mut z = zip::ZipArchive::new(f).context("this isn't a ZIP the app can read")?;
    let mut out = vec![];
    for i in 0..z.len() {
        let e = z.by_index_raw(i)?;
        if e.is_dir() {
            continue;
        }
        let name = e.name().to_string();
        let base = name.rsplit('/').next().unwrap_or(&name);
        if name.starts_with("__MACOSX/") || base.starts_with("._") || base == ".DS_Store" {
            continue;
        }
        out.push(json!({ "name": name, "size": e.size(), "kind": file_kind(&name), "modified": e.last_modified().map(modified_millis).unwrap_or(0) }));
    }
    out.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    Ok(out)
}

/// One entry's bytes (at most `limit`).
pub fn read(path: &Path, entry: &str, limit: u64) -> Result<Vec<u8>> {
    let f = std::fs::File::open(path).with_context(|| format!("can't open {}", path.display()))?;
    let mut z = zip::ZipArchive::new(f).context("this isn't a ZIP the app can read")?;
    let mut e = z
        .by_name(entry)
        .map_err(|_| anyhow!("{entry} isn't in the archive"))?;
    if e.size() > limit {
        bail!("{entry} is too big to show ({} MB)", e.size() >> 20);
    }
    let mut out = Vec::with_capacity(e.size() as usize);
    e.read_to_end(&mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn lists_and_reads_entries() {
        let d = std::env::temp_dir().join(format!("modlib-zip-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("m.zip");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&p).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            z.add_directory("Dragon/", o).unwrap();
            z.start_file("Dragon/body.stl", o).unwrap();
            z.write_all(b"solid x").unwrap();
            z.start_file("Dragon/photo.jpg", o).unwrap();
            z.write_all(b"jpg").unwrap();
            z.start_file("__MACOSX/Dragon/._body.stl", o).unwrap();
            z.finish().unwrap();
        }
        let l = list(&p).unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(
            l[0],
            json!({ "name": "Dragon/body.stl", "size": 7, "kind": "model", "modified": 315532800000i64 })
        );
        assert_eq!(read(&p, "Dragon/photo.jpg", 100).unwrap(), b"jpg");
        assert!(read(&p, "Dragon/body.stl", 3).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
