//! Folder scan: walk, fingerprint (with a hash cache), merge into the library.
//! Walking and hashing run without the store lock; only `merge` takes it.

use crate::library::{iso, lock, Book, Format, Library, Store};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

/// A file modified more recently than this may still be copying.
pub const MIN_AGE_MS: i64 = 2000;
const HASH_BYTES: u64 = 64 * 1024;

/// (path, sizeBytes, mtimeMs) -> id
pub type HashCache = HashMap<(String, u64, i64), String>;
pub type Hasher<'a> = dyn FnMut(&Path, u64) -> io::Result<String> + 'a;

#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub id: String,
    pub path: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub mtime_ms: i64,
    pub format: Format,
}

#[derive(Debug, Default)]
pub struct ScanOutput {
    pub found: Vec<Found>,
    /// False when the books folder is unset or cannot be read.
    pub readable: bool,
    /// PDFs and EPUBs seen, including ones skipped for being too new.
    pub book_count: usize,
    /// Some file was too new: scan again shortly.
    pub skipped_new: bool,
}

/// sha256 of the first 64 KiB, hex, then `-` and the file size.
pub fn fingerprint(path: &Path, size: u64) -> io::Result<String> {
    let mut hasher = Sha256::new();
    io::copy(&mut fs::File::open(path)?.take(HASH_BYTES), &mut hasher)?;
    let hex: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!("{hex}-{size}"))
}

pub fn cache_of(lib: &Library) -> HashCache {
    lib.books
        .values()
        .map(|b| ((b.path.clone(), b.size_bytes, b.mtime_ms), b.id.clone()))
        .collect()
}

struct Walk {
    files: Vec<(PathBuf, u64, i64)>,
    book_count: usize,
    skipped_new: bool,
}

fn walk(dir: &Path, now_ms: i64, out: &mut Walk) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        // file_type() does not follow links; links and junctions are skipped
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        let path = entry.path();
        if ft.is_dir() {
            walk(&path, now_ms, out);
        } else if ft.is_file() && format_of(&path).is_some() {
            let Ok(meta) = entry.metadata() else { continue };
            let Some(mtime_ms) = mtime_ms(&meta) else { continue };
            out.book_count += 1;
            if (0..MIN_AGE_MS).contains(&(now_ms - mtime_ms)) {
                out.skipped_new = true;
                continue;
            }
            out.files.push((path, meta.len(), mtime_ms));
        }
    }
    true
}

fn format_of(path: &Path) -> Option<Format> {
    let ext = path.extension()?;
    if ext.eq_ignore_ascii_case("pdf") {
        Some(Format::Pdf)
    } else if ext.eq_ignore_ascii_case("epub") {
        Some(Format::Epub)
    } else {
        None
    }
}

fn mtime_ms(meta: &fs::Metadata) -> Option<i64> {
    let d = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(d.as_millis()).ok()
}

/// Walk `folder`, reuse cached ids where (path, size, mtime) match, hash the rest.
/// Identical content under two paths keeps the first path in sorted order.
pub fn collect(folder: &Path, cache: &HashCache, now_ms: i64, hasher: &mut Hasher) -> ScanOutput {
    let mut w = Walk { files: Vec::new(), book_count: 0, skipped_new: false };
    let readable = walk(folder, now_ms, &mut w);
    let mut files: Vec<(String, u64, i64, PathBuf)> = w
        .files
        .into_iter()
        .map(|(p, size, m)| (p.to_string_lossy().into_owned(), size, m, p))
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut seen = HashSet::new();
    let mut found = Vec::new();
    for (path, size, mtime, p) in files {
        let id = match cache.get(&(path.clone(), size, mtime)) {
            Some(id) => id.clone(),
            None => match hasher(&p, size) {
                Ok(id) => id,
                Err(_) => continue,
            },
        };
        if !seen.insert(id.clone()) {
            continue;
        }
        let file_name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let format = format_of(&p).unwrap_or(Format::Pdf); // walk only keeps books
        found.push(Found { id, path, file_name, size_bytes: size, mtime_ms: mtime, format });
    }
    ScanOutput { found, readable, book_count: w.book_count, skipped_new: w.skipped_new }
}

/// Apply a scan to the store. Never deletes or changes a record because its
/// file is gone; only the in-memory `present` set changes.
pub fn merge(store: &mut Store, out: ScanOutput, now: DateTime<Utc>) {
    store.folder_readable = out.readable;
    store.book_count = out.book_count;
    store.present.clear();
    if !out.readable {
        return;
    }
    let mut dirty = false;
    for f in out.found {
        store.present.insert(f.id.clone());
        match store.lib.books.get_mut(&f.id) {
            Some(b) => {
                if b.path != f.path
                    || b.file_name != f.file_name
                    || b.size_bytes != f.size_bytes
                    || b.mtime_ms != f.mtime_ms
                {
                    b.path = f.path;
                    b.file_name = f.file_name;
                    b.size_bytes = f.size_bytes;
                    b.mtime_ms = f.mtime_ms;
                    dirty = true;
                }
            }
            None => {
                store.lib.books.insert(
                    f.id.clone(),
                    Book {
                        id: f.id,
                        path: f.path,
                        file_name: f.file_name,
                        meta_title: None,
                        format: f.format,
                        size_bytes: f.size_bytes,
                        mtime_ms: f.mtime_ms,
                        page_count: None,
                        added_at: iso(now),
                        last_opened_at: None,
                        position: None,
                        progress: 0.0,
                        finished_at: None,
                        meta_author: None,
                        cover_checked: false,
                    },
                );
                dirty = true;
            }
        }
    }
    if dirty {
        store.persist();
    }
}

/// Full scan: snapshot under the lock, walk and hash outside it, merge under it.
/// Returns true when a file was too new and a rescan should follow in ~1.6 s.
pub fn run(shared: &Mutex<Store>, now: DateTime<Utc>, hasher: &mut Hasher) -> bool {
    let (folder, cache) = {
        let s = lock(shared);
        (s.lib.settings.books_folder.clone(), cache_of(&s.lib))
    };
    let out = match folder {
        Some(f) => collect(Path::new(&f), &cache, now.timestamp_millis(), hasher),
        None => ScanOutput::default(),
    };
    let again = out.skipped_new;
    merge(&mut lock(shared), out, now);
    again
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::fs::File;
    use std::time::Duration;

    fn now() -> DateTime<Utc> {
        // far enough after "real" file mtimes that nothing counts as too new
        Utc::now() + chrono::Duration::seconds(60)
    }

    fn write(dir: &Path, rel: &str, bytes: &[u8]) -> PathBuf {
        let p = dir.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, bytes).unwrap();
        p
    }

    fn setup() -> (tempfile::TempDir, tempfile::TempDir, Mutex<Store>) {
        let books = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let mut store = Store::open(data.path(), now());
        store.lib.settings.books_folder = Some(books.path().to_string_lossy().into_owned());
        (books, data, Mutex::new(store))
    }

    fn rescan(m: &Mutex<Store>) {
        run(m, now(), &mut fingerprint);
    }

    fn present_paths(m: &Mutex<Store>) -> Vec<String> {
        let s = lock(m);
        let mut v: Vec<_> = s.present.iter().map(|id| s.lib.books[id].path.clone()).collect();
        v.sort();
        v
    }

    #[test]
    fn fingerprint_is_stable_across_rename_and_has_id_shape() {
        let d = tempfile::tempdir().unwrap();
        let a = write(d.path(), "a.pdf", &[7u8; 100]);
        let id1 = fingerprint(&a, 100).unwrap();
        let b = d.path().join("renamed.pdf");
        fs::rename(&a, &b).unwrap();
        assert_eq!(fingerprint(&b, 100).unwrap(), id1);
        let (hex, size) = id1.split_once('-').unwrap();
        assert_eq!(hex.len(), 64);
        assert!(hex.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)));
        assert_eq!(size, "100");
    }

    #[test]
    fn fingerprint_changes_when_the_first_64k_change_but_not_after_them() {
        let d = tempfile::tempdir().unwrap();
        let mut bytes = vec![1u8; 70_000];
        let a = write(d.path(), "a.pdf", &bytes);
        let id = fingerprint(&a, 70_000).unwrap();
        bytes[100] = 2;
        let b = write(d.path(), "b.pdf", &bytes);
        assert_ne!(fingerprint(&b, 70_000).unwrap(), id);
        bytes[100] = 1;
        bytes[69_999] = 9; // past the first 64 KiB, same size
        let c = write(d.path(), "c.pdf", &bytes);
        assert_eq!(fingerprint(&c, 70_000).unwrap(), id);
    }

    #[test]
    fn hash_cache_hit_skips_reading_and_a_changed_size_or_mtime_misses() {
        let d = tempfile::tempdir().unwrap();
        let p = write(d.path(), "a.pdf", b"hello");
        let calls = std::cell::Cell::new(0);
        let mut counting = |path: &Path, size: u64| {
            calls.set(calls.get() + 1);
            fingerprint(path, size)
        };
        let first = collect(d.path(), &HashCache::new(), i64::MAX / 2, &mut counting);
        assert_eq!(calls.get(), 1);
        let f = &first.found[0];
        let mut cache = HashCache::new();
        cache.insert((f.path.clone(), f.size_bytes, f.mtime_ms), f.id.clone());

        let again = collect(d.path(), &cache, i64::MAX / 2, &mut counting);
        assert_eq!(calls.get(), 1, "cache hit must not re-read");
        assert_eq!(again.found, first.found);

        // same path, new mtime: miss
        let file = File::options().write(true).open(&p).unwrap();
        file.set_modified(std::time::SystemTime::now() - Duration::from_secs(5000)).unwrap();
        drop(file);
        collect(d.path(), &cache, i64::MAX / 2, &mut counting);
        assert_eq!(calls.get(), 2);

        // same path, new size: miss
        write(d.path(), "a.pdf", b"hello!!");
        collect(d.path(), &cache, i64::MAX / 2, &mut counting);
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn walks_nested_folders_and_matches_pdf_in_any_case_only() {
        let (books, _data, m) = setup();
        write(books.path(), "a.pdf", b"1");
        write(books.path(), "deep/er/B.PDF", b"22");
        write(books.path(), "c.Pdf", b"333");
        write(books.path(), "notes.txt", b"4444");
        write(books.path(), "book.pdf.bak", b"55555");
        rescan(&m);
        let s = lock(&m);
        assert_eq!(s.present.len(), 3);
        assert_eq!(s.book_count, 3);
        assert!(s.folder_readable);
    }

    #[test]
    fn epub_in_any_case_is_a_book_with_format_epub_and_a_pdf_stays_pdf() {
        let (books, _data, m) = setup();
        write(books.path(), "a.pdf", b"1");
        write(books.path(), "deep/B.epub", b"22");
        write(books.path(), "C.EPUB", b"333");
        write(books.path(), "d.Epub", b"4444");
        write(books.path(), "book.epub.bak", b"55555");
        write(books.path(), "epub", b"666666");
        rescan(&m);
        let s = lock(&m);
        assert_eq!(s.present.len(), 4);
        let format_of = |name: &str| {
            s.lib.books.values().find(|b| b.file_name == name).map(|b| b.format)
        };
        assert_eq!(format_of("a.pdf"), Some(Format::Pdf));
        for name in ["B.epub", "C.EPUB", "d.Epub"] {
            assert_eq!(format_of(name), Some(Format::Epub), "{name}");
        }
    }

    #[test]
    fn a_folder_with_only_epubs_is_not_empty_because_the_count_covers_both_formats() {
        let (books, _data, m) = setup();
        write(books.path(), "only.epub", b"1");
        rescan(&m);
        assert_eq!(lock(&m).book_count, 1);
        write(books.path(), "also.pdf", b"22");
        rescan(&m);
        assert_eq!(lock(&m).book_count, 2);
    }

    #[test]
    fn new_file_is_added_with_addedat_and_a_second_scan_changes_nothing() {
        let (books, _data, m) = setup();
        write(books.path(), "a.pdf", b"1");
        rescan(&m);
        let before = lock(&m).lib.clone();
        assert_eq!(before.books.len(), 1);
        rescan(&m);
        assert_eq!(lock(&m).lib, before);
    }

    #[test]
    fn removed_file_keeps_its_record_and_returns_with_its_progress() {
        let (books, _data, m) = setup();
        let p = write(books.path(), "a.pdf", b"1");
        rescan(&m);
        let id = lock(&m).lib.books.keys().next().unwrap().clone();
        lock(&m).lib.books.get_mut(&id).unwrap().progress = 0.5;

        fs::remove_file(&p).unwrap();
        rescan(&m);
        {
            let s = lock(&m);
            assert!(s.present.is_empty());
            assert_eq!(s.lib.books[&id].progress, 0.5, "record kept");
            assert!(s.folder_readable);
        }

        write(books.path(), "a.pdf", b"1");
        rescan(&m);
        let s = lock(&m);
        assert!(s.present.contains(&id));
        assert_eq!(s.lib.books[&id].progress, 0.5);
    }

    #[test]
    fn renamed_and_moved_file_keeps_its_id_and_updates_path() {
        let (books, _data, m) = setup();
        let p = write(books.path(), "a.pdf", b"content");
        rescan(&m);
        let id = lock(&m).lib.books.keys().next().unwrap().clone();
        lock(&m).lib.books.get_mut(&id).unwrap().progress = 0.25;

        fs::create_dir_all(books.path().join("sub")).unwrap();
        fs::rename(&p, books.path().join("sub").join("b.pdf")).unwrap();
        rescan(&m);

        let s = lock(&m);
        assert_eq!(s.lib.books.len(), 1);
        let b = &s.lib.books[&id];
        assert!(b.path.ends_with("b.pdf") && b.path.contains("sub"));
        assert_eq!(b.file_name, "b.pdf");
        assert_eq!(b.progress, 0.25);
        assert!(s.present.contains(&id));
    }

    #[test]
    fn duplicate_content_keeps_the_first_path_in_sorted_order() {
        let (books, _data, m) = setup();
        write(books.path(), "z/copy.pdf", b"same");
        write(books.path(), "a/orig.pdf", b"same");
        rescan(&m);
        let s = lock(&m);
        assert_eq!(s.lib.books.len(), 1);
        assert_eq!(s.present.len(), 1);
        let b = s.lib.books.values().next().unwrap();
        assert!(b.path.contains("orig.pdf"), "got {}", b.path);
    }

    #[test]
    fn file_younger_than_two_seconds_is_skipped_and_asks_for_a_rescan() {
        let (books, _data, m) = setup();
        write(books.path(), "fresh.pdf", b"x"); // mtime is "now"
        let again = run(&m, Utc::now(), &mut fingerprint);
        assert!(again);
        assert!(lock(&m).present.is_empty());
        assert!(lock(&m).lib.books.is_empty());
        assert_eq!(lock(&m).book_count, 1);

        let again = run(&m, Utc::now() + chrono::Duration::seconds(3), &mut fingerprint);
        assert!(!again);
        assert_eq!(lock(&m).present.len(), 1);
    }

    #[test]
    fn missing_folder_is_unreadable_and_changes_no_record() {
        let (books, _data, m) = setup();
        write(books.path(), "a.pdf", b"1");
        rescan(&m);
        let before = lock(&m).lib.clone();
        lock(&m).lib.settings.books_folder =
            Some(books.path().join("gone").to_string_lossy().into_owned());
        rescan(&m);
        let s = lock(&m);
        assert!(!s.folder_readable);
        assert!(s.present.is_empty());
        assert_eq!(s.lib.books, before.books);
    }

    #[test]
    fn unset_folder_scans_nothing() {
        let data = tempfile::tempdir().unwrap();
        let m = Mutex::new(Store::open(data.path(), Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap()));
        rescan(&m);
        assert!(!lock(&m).folder_readable);
        assert!(present_paths(&m).is_empty());
    }
}
