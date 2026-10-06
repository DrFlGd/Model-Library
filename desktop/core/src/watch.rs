//! Noticing changes made outside the app (docs/PLAN.md, "Phase 5 design", 4): a
//! watcher on the library folder. Changes are gathered until the folder has been
//! quiet for a moment, then `on_change` gets the paths that changed, once. The
//! app's own folder (`_library/`) and hidden files are left out.

use crate::library::APP_DIR;
use notify::{Event, EventKind, RecursiveMode, Watcher as _};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

/// How long the folder must be quiet before the library is read again.
pub const SETTLE: Duration = Duration::from_secs(2);
/// More changed paths than this and the whole library is looked at instead.
const MANY: usize = 5000;

/// Watches one library folder until it's dropped.
pub struct Watcher {
    root: PathBuf,
    _inner: notify::RecommendedWatcher,
}

impl Watcher {
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Does a change at `p` matter to the library's lists?
fn matters(root: &Path, p: &Path) -> bool {
    let Ok(rel) = p.strip_prefix(root) else {
        return false;
    };
    match rel.components().next() {
        None => true,
        Some(first) => {
            let first = first.as_os_str().to_string_lossy();
            first != APP_DIR && !first.starts_with('.')
        }
    }
}

/// Watch `root` and everything in it. Once changes have settled for `settle`,
/// `on_change` gets the paths that changed (None: too many to list, or the
/// system lost track, so look at everything).
pub fn watch(
    root: &Path,
    settle: Duration,
    on_change: impl Fn(Option<Vec<PathBuf>>) + Send + 'static,
) -> notify::Result<Watcher> {
    let (tx, rx) = mpsc::channel::<Option<Vec<PathBuf>>>();
    let base = root.to_path_buf();
    let mut w = notify::recommended_watcher(move |res: notify::Result<Event>| {
        let _ = match res {
            Ok(ev) if matches!(ev.kind, EventKind::Access(_)) => Ok(()),
            Ok(ev) if ev.need_rescan() => tx.send(None),
            Ok(ev) => {
                let paths: Vec<PathBuf> =
                    ev.paths.into_iter().filter(|p| matters(&base, p)).collect();
                if paths.is_empty() {
                    Ok(())
                } else {
                    tx.send(Some(paths))
                }
            }
            Err(_) => tx.send(None),
        };
    })?;
    w.watch(root, RecursiveMode::Recursive)?;
    std::thread::Builder::new()
        .name("library-watch".into())
        .spawn(move || {
            // a change; then more until none come for `settle`; then report them
            while let Ok(first) = rx.recv() {
                let mut all: Option<HashSet<PathBuf>> = first.map(|p| p.into_iter().collect());
                loop {
                    match rx.recv_timeout(settle) {
                        Ok(more) => match (all.as_mut(), more) {
                            (Some(set), Some(p)) => {
                                set.extend(p);
                                if set.len() > MANY {
                                    all = None;
                                }
                            }
                            _ => all = None,
                        },
                        Err(mpsc::RecvTimeoutError::Timeout) => break,
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }
                on_change(all.map(|s| s.into_iter().collect()));
            }
        })?;
    Ok(Watcher {
        root: root.to_path_buf(),
        _inner: w,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::tests::temp_dir;
    use std::sync::{Arc, Mutex};

    #[test]
    fn reports_settled_changes_once_and_skips_the_apps_folder() {
        let root = temp_dir("watch");
        std::fs::create_dir_all(root.join(APP_DIR)).unwrap();
        let seen: Arc<Mutex<Vec<Option<Vec<PathBuf>>>>> = Arc::default();
        let s = seen.clone();
        let w = watch(&root, Duration::from_millis(800), move |p| {
            s.lock().unwrap().push(p)
        })
        .unwrap();
        assert_eq!(w.root(), root);
        std::fs::write(root.join(APP_DIR).join("journal.json"), "{}").unwrap();
        std::thread::sleep(Duration::from_millis(1500));
        assert!(
            seen.lock().unwrap().is_empty(),
            "the app's own folder is left out"
        );
        std::fs::create_dir_all(root.join("Unsorted/Hook")).unwrap();
        for i in 0..5 {
            std::fs::write(root.join(format!("Unsorted/Hook/h{i}.stl")), "solid h").unwrap();
            std::thread::sleep(Duration::from_millis(50));
        }
        let t = std::time::Instant::now();
        while seen.lock().unwrap().is_empty() && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(50));
        }
        std::thread::sleep(Duration::from_millis(1500));
        let got = seen.lock().unwrap().clone();
        assert_eq!(got.len(), 1, "one report for the lot: {got:?}");
        let paths = got[0].clone().unwrap();
        assert!(
            paths.iter().any(|p| p.ends_with("Unsorted/Hook/h4.stl")),
            "{paths:?}"
        );
        drop(w);
    }
}
