//! A small rotating log file for bug reports. Written to the app's log folder
//! (see `init`), `yonder.log` plus up to two older files, about 1 MB in total.
//! Info level by default; debug level with `--verbose` or the setting.
//! Nothing personal is logged: only app/process names and Yonder's own events.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Rotate once the current file passes this size (3 files x 350 KB, about 1 MB).
const MAX_BYTES: u64 = 350_000;
/// Files kept besides the current one.
const KEEP_OLD: usize = 2;
const NAME: &str = "yonder";

static VERBOSE: AtomicBool = AtomicBool::new(false);
static SINK: Mutex<Option<Sink>> = Mutex::new(None);

struct Sink {
    dir: PathBuf,
    file: File,
    size: u64,
}

pub fn set_verbose(on: bool) {
    VERBOSE.store(on, Ordering::Relaxed);
}

pub fn verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

fn path(dir: &std::path::Path, index: usize) -> PathBuf {
    if index == 0 {
        dir.join(format!("{NAME}.log"))
    } else {
        dir.join(format!("{NAME}.{index}.log"))
    }
}

fn open(dir: &std::path::Path) -> Option<(File, u64)> {
    let file = OpenOptions::new().create(true).append(true).open(path(dir, 0)).ok()?;
    let size = file.metadata().map(|m| m.len()).unwrap_or(0);
    Some((file, size))
}

/// Starts logging into `dir` (created if needed). Safe to call once at startup.
pub fn init(dir: PathBuf) {
    let _ = fs::create_dir_all(&dir);
    if let Some((file, size)) = open(&dir) {
        *SINK.lock().unwrap_or_else(|e| e.into_inner()) = Some(Sink { dir, file, size });
    }
}

fn rotate(sink: &mut Sink) {
    let _ = fs::remove_file(path(&sink.dir, KEEP_OLD));
    for i in (0..KEEP_OLD).rev() {
        let _ = fs::rename(path(&sink.dir, i), path(&sink.dir, i + 1));
    }
    if let Some((file, size)) = open(&sink.dir) {
        sink.file = file;
        sink.size = size;
    }
}

fn write(level: &str, msg: &str) {
    let Ok(mut guard) = SINK.lock() else { return };
    let Some(sink) = guard.as_mut() else { return };
    // One line per event: keep multi-line messages (panics, errors) on one line.
    let line = format!(
        "{} {level:<5} {}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
        msg.replace(['\r', '\n'], " | ")
    );
    if sink.size + line.len() as u64 > MAX_BYTES {
        rotate(sink);
    }
    if sink.file.write_all(line.as_bytes()).is_ok() {
        sink.size += line.len() as u64;
    }
}

pub fn info(msg: &str) {
    write("INFO", msg);
}

pub fn warn(msg: &str) {
    write("WARN", msg);
}

pub fn debug(msg: &str) {
    if verbose() {
        write("DEBUG", msg);
    }
}

/// Logs `msg` at debug level the first time `key` is seen (for checks that run
/// every few seconds and would otherwise repeat the same failure).
pub fn debug_once(key: &'static str, msg: &str) {
    use std::collections::HashSet;
    static SEEN: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);
    if !verbose() {
        return;
    }
    let first = SEEN.lock().map(|mut g| g.get_or_insert_with(HashSet::new).insert(key)).unwrap_or(false);
    if first {
        write("DEBUG", msg);
    }
}

/// Log panics too (a release build aborts right after the hook returns).
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        write("ERROR", &format!("panic: {info}"));
        previous(info);
    }));
}

macro_rules! log_info {
    ($($arg:tt)*) => { $crate::logger::info(&format!($($arg)*)) };
}
macro_rules! log_warn {
    ($($arg:tt)*) => { $crate::logger::warn(&format!($($arg)*)) };
}
macro_rules! log_debug {
    ($($arg:tt)*) => { if $crate::logger::verbose() { $crate::logger::debug(&format!($($arg)*)) } };
}
pub(crate) use {log_debug, log_info, log_warn};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotates_and_keeps_few_files() {
        let dir = std::env::temp_dir().join(format!("yonder-log-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        init(dir.clone());
        let line = "x".repeat(1000);
        for _ in 0..2000 {
            info(&line);
        }
        let count = fs::read_dir(&dir).unwrap().count();
        assert_eq!(count, KEEP_OLD + 1);
        let total: u64 = fs::read_dir(&dir).unwrap().map(|e| e.unwrap().metadata().unwrap().len()).sum();
        assert!(total <= MAX_BYTES * (KEEP_OLD as u64 + 1) + 2000, "total {total}");
        *SINK.lock().unwrap() = None;
        let _ = fs::remove_dir_all(&dir);
    }
}
