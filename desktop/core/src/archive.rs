//! Reading inside archives without unpacking them into the library (docs/PLAN.md,
//! "Phase 3 design" and "Group 4 design"): listing the entries, and reading one
//! (a 3D file or a picture). A ZIP is read one entry at a time. A 7z or RAR often
//! can't be (a solid archive has to be unpacked from the start to reach a file),
//! so it's unpacked once into the data folder's `archive-cache` and read from there.

use crate::model::file_kind;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

/// What a locked archive says; the page asks for the password when it sees it.
pub const LOCKED: &str = "This archive is locked with a password.";
/// What a wrong password says; the page asks again.
pub const WRONG_PASSWORD: &str = "That password didn't open the archive.";
/// What a 7z or RAR too big to unpack in passing says; the page opens it as a job.
pub const NOT_OPEN: &str = "This archive needs opening first.";
/// 7z and RAR archives up to this size are unpacked as soon as they're looked
/// inside (32 MB; the page checks set a smaller size to see the job).
pub fn open_at_once() -> u64 {
    std::env::var("MODLIB_ARCHIVE_OPEN_AT_ONCE").ok().and_then(|v| v.parse().ok()).unwrap_or(32 << 20)
}
/// The most an archive may unpack to in the cache.
const MAX_OPEN: u64 = 16 << 30;
/// The cache keeps archives used in the last 14 days, up to 10 GB.
const CACHE_DAYS: u64 = 14;
const CACHE_BYTES: u64 = 10 << 30;

/// Validate an entry before materialising it outside its archive. In particular,
/// backslashes are separators on Windows, not innocuous file-name characters.
pub fn safe_entry(name: &str) -> Result<PathBuf> {
    if name.is_empty() || name.starts_with('/') || (name.contains('\\') || name.contains(':') || name.contains('\0'))
        || name.split('/').any(|part| part.is_empty() || part == "." || part == "..")
        || name.chars().any(|c| c.is_control()) {
        bail!("The archive contains an unsafe path: {name}");
    }
    // Reserved Windows device names are unsafe even when they have extensions.
    for part in name.split('/') {
        let stem = part.split('.').next().unwrap_or("").trim_end_matches(' ').to_ascii_uppercase();
        if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "COM1" | "COM2" | "COM3" | "COM4" | "COM5" | "COM6" | "COM7" | "COM8" | "COM9" | "LPT1" | "LPT2" | "LPT3" | "LPT4" | "LPT5" | "LPT6" | "LPT7" | "LPT8" | "LPT9")
            || (part.ends_with(' ') || part.ends_with('.')) {
            bail!("The archive contains a path unavailable on Windows: {name}");
        }
    }
    Ok(PathBuf::from(name))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format { Zip, SevenZ, Rar }

/// The number in "name.part3.rar", if it's named that way.
fn rar_part(lower: &str) -> Option<u32> {
    let stem = lower.strip_suffix(".rar")?;
    let (_, n) = stem.rsplit_once(".part")?;
    (!n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())).then(|| n.parse().ok()).flatten()
}

/// Which kind of archive a file is, when the app can look inside it: a ZIP, a 7z
/// or a RAR, or the first file of a set (`name.7z.001`, `name.part1.rar`).
pub fn format(name: &str) -> Option<Format> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".zip") {
        Some(Format::Zip)
    } else if lower.ends_with(".7z") || lower.ends_with(".7z.001") {
        Some(Format::SevenZ)
    } else if lower.ends_with(".rar") {
        match rar_part(&lower) { Some(n) if n != 1 => None, _ => Some(Format::Rar) }
    } else {
        None
    }
}

/// Whether a file is an archive this app can look inside.
pub fn is_archive(name: &str) -> bool {
    format(name).is_some()
}

/// Whether a file is a ZIP (the only kind the app makes).
pub fn is_zip(name: &str) -> bool {
    format(name) == Some(Format::Zip)
}

/// Whether a file is one of a set's later files (`name.7z.002`, `name.part2.rar`,
/// `name.r00`): an archive, but opened through the set's first file.
pub fn is_part(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if let Some((stem, n)) = lower.rsplit_once('.') {
        if n.len() == 3 && digits(n) && stem.ends_with(".7z") { return n != "001"; }
        if n.len() == 3 && n.starts_with('r') && digits(&n[1..]) { return true; }
    }
    rar_part(&lower).is_some_and(|n| n != 1)
}

// ---- passwords (kept in memory until the app closes, never written down) ----

fn passwords() -> &'static Mutex<HashMap<PathBuf, String>> {
    static P: OnceLock<Mutex<HashMap<PathBuf, String>>> = OnceLock::new();
    P.get_or_init(Default::default)
}

/// Remember the password for an archive while the app runs.
pub fn set_password(path: &Path, password: &str) {
    passwords().lock().unwrap().insert(path.to_path_buf(), password.to_string());
}

fn password(path: &Path) -> Option<String> {
    passwords().lock().unwrap().get(path).cloned()
}

// ---- the cache ----

fn cache_root() -> &'static Mutex<Option<PathBuf>> {
    static C: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    C.get_or_init(Default::default)
}

/// Where 7z and RAR archives are unpacked (the app's data folder).
pub fn set_cache(dir: &Path) {
    *cache_root().lock().unwrap() = Some(dir.join("archive-cache"));
}

fn cache() -> PathBuf {
    cache_root().lock().unwrap().clone().unwrap_or_else(|| std::env::temp_dir().join("modlib-archive-cache"))
}

/// The files of a set, first one first (one file unless it's a split 7z).
fn volumes(path: &Path) -> Vec<PathBuf> {
    let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
    let Some(stem) = name.len().checked_sub(4).filter(|_| name.to_ascii_lowercase().ends_with(".7z.001")).map(|n| &name[..n]) else {
        return vec![path.to_path_buf()];
    };
    let mut out = vec![];
    for n in 1..1000 {
        let p = path.with_file_name(format!("{stem}.{n:03}"));
        if !p.is_file() { break; }
        out.push(p);
    }
    out
}

/// The set's size on disk, all its files together.
pub fn set_size(path: &Path) -> u64 {
    match format(&path.to_string_lossy()) {
        Some(Format::Rar) => {
            let lower = path.file_name().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
            let first = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            let Some(dir) = path.parent() else { return first };
            // name.part1.rar → name.partN.rar; name.rar → name.rNN
            let (prefix, old) = match lower.strip_suffix(".rar").and_then(|s| s.rsplit_once(".part")) {
                Some((p, _)) => (format!("{p}.part"), false),
                None => (lower.trim_end_matches(".rar").to_string() + ".r", true),
            };
            let mut total = first;
            for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let n = e.file_name().to_string_lossy().to_ascii_lowercase();
                let more = n.strip_prefix(&prefix).is_some_and(|rest| if old { rest.len() == 2 && rest.bytes().all(|b| b.is_ascii_digit()) } else { rar_part(&n).is_some_and(|k| k != 1) });
                if more { total += e.metadata().map(|m| m.len()).unwrap_or(0); }
            }
            total
        }
        _ => volumes(path).iter().map(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)).sum(),
    }
}

/// The cache folder for this archive as it is now (path, size and date).
fn cache_dir(path: &Path) -> Result<PathBuf> {
    use sha2::{Digest, Sha256};
    let meta = std::fs::metadata(path).with_context(|| format!("can't open {}", path.display()))?;
    let stamp = meta.modified().ok().and_then(|d| d.duration_since(SystemTime::UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0);
    let key = format!("{}:{}:{stamp}:{}", path.display(), meta.len(), set_size(path));
    Ok(cache().join(&hex::encode(Sha256::digest(key.as_bytes()))[..24]))
}

/// The unpacked files of a 7z or RAR, when it has been opened.
fn opened(path: &Path) -> Option<PathBuf> {
    let dir = cache_dir(path).ok()?;
    let done = dir.join("done");
    if !done.is_file() { return None; }
    // the cache drops what hasn't been used for a while
    if let Ok(f) = File::options().write(true).open(&done) { let _ = f.set_modified(SystemTime::now()); }
    Some(dir.join("files"))
}

/// Whether looking inside this archive needs it opened first (a 7z or RAR
/// bigger than [`open_at_once`] that isn't in the cache yet).
pub fn needs_opening(path: &Path) -> bool {
    matches!(format(&path.to_string_lossy()), Some(Format::SevenZ | Format::Rar))
        && opened(path).is_none() && set_size(path) > open_at_once()
}

/// Unpack a 7z or RAR into the cache (once; later calls return at once), and
/// give the folder its files are in. `progress(done, total, name)` in bytes.
pub fn open(path: &Path, cancel: &AtomicBool, progress: &dyn Fn(u64, u64, &str)) -> Result<PathBuf> {
    if let Some(dir) = opened(path) { return Ok(dir); }
    let dir = cache_dir(path)?;
    let total: u64 = list(path)?.iter().filter_map(|e| e["size"].as_u64()).sum();
    if total > MAX_OPEN { bail!("This archive unpacks to {} GB, too much to look inside here. Use Extract archive… instead.", total >> 30); }
    let tmp = scratch();
    let files = tmp.join("files");
    std::fs::create_dir_all(&files)?;
    let result = (|| -> Result<()> {
        let mut done = 0u64;
        each(path, cancel, &mut |name, size, r| {
            let Ok(rel) = safe_entry(name) else { return Ok(()) };
            let to = files.join(rel);
            std::fs::create_dir_all(to.parent().unwrap())?;
            let mut out = File::create(&to)?;
            let n = io::copy(&mut r.take(size + 1), &mut out)?;
            if n > size { bail!("{name} unpacked bigger than the archive says."); }
            done += n;
            progress(done, total, name);
            Ok(())
        })?;
        std::fs::write(tmp.join("done"), b"")?;
        if dir.exists() { let _ = std::fs::remove_dir_all(&dir); }
        std::fs::rename(&tmp, &dir)?;
        Ok(())
    })();
    if result.is_err() { let _ = std::fs::remove_dir_all(&tmp); }
    result?;
    trim(&dir);
    Ok(dir.join("files"))
}

/// A new working folder in the cache (removed when done; `trim` clears any left over).
fn scratch() -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    cache().join(format!(".open-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)))
}

fn folder_size(p: &Path) -> u64 {
    std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| {
        let t = e.file_type();
        if t.as_ref().is_ok_and(|t| t.is_dir()) { folder_size(&e.path()) } else { e.metadata().map(|m| m.len()).unwrap_or(0) }
    }).sum()
}

/// Keep the cache to archives used lately and to its size, newest kept first.
fn trim(keep: &Path) {
    let now = SystemTime::now();
    let mut dirs = vec![];
    for e in std::fs::read_dir(cache()).into_iter().flatten().flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        let used = std::fs::metadata(p.join("done")).and_then(|m| m.modified())
            .or_else(|_| e.metadata().and_then(|m| m.modified())).unwrap_or(now);
        let age = now.duration_since(used).unwrap_or_default();
        if name.starts_with(".open-") {
            // left by a stop or a crash
            if age > Duration::from_secs(86400) { let _ = std::fs::remove_dir_all(&p); }
            continue;
        }
        if p != keep && age > Duration::from_secs(CACHE_DAYS * 86400) { let _ = std::fs::remove_dir_all(&p); continue; }
        dirs.push((used, folder_size(&p), p));
    }
    dirs.sort_by(|a, b| b.0.cmp(&a.0));
    let mut total = 0;
    for (_, size, p) in dirs {
        total += size;
        if total > CACHE_BYTES && p != keep { let _ = std::fs::remove_dir_all(&p); }
    }
}

// ---- reading ----

/// A split 7z's files read as one.
struct Joined { parts: Vec<(File, u64)>, pos: u64, len: u64 }

impl Joined {
    fn open(path: &Path) -> Result<Self> {
        let mut parts = vec![];
        for p in volumes(path) {
            let f = File::open(&p).with_context(|| format!("can't open {}", p.display()))?;
            let n = f.metadata()?.len();
            parts.push((f, n));
        }
        let len = parts.iter().map(|p| p.1).sum();
        Ok(Joined { parts, pos: 0, len })
    }
}

impl Read for Joined {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut at = self.pos;
        for (f, n) in &mut self.parts {
            if at < *n {
                f.seek(SeekFrom::Start(at))?;
                let want = buf.len().min((*n - at) as usize);
                let got = f.read(&mut buf[..want])?;
                self.pos += got as u64;
                return Ok(got);
            }
            at -= *n;
        }
        Ok(0)
    }
}

impl Seek for Joined {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let p = match to {
            SeekFrom::Start(n) => n as i128,
            SeekFrom::End(n) => self.len as i128 + n as i128,
            SeekFrom::Current(n) => self.pos as i128 + n as i128,
        };
        if p < 0 { return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek before the start")); }
        self.pos = p as u64;
        Ok(self.pos)
    }
}

fn seven_error(e: sevenz_rust2::Error, locked: bool) -> anyhow::Error {
    match e {
        sevenz_rust2::Error::PasswordRequired => anyhow!(LOCKED),
        sevenz_rust2::Error::MaybeBadPassword(_) => anyhow!(if locked { WRONG_PASSWORD } else { LOCKED }),
        e => anyhow!("This 7z can't be read: {e}"),
    }
}

fn seven(path: &Path) -> Result<sevenz_rust2::ArchiveReader<Joined>> {
    let pw = password(path);
    let p = pw.as_deref().map(sevenz_rust2::Password::from).unwrap_or_else(sevenz_rust2::Password::empty);
    sevenz_rust2::ArchiveReader::new(Joined::open(path)?, p).map_err(|e| seven_error(e, pw.is_some()))
}

fn rar_error(e: unrar::error::UnrarError, locked: bool) -> anyhow::Error {
    use unrar::error::Code;
    match e.code {
        Code::MissingPassword => anyhow!(if locked { WRONG_PASSWORD } else { LOCKED }),
        Code::BadPassword => anyhow!(WRONG_PASSWORD),
        Code::EOpen => anyhow!("A file of this RAR set is missing: {e}"),
        _ => anyhow!("This RAR can't be read: {e}"),
    }
}

fn rar_name(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

// ZIP and RAR dates have no timezone. Treat their calendar values consistently for sorting.
fn civil_millis(year: i64, month: i64, day: i64, hour: i64, minute: i64, second: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let y = year - era * 400;
    let m = month + if month > 2 { -3 } else { 9 };
    let days = era * 146097 + y * 365 + y / 4 - y / 100 + (153 * m + 2) / 5 + day - 1 - 719468;
    (days * 86400 + hour * 3600 + minute * 60 + second) * 1000
}

fn modified_millis(d: zip::DateTime) -> i64 {
    civil_millis(d.year().into(), d.month().into(), d.day().into(), d.hour().into(), d.minute().into(), d.second().into())
}

/// A RAR's MS-DOS date and time.
fn dos_millis(t: u32) -> i64 {
    let (date, time) = (i64::from(t >> 16), i64::from(t & 0xffff));
    if date == 0 { return 0; }
    civil_millis(1980 + (date >> 9), (date >> 5) & 15, date & 31, time >> 11, (time >> 5) & 63, (time & 31) * 2)
}

/// A 7z's Windows file time.
fn nt_millis(t: sevenz_rust2::NtTime) -> i64 {
    let ticks: u64 = t.into();
    if ticks == 0 { 0 } else { (ticks / 10_000) as i64 - 11_644_473_600_000 }
}

fn junk(name: &str) -> bool {
    let base = name.rsplit('/').next().unwrap_or(name);
    name.starts_with("__MACOSX/") || base.starts_with("._") || base == ".DS_Store"
}

/// The files in an archive: [{ name, size, kind, modified }], folders left out, sorted by name.
pub fn list(path: &Path) -> Result<Vec<Value>> {
    let mut out = vec![];
    let mut add = |name: String, size: u64, modified: i64| {
        if !junk(&name) {
            out.push(json!({ "name": name, "size": size, "kind": file_kind(&name), "modified": modified }));
        }
    };
    match format(&path.to_string_lossy()) {
        Some(Format::SevenZ) => {
            let r = seven(path)?;
            for e in &r.archive().files {
                if e.is_directory() || e.is_anti_item() { continue; }
                add(e.name().replace('\\', "/"), e.size(), nt_millis(e.last_modified_date()));
            }
        }
        Some(Format::Rar) => {
            let pw = password(path);
            let a = match &pw { Some(p) => unrar::Archive::with_password(path, p.as_bytes()), None => unrar::Archive::new(path) };
            for e in a.open_for_listing().map_err(|e| rar_error(e, pw.is_some()))? {
                let e = e.map_err(|e| rar_error(e, pw.is_some()))?;
                if e.is_directory() || e.is_split_before() { continue; }
                add(rar_name(&e.filename), e.unpacked_size, dos_millis(e.file_time));
            }
        }
        _ => {
            let f = File::open(path).with_context(|| format!("can't open {}", path.display()))?;
            let mut z = zip::ZipArchive::new(f).context("this isn't a ZIP the app can read")?;
            for i in 0..z.len() {
                let e = z.by_index_raw(i)?;
                if e.is_dir() { continue; }
                add(e.name().to_string(), e.size(), e.last_modified().map(modified_millis).unwrap_or(0));
            }
        }
    }
    out.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    Ok(out)
}

/// Every file in an archive, in its order, each with its size and its bytes to
/// read (a 7z or RAR is unpacked as it goes; nothing is cached). Stops between files.
pub fn each(path: &Path, cancel: &AtomicBool, f: &mut dyn FnMut(&str, u64, &mut dyn Read) -> Result<()>) -> Result<()> {
    let stopped = || if cancel.load(Ordering::Relaxed) { Err(anyhow!("Stopped.")) } else { Ok(()) };
    match format(&path.to_string_lossy()) {
        Some(Format::SevenZ) => {
            let locked = password(path).is_some();
            let mut r = seven(path)?;
            let mut failed = None;
            let result = r.for_each_entries(|e, data| {
                if e.is_directory() || e.is_anti_item() { return Ok(true); }
                match stopped().and_then(|_| f(&e.name().replace('\\', "/"), e.size(), data)) {
                    Ok(()) => Ok(true),
                    Err(err) => { failed = Some(err); Ok(false) }
                }
            });
            if let Some(e) = failed { return Err(e); }
            result.map_err(|e| seven_error(e, locked))
        }
        Some(Format::Rar) => {
            let pw = password(path);
            let a = match &pw { Some(p) => unrar::Archive::with_password(path, p.as_bytes()), None => unrar::Archive::new(path) };
            let err = |e| rar_error(e, pw.is_some());
            let mut at = a.open_for_processing().map_err(err)?;
            let scratch = scratch();
            std::fs::create_dir_all(&scratch)?;
            let result = (|| -> Result<()> {
                while let Some(h) = at.read_header().map_err(err)? {
                    stopped()?;
                    let e = h.entry();
                    if e.is_directory() {
                        at = h.skip().map_err(err)?;
                        continue;
                    }
                    let (name, size) = (rar_name(&e.filename), e.unpacked_size);
                    // unrar writes files, so each one goes through a scratch file
                    let tmp = scratch.join("entry");
                    at = h.extract_to(&tmp).map_err(err)?;
                    let mut file = File::open(&tmp)?;
                    f(&name, size, &mut file)?;
                    drop(file);
                    let _ = std::fs::remove_file(&tmp);
                }
                Ok(())
            })();
            let _ = std::fs::remove_dir_all(&scratch);
            result
        }
        _ => {
            let mut z = zip::ZipArchive::new(File::open(path)?).context("this isn't a ZIP the app can read")?;
            for i in 0..z.len() {
                stopped()?;
                let mut e = z.by_index(i).context("The ZIP contains an unsupported or encrypted entry.")?;
                if e.is_dir() { continue; }
                let (name, size) = (e.name().to_string(), e.size());
                f(&name, size, &mut e)?;
            }
            Ok(())
        }
    }
}

/// One entry, to read from: straight from a ZIP, or from a 7z or RAR's cache.
/// A 7z or RAR that isn't in the cache is opened first when it's no bigger than
/// `open_at_once` (`u64::MAX` in jobs, which can wait and be stopped); a bigger
/// one says [`NOT_OPEN`].
pub fn with_entry<T>(path: &Path, entry: &str, open_at_once: u64, cancel: &AtomicBool, f: impl FnOnce(&mut dyn Read, u64) -> Result<T>) -> Result<T> {
    match format(&path.to_string_lossy()) {
        Some(Format::SevenZ | Format::Rar) => {
            if opened(path).is_none() && set_size(path) > open_at_once { bail!(NOT_OPEN); }
            let dir = open(path, cancel, &|_, _, _| {})?;
            let at = dir.join(safe_entry(entry)?);
            let mut file = File::open(&at).map_err(|_| anyhow!("{entry} isn't in the archive"))?;
            let size = file.metadata()?.len();
            f(&mut file, size)
        }
        _ => {
            let mut z = zip::ZipArchive::new(File::open(path).with_context(|| format!("can't open {}", path.display()))?)
                .context("this isn't a ZIP the app can read")?;
            let mut e = z.by_name(entry).map_err(|_| anyhow!("{entry} isn't in the archive"))?;
            let size = e.size();
            f(&mut e, size)
        }
    }
}

fn read_limited(r: &mut dyn Read, size: u64, entry: &str, limit: u64) -> Result<Vec<u8>> {
    if size > limit {
        bail!("{entry} is too big to show ({} MB)", size >> 20);
    }
    let mut out = Vec::with_capacity(size.min(limit) as usize);
    r.take(limit + 1).read_to_end(&mut out).map_err(|e| io::Error::new(e.kind(), format!("Cannot read the archive's entry (it may be encrypted or corrupt): {e}")))?;
    if out.len() as u64 > limit { bail!("{entry} expanded beyond the safe reading limit."); }
    Ok(out)
}

/// One entry's bytes (at most `limit`). A 7z or RAR bigger than `open_at_once`
/// that hasn't been opened says [`NOT_OPEN`] instead.
pub fn read_opening(path: &Path, entry: &str, limit: u64, open_at_once: u64) -> Result<Vec<u8>> {
    with_entry(path, entry, open_at_once, &AtomicBool::new(false), |r, size| read_limited(r, size, entry, limit))
}

/// One entry's bytes (at most `limit`).
pub fn read(path: &Path, entry: &str, limit: u64) -> Result<Vec<u8>> {
    read_opening(path, entry, limit, open_at_once())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    /// A 7z with these files, made the way 7-Zip does (LZMA2, solid).
    pub(crate) fn make_7z(at: &Path, files: &[(&str, &[u8])]) {
        let mut w = sevenz_rust2::ArchiveWriter::create(at).unwrap();
        for (name, data) in files {
            w.push_archive_entry(sevenz_rust2::ArchiveEntry::new_file(name), Some(*data)).unwrap();
        }
        w.finish().unwrap();
    }

    fn rar_data(name: &str) -> PathBuf {
        std::fs::canonicalize(env!("CARGO_MANIFEST_DIR")).unwrap().join("../../tests/fixtures").join(name)
    }

    #[test]
    fn lists_and_reads_entries() {
        let d = std::env::temp_dir().join(format!("modlib-zip-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("m.zip");
        {
            let mut z = zip::ZipWriter::new(File::create(&p).unwrap());
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

    #[test]
    fn knows_archives_and_sets() {
        for n in ["a.zip", "a.7z", "A.7Z.001", "a.rar", "a.part1.rar", "a.part01.rar"] { assert!(is_archive(n), "{n}"); }
        for n in ["a.7z.002", "a.part2.rar", "a.r00", "a.stl", "a.zip.001"] { assert!(!is_archive(n), "{n}"); }
        for n in ["a.7z.002", "a.part2.rar", "a.part02.rar", "a.r00", "a.r12"] { assert!(is_part(n), "{n}"); }
        for n in ["a.7z.001", "a.part1.rar", "a.rar", "a.001", "a.stl"] { assert!(!is_part(n), "{n}"); }
        assert_eq!(file_kind("a.7z.003"), "archive");
        assert_eq!(file_kind("a.r01"), "archive");
        assert!(is_zip("A.ZIP") && !is_zip("a.7z"));
        assert_eq!(dos_millis(0x2e21_0000), civil_millis(2003, 1, 1, 0, 0, 0));
    }

    #[test]
    fn reads_a_7z_through_the_cache_and_a_split_one() {
        let d = crate::library::tests::temp_dir("archive-7z");
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("kit.7z");
        make_7z(&p, &[("Kit/arm.stl", b"solid arm"), ("Kit/notes.txt", b"hello"), ("__MACOSX/._x", b"")]);
        let l = list(&p).unwrap();
        assert_eq!(l.iter().map(|e| e["name"].as_str().unwrap()).collect::<Vec<_>>(), ["Kit/arm.stl", "Kit/notes.txt"]);
        assert_eq!(l[0]["size"], 9);
        assert!(!needs_opening(&p));
        assert_eq!(read(&p, "Kit/notes.txt", 100).unwrap(), b"hello");
        assert!(opened(&p).is_some(), "small archives are unpacked when first read");
        assert!(read(&p, "Kit/../x", 100).is_err());
        // bigger ones need opening first
        assert_eq!(read_opening(&p, "Kit/arm.stl", 100, 0).unwrap(), b"solid arm", "already in the cache");
        let q = d.join("big.7z");
        make_7z(&q, &[("a.stl", b"solid a")]);
        assert_eq!(read_opening(&q, "a.stl", 100, 0).unwrap_err().to_string(), NOT_OPEN);
        let last = Mutex::new((0, 0));
        open(&q, &AtomicBool::new(false), &|done, total, _| *last.lock().unwrap() = (done, total)).unwrap();
        assert_eq!(*last.lock().unwrap(), (7, 7));
        assert_eq!(read_opening(&q, "a.stl", 100, 0).unwrap(), b"solid a");
        // a split 7z reads as one
        let whole = std::fs::read(&p).unwrap();
        let (a, b) = whole.split_at(whole.len() / 2);
        std::fs::write(d.join("set.7z.001"), a).unwrap();
        std::fs::write(d.join("set.7z.002"), b).unwrap();
        assert_eq!(set_size(&d.join("set.7z.001")), whole.len() as u64);
        assert_eq!(read(&d.join("set.7z.001"), "Kit/arm.stl", 100).unwrap(), b"solid arm");
        // every file in order, for extracting
        let mut names = vec![];
        each(&p, &AtomicBool::new(false), &mut |n, size, r| { let mut s = vec![]; r.read_to_end(&mut s)?; assert_eq!(s.len() as u64, size); names.push(n.to_string()); Ok(()) }).unwrap();
        assert_eq!(names, ["Kit/arm.stl", "Kit/notes.txt", "__MACOSX/._x"]);
        assert!(each(&p, &AtomicBool::new(true), &mut |_, _, _| Ok(())).is_err(), "Stop stops");
    }

    #[test]
    fn locked_7z_asks_for_its_password() {
        let d = crate::library::tests::temp_dir("archive-7z-locked");
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("locked.7z");
        sevenz_rust2::compress_to_path_encrypted(
            { let s = d.join("src"); std::fs::create_dir_all(&s).unwrap(); std::fs::write(s.join("a.txt"), b"secret").unwrap(); s },
            &p, "pw".into()).unwrap();
        let e = read(&p, "a.txt", 100).unwrap_err().to_string();
        assert_eq!(e, LOCKED);
        set_password(&p, "nope");
        assert_eq!(read(&p, "a.txt", 100).unwrap_err().to_string(), WRONG_PASSWORD);
        set_password(&p, "pw");
        assert_eq!(read(&p, "a.txt", 100).unwrap(), b"secret");
    }

    #[test]
    fn reads_rar_files() {
        let d = crate::library::tests::temp_dir("archive-rar");
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("version.rar");
        std::fs::copy(rar_data("version.rar"), &p).unwrap();
        let l = list(&p).unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0]["name"], "VERSION");
        assert_eq!(read(&p, "VERSION", 100).unwrap(), b"unrar-0.4.0");
        let q = d.join("crypted.rar");
        std::fs::copy(rar_data("crypted.rar"), &q).unwrap();
        assert_eq!(list(&q).unwrap()[0]["name"], ".gitignore", "only its files are locked");
        assert_eq!(read(&q, ".gitignore", 1000).unwrap_err().to_string(), LOCKED);
        set_password(&q, "unrar");
        assert_eq!(read(&q, ".gitignore", 1000).unwrap(), b"target\nCargo.lock\n");
    }
}
