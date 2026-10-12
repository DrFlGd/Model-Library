//! Progress and Stop for the checks a review makes first: reading and hashing
//! every file an import, transfer, ZIP operation, extraction or deletion would
//! touch. The page asks for a plan as a job (`job: true`); the job runs the plan
//! inside `scope`, and the hashing loops call `file` and `read`, which report to
//! the job and fail with "Stopped." once Stop is pressed. Outside a scope (the
//! plan run directly, or an operation re-checking its files) they do nothing.

use std::cell::RefCell;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// What a check reports: files started, bytes read, the file being read.
pub type Report = Arc<dyn Fn(u64, u64, &str) + Send + Sync>;

#[derive(Clone)]
pub struct Check {
    pub cancel: Arc<AtomicBool>,
    pub report: Report,
}

struct State {
    check: Check,
    files: u64,
    bytes: u64,
    name: String,
    last: Instant,
}

thread_local! {
    static NOW: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Run `f` with `check` receiving the progress of the hashing it does on this thread.
pub fn scope<T>(check: Check, f: impl FnOnce() -> T) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            NOW.with(|n| *n.borrow_mut() = None);
        }
    }
    NOW.with(|n| {
        *n.borrow_mut() = Some(State { check, files: 0, bytes: 0, name: String::new(), last: Instant::now() })
    });
    let _reset = Reset;
    f()
}

fn stopped() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Interrupted, "Stopped.")
}

/// A file is about to be read in full.
pub fn file(path: &Path) -> std::io::Result<()> {
    NOW.with(|n| {
        let mut n = n.borrow_mut();
        let Some(s) = n.as_mut() else { return Ok(()) };
        if s.check.cancel.load(Ordering::Relaxed) {
            return Err(stopped());
        }
        s.files += 1;
        s.name = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        (s.check.report)(s.files, s.bytes, &s.name);
        s.last = Instant::now();
        Ok(())
    })
}

/// `n` more bytes were read.
pub fn read(n: usize) -> std::io::Result<()> {
    NOW.with(|cell| {
        let mut cell = cell.borrow_mut();
        let Some(s) = cell.as_mut() else { return Ok(()) };
        if s.check.cancel.load(Ordering::Relaxed) {
            return Err(stopped());
        }
        s.bytes += n as u64;
        if s.last.elapsed() >= Duration::from_millis(150) {
            (s.check.report)(s.files, s.bytes, &s.name);
            s.last = Instant::now();
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn reports_inside_a_scope_and_stops_when_asked() {
        let seen = Arc::new(Mutex::new(vec![]));
        let s2 = seen.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let check = Check { cancel: cancel.clone(), report: Arc::new(move |f, b, n: &str| s2.lock().unwrap().push((f, b, n.to_string()))) };
        // outside a scope: nothing reported, never stopped
        assert!(file(Path::new("a.stl")).is_ok() && read(10).is_ok());
        scope(check, || {
            file(Path::new("dir/a.stl")).unwrap();
            read(100).unwrap();
            cancel.store(true, Ordering::Relaxed);
            assert_eq!(read(1).unwrap_err().to_string(), "Stopped.");
            assert!(file(Path::new("b.stl")).is_err());
        });
        assert_eq!(seen.lock().unwrap()[0], (1, 0, "a.stl".to_string()));
        // the scope is gone again
        assert!(read(1).is_ok());
    }
}
