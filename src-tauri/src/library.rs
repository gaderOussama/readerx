//! The data model and the single `library.json` file: load, atomic save,
//! corrupt-file handling. Takes a plain directory so tests can use temp dirs.

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PageTheme {
    Original,
    Paper,
    Night,
}

impl PageTheme {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "original" => Some(Self::Original),
            "paper" => Some(Self::Paper),
            "night" => Some(Self::Night),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub schema_version: u32,
    pub books_folder: Option<String>,
    pub page_theme: PageTheme,
    /// Where the highlight Markdown files go. Null means no export.
    #[serde(default)]
    pub export_folder: Option<String>,
    /// Book id -> code of its last failed export; removed on the next success.
    #[serde(default)]
    pub export_errors: BTreeMap<String, String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            books_folder: None,
            page_theme: PageTheme::Paper,
            export_folder: None,
            export_errors: BTreeMap::new(),
        }
    }
}

/// PDF.js zoom: a percent above 0, or one of four names.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum Zoom {
    Scale(f64),
    Named(String),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PdfPosition {
    // i64 so a negative page reaches validation instead of failing to deserialize
    pub page: i64,
    pub top: f64,
    pub left: f64,
    pub zoom: Zoom,
    pub at_end: bool,
}

/// Tagged so v3 can add `{ kind: "epub", cfi }` without touching old data.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Position {
    Pdf(PdfPosition),
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Pdf,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Book {
    pub id: String,
    pub path: String,
    pub file_name: String,
    pub meta_title: Option<String>,
    pub format: Format,
    pub size_bytes: u64,
    pub mtime_ms: i64,
    pub page_count: Option<u32>,
    pub added_at: String,
    pub last_opened_at: Option<String>,
    pub position: Option<Position>,
    pub progress: f64,
    pub finished_at: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Library {
    pub settings: Settings,
    #[serde(default)]
    pub books: BTreeMap<String, Book>,
}

/// Everything behind the one mutex: the saved library plus session-only state.
pub struct Store {
    pub dir: PathBuf,
    pub lib: Library,
    pub notice: Option<String>,
    /// Ids whose file was found in the last scan. Never saved.
    pub present: HashSet<String>,
    pub folder_readable: bool,
    pub pdf_count: usize,
}

impl Store {
    pub fn open(dir: &Path, now: DateTime<Utc>) -> Store {
        let (lib, notice) = load(dir, now);
        Store {
            dir: dir.to_path_buf(),
            lib,
            notice,
            present: HashSet::new(),
            folder_readable: false,
            pdf_count: 0,
        }
    }

    pub fn covers_dir(&self) -> PathBuf {
        self.dir.join("covers")
    }

    /// On failure the data stays in memory and the next save tries again.
    pub fn persist(&self) {
        if let Err(e) = save(&self.dir, &self.lib) {
            eprintln!("readerx: could not save library.json: {e}");
        }
    }
}

/// A poisoned lock still holds a consistent library (saves are whole-file), so keep going.
pub fn lock(m: &std::sync::Mutex<Store>) -> std::sync::MutexGuard<'_, Store> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// `^[0-9a-f]{64}-[0-9]+$`. Checked before an id is used in any file path.
pub fn is_valid_id(id: &str) -> bool {
    match id.split_once('-') {
        Some((hex, size)) => {
            hex.len() == 64
                && hex.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
                && !size.is_empty()
                && size.bytes().all(|c| c.is_ascii_digit())
        }
        None => false,
    }
}

pub fn iso(now: DateTime<Utc>) -> String {
    now.to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn library_path(dir: &Path) -> PathBuf {
    dir.join("library.json")
}

/// `<name>.tmp` beside the target: `library.json.tmp`, `<bookId>.json.tmp`.
pub fn tmp_path_of(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    target.with_file_name(name)
}

fn tmp_path(dir: &Path) -> PathBuf {
    tmp_path_of(&library_path(dir))
}

/// Returns the library and, when a bad file was set aside, the notice text.
pub fn load(dir: &Path, now: DateTime<Utc>) -> (Library, Option<String>) {
    let _ = std::fs::remove_file(tmp_path(dir));
    let path = library_path(dir);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return (Library::default(), None),
        Err(_) => return (Library::default(), Some(set_aside(dir, now))),
    };
    match serde_json::from_slice::<Library>(&bytes) {
        Ok(lib) if lib.settings.schema_version == SCHEMA_VERSION => (lib, None),
        _ => (Library::default(), Some(set_aside(dir, now))),
    }
}

fn set_aside(dir: &Path, now: DateTime<Utc>) -> String {
    // No ':' in the stamp: Windows forbids it in file names.
    let name = format!("library.corrupt-{}.json", now.format("%Y%m%d-%H%M%S"));
    match std::fs::rename(library_path(dir), dir.join(&name)) {
        Ok(()) => format!(
            "The library file could not be read, so ReaderX started with an empty library. \
             The old file was kept as {name}."
        ),
        Err(_) => "The library file could not be read, so ReaderX started with an empty library."
            .to_string(),
    }
}

pub fn save(dir: &Path, lib: &Library) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(lib).map_err(io::Error::other)?;
    save_file(&library_path(dir), &bytes)
}

/// Atomically replace `target` with `bytes` through `<target>.tmp`.
pub fn save_file(target: &Path, bytes: &[u8]) -> io::Result<()> {
    atomic_write(
        target,
        &tmp_path_of(target),
        bytes,
        &mut |from, to| std::fs::rename(from, to),
        5,
        Duration::from_millis(50),
    )
}

/// Write `tmp`, flush it to disk, rename over `target`. The rename can fail
/// while antivirus or an indexer holds the file, so it is retried.
pub fn atomic_write(
    target: &Path,
    tmp: &Path,
    bytes: &[u8],
    rename: &mut dyn FnMut(&Path, &Path) -> io::Result<()>,
    attempts: u32,
    delay: Duration,
) -> io::Result<()> {
    let mut f = std::fs::File::create(tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    let mut last = None;
    for i in 0..attempts {
        if i > 0 {
            std::thread::sleep(delay);
        }
        match rename(tmp, target) {
            Ok(()) => return Ok(()),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| io::Error::other("no rename attempts")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t0() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 16, 9, 5, 7).unwrap()
    }

    fn sample_book() -> Book {
        Book {
            id: format!("{}-3", "a".repeat(64)),
            path: "C:\\books\\a.pdf".into(),
            file_name: "a.pdf".into(),
            meta_title: None,
            format: Format::Pdf,
            size_bytes: 3,
            mtime_ms: 1,
            page_count: Some(10),
            added_at: iso(t0()),
            last_opened_at: None,
            position: Some(Position::Pdf(PdfPosition {
                page: 2,
                top: 1.5,
                left: -3.0,
                zoom: Zoom::Named("page-fit".into()),
                at_end: false,
            })),
            progress: 0.2,
            finished_at: None,
        }
    }

    #[test]
    fn save_then_load_round_trips() {
        let d = tempfile::tempdir().unwrap();
        let mut lib = Library::default();
        let b = sample_book();
        lib.books.insert(b.id.clone(), b);
        save(d.path(), &lib).unwrap();
        let (back, notice) = load(d.path(), t0());
        assert_eq!(back, lib);
        assert!(notice.is_none());
        assert!(!tmp_path(d.path()).exists());
    }

    #[test]
    fn saved_file_is_valid_json_with_camel_case_and_tagged_position() {
        let d = tempfile::tempdir().unwrap();
        let mut lib = Library::default();
        let b = sample_book();
        lib.books.insert(b.id.clone(), b.clone());
        save(d.path(), &lib).unwrap();
        let v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(library_path(d.path())).unwrap()).unwrap();
        assert_eq!(v["settings"]["schemaVersion"], 1);
        assert_eq!(v["settings"]["pageTheme"], "paper");
        let book = &v["books"][&b.id];
        assert_eq!(book["position"]["kind"], "pdf");
        assert_eq!(book["position"]["atEnd"], false);
        assert_eq!(book["position"]["zoom"], "page-fit");
        assert!(book["lastOpenedAt"].is_null());
    }

    #[test]
    fn leftover_tmp_is_removed_on_load() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(tmp_path(d.path()), b"half writ").unwrap();
        let (lib, notice) = load(d.path(), t0());
        assert_eq!(lib, Library::default());
        assert!(notice.is_none());
        assert!(!tmp_path(d.path()).exists());
    }

    #[test]
    fn corrupt_file_is_set_aside_with_a_colon_free_name_and_a_notice() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(library_path(d.path()), b"{ not json").unwrap();
        let (lib, notice) = load(d.path(), t0());
        assert_eq!(lib, Library::default());
        let name = "library.corrupt-20261116-090507.json";
        assert!(!name.contains(':'));
        assert!(d.path().join(name).exists());
        assert!(!library_path(d.path()).exists());
        assert!(notice.unwrap().contains(name));
    }

    #[test]
    fn unknown_schema_version_is_set_aside_not_overwritten() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            library_path(d.path()),
            br#"{"settings":{"schemaVersion":2,"booksFolder":null,"pageTheme":"paper"},"books":{}}"#,
        )
        .unwrap();
        let (_, notice) = load(d.path(), t0());
        assert!(notice.is_some());
        assert!(d.path().join("library.corrupt-20261116-090507.json").exists());
    }

    #[test]
    fn rename_is_retried_then_succeeds() {
        let d = tempfile::tempdir().unwrap();
        let mut calls = 0;
        let r = atomic_write(
            &library_path(d.path()),
            &tmp_path(d.path()),
            b"{}",
            &mut |from, to| {
                calls += 1;
                if calls < 3 {
                    Err(io::Error::other("locked"))
                } else {
                    std::fs::rename(from, to)
                }
            },
            5,
            Duration::from_millis(1),
        );
        assert!(r.is_ok());
        assert_eq!(calls, 3);
        assert_eq!(std::fs::read(library_path(d.path())).unwrap(), b"{}");
    }

    #[test]
    fn rename_gives_up_after_the_attempts_and_keeps_the_old_file() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(library_path(d.path()), b"old").unwrap();
        let mut calls = 0;
        let r = atomic_write(
            &library_path(d.path()),
            &tmp_path(d.path()),
            b"new",
            &mut |_, _| {
                calls += 1;
                Err(io::Error::other("locked"))
            },
            5,
            Duration::from_millis(1),
        );
        assert!(r.is_err());
        assert_eq!(calls, 5);
        assert_eq!(std::fs::read(library_path(d.path())).unwrap(), b"old");
    }

    #[test]
    fn save_file_writes_any_target_through_its_own_tmp() {
        let d = tempfile::tempdir().unwrap();
        let target = d.path().join("abc.json");
        std::fs::write(&target, b"old").unwrap();
        save_file(&target, b"new").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert!(!d.path().join("abc.json.tmp").exists());
        assert_eq!(tmp_path_of(&target), d.path().join("abc.json.tmp"));
    }

    #[test]
    fn old_library_without_export_settings_still_loads_with_defaults() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            library_path(d.path()),
            br#"{"settings":{"schemaVersion":1,"booksFolder":"D:/b","pageTheme":"night"},"books":{}}"#,
        )
        .unwrap();
        let (lib, notice) = load(d.path(), t0());
        assert!(notice.is_none());
        assert_eq!(lib.settings.export_folder, None);
        assert!(lib.settings.export_errors.is_empty());
        assert_eq!(lib.settings.page_theme, PageTheme::Night);
    }

    #[test]
    fn export_settings_round_trip_in_camel_case() {
        let d = tempfile::tempdir().unwrap();
        let mut lib = Library::default();
        lib.settings.export_folder = Some("D:/vault".into());
        lib.settings.export_errors.insert("x".into(), "locked".into());
        save(d.path(), &lib).unwrap();
        let v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(library_path(d.path())).unwrap()).unwrap();
        assert_eq!(v["settings"]["exportErrors"]["x"], "locked");
        assert_eq!(load(d.path(), t0()).0, lib);
    }
}
