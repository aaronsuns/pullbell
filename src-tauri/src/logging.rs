//! A tiny file logger: `~/Library/Logs/Pullbell/pullbell.log`, rotated at 1 MB, 3 old files kept.
//! Callers must never log tokens or comment bodies beyond the 120-char banner text.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const MAX_BYTES: u64 = 1024 * 1024;
pub const KEEP: usize = 3;

/// Shift `log.2 → log.3`, `log.1 → log.2`, `log → log.1` when `log` is at least `max` bytes.
/// The oldest file falls off.
pub fn rotate_if_needed(path: &Path, max: u64, keep: usize) -> std::io::Result<bool> {
    let size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if size < max {
        return Ok(false);
    }
    let numbered = |i: usize| PathBuf::from(format!("{}.{i}", path.display()));
    let _ = fs::remove_file(numbered(keep));
    for i in (1..keep).rev() {
        if numbered(i).exists() {
            fs::rename(numbered(i), numbered(i + 1))?;
        }
    }
    fs::rename(path, numbered(1))?;
    Ok(true)
}

pub struct FileLogger {
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileLogger {
    pub fn new(path: PathBuf) -> Self {
        FileLogger {
            path,
            lock: Mutex::new(()),
        }
    }

    pub fn write_line(&self, line: &str) {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(dir) = self.path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = rotate_if_needed(&self.path, MAX_BYTES, KEEP);
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            let _ = writeln!(f, "{line}");
        }
    }
}

impl log::Log for FileLogger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info && m.target().starts_with("pullbell")
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            self.write_line(&format!("{ts} {:<5} {}", r.level(), r.args()));
        }
    }
    fn flush(&self) {}
}

/// Install the logger. Safe to call once; later calls are ignored.
pub fn init(path: PathBuf) {
    let logger = Box::leak(Box::new(FileLogger::new(path)));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}

pub fn default_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    home.join("Library/Logs/Pullbell/pullbell.log")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotates_and_keeps_three() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("pullbell.log");
        for round in 0..5 {
            fs::write(&p, format!("round {round} ").repeat(20)).unwrap();
            assert!(rotate_if_needed(&p, 50, KEEP).unwrap());
        }
        let mut names: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec!["pullbell.log.1", "pullbell.log.2", "pullbell.log.3"]
        );
        assert!(fs::read_to_string(dir.path().join("pullbell.log.1"))
            .unwrap()
            .starts_with("round 4"));
        assert!(fs::read_to_string(dir.path().join("pullbell.log.3"))
            .unwrap()
            .starts_with("round 2"));
    }

    #[test]
    fn small_files_are_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("pullbell.log");
        fs::write(&p, "x").unwrap();
        assert!(!rotate_if_needed(&p, 50, KEEP).unwrap());
        assert!(!rotate_if_needed(&dir.path().join("missing.log"), 50, KEEP).unwrap());
    }

    #[test]
    fn logger_appends_lines() {
        let dir = tempfile::tempdir().unwrap();
        let l = FileLogger::new(dir.path().join("sub/pullbell.log"));
        l.write_line("one");
        l.write_line("two");
        let s = fs::read_to_string(dir.path().join("sub/pullbell.log")).unwrap();
        assert_eq!(s, "one\ntwo\n");
    }
}
