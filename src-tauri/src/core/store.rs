//! JSON files under the app data dir, written atomically. A corrupt file is moved aside and
//! replaced by defaults; loading never fails.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

/// Write `contents` to `path` via a temp file in the same directory + fsync + rename, so a crash
/// leaves either the old or the new file, never half of one.
pub fn write_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = dir.join(format!(".{name}.tmp-{}", std::process::id()));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(contents)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

pub fn save_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let raw = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    write_atomic(path, &raw)
}

/// Move a bad file out of the way: `state.json` → `state.json.corrupt-<unix-secs>`.
pub fn quarantine(path: &Path) -> Option<PathBuf> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let name = path.file_name()?.to_str()?;
    let dest = path.with_file_name(format!("{name}.corrupt-{secs}"));
    fs::rename(path, &dest).ok().map(|_| dest)
}

/// Outcome of loading a JSON file.
#[derive(Debug, PartialEq)]
pub enum Loaded {
    Missing,
    Ok,
    /// The file could not be parsed and was moved to this path.
    Quarantined(Option<PathBuf>),
}

/// Load `T` strictly; on parse failure move the file aside and return the default.
pub fn load_json<T: DeserializeOwned + Default>(path: &Path) -> (T, Loaded) {
    match fs::read_to_string(path) {
        Err(_) => (T::default(), Loaded::Missing),
        Ok(raw) => match serde_json::from_str(&raw) {
            Ok(v) => (v, Loaded::Ok),
            Err(_) => (T::default(), Loaded::Quarantined(quarantine(path))),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
    struct Doc {
        n: u32,
    }

    #[test]
    fn writes_atomically_and_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("nested/state.json");
        save_json(&p, &Doc { n: 7 }).unwrap();
        save_json(&p, &Doc { n: 8 }).unwrap();
        let (d, how) = load_json::<Doc>(&p);
        assert_eq!((d, how), (Doc { n: 8 }, Loaded::Ok));
        let leftovers: Vec<_> = fs::read_dir(p.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn missing_file_is_default() {
        let dir = tempfile::tempdir().unwrap();
        let (d, how) = load_json::<Doc>(&dir.path().join("none.json"));
        assert_eq!((d, how), (Doc::default(), Loaded::Missing));
    }

    #[test]
    fn corrupt_file_is_moved_aside() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("state.json");
        fs::write(&p, "{ truncated").unwrap();
        let (d, how) = load_json::<Doc>(&p);
        assert_eq!(d, Doc::default());
        let Loaded::Quarantined(Some(moved)) = how else {
            panic!("expected quarantine, got {how:?}")
        };
        assert!(!p.exists());
        assert_eq!(fs::read_to_string(moved).unwrap(), "{ truncated");
    }
}
