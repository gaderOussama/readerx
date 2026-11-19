//! Values worked out on every read and never stored: present, section,
//! display title, the Continue book, ordering, folder status.

use crate::library::{is_valid_id, Book, Library, Settings, Store};
use serde::Serialize;
use std::collections::HashSet;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Section {
    Reading,
    ToRead,
    Finished,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FolderStatus {
    Ok,
    Missing,
    Unset,
    Empty,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BookView {
    #[serde(flatten)]
    pub book: Book,
    pub present: bool,
    pub section: Option<Section>,
    pub display_title: String,
    pub cover_path: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StateView {
    pub settings: Settings,
    /// Present books only: Continue book first, then Reading, To read, Finished.
    pub books: Vec<BookView>,
    pub continue_id: Option<String>,
    pub folder_status: FolderStatus,
    pub notice: Option<String>,
}

pub fn display_title(meta_title: Option<&str>, file_name: &str) -> String {
    if let Some(t) = meta_title.map(str::trim) {
        if !is_junk_title(t) {
            return t.to_string();
        }
    }
    for ext in [".pdf", ".epub"] {
        if let Some(i) = file_name.len().checked_sub(ext.len()) {
            if i > 0 && file_name.is_char_boundary(i) && file_name[i..].eq_ignore_ascii_case(ext) {
                return file_name[..i].to_string();
            }
        }
    }
    file_name.to_string()
}

fn is_junk_title(t: &str) -> bool {
    t.is_empty()
        || t.eq_ignore_ascii_case("untitled")
        || t.starts_with("Microsoft Word - ")
        || ends_in_file_extension(t)
}

/// `.docx`, `.pdf`, `.indd`: a dot, then 2 to 5 letters or digits with at least
/// one letter. "Python 3.12" and "Dr. Who" are not extensions.
fn ends_in_file_extension(t: &str) -> bool {
    match t.rsplit_once('.') {
        Some((stem, ext)) => {
            !stem.is_empty()
                && (2..=5).contains(&ext.len())
                && ext.bytes().all(|c| c.is_ascii_alphanumeric())
                && ext.bytes().any(|c| c.is_ascii_alphabetic())
        }
        None => false,
    }
}

pub fn section(book: &Book, present: bool) -> Option<Section> {
    if !present {
        None
    } else if book.finished_at.is_some() {
        Some(Section::Finished)
    } else if book.position.is_some() {
        Some(Section::Reading)
    } else {
        Some(Section::ToRead)
    }
}

/// Most recently opened present book that is started and not finished.
pub fn continue_id(lib: &Library, present: &HashSet<String>) -> Option<String> {
    lib.books
        .values()
        .filter(|b| present.contains(&b.id) && b.finished_at.is_none() && b.position.is_some())
        .max_by(|a, b| {
            a.last_opened_at
                .cmp(&b.last_opened_at)
                .then_with(|| b.id.cmp(&a.id))
        })
        .map(|b| b.id.clone())
}

/// Present books in shelf order: Continue book, Reading, To read, Finished.
pub fn ordered<'a>(lib: &'a Library, present: &HashSet<String>) -> Vec<&'a Book> {
    let cont = continue_id(lib, present);
    let in_section = |s: Section| {
        lib.books
            .values()
            .filter(move |b| section(b, present.contains(&b.id)) == Some(s))
    };

    let mut reading: Vec<&Book> = in_section(Section::Reading)
        .filter(|b| Some(&b.id) != cont.as_ref())
        .collect();
    reading.sort_by(|a, b| b.last_opened_at.cmp(&a.last_opened_at).then_with(|| a.id.cmp(&b.id)));

    let mut to_read: Vec<&Book> = in_section(Section::ToRead).collect();
    to_read.sort_by_cached_key(|b| (title_of(b).to_lowercase(), b.id.clone()));

    let mut finished: Vec<&Book> = in_section(Section::Finished).collect();
    finished.sort_by(|a, b| b.finished_at.cmp(&a.finished_at).then_with(|| a.id.cmp(&b.id)));

    cont.as_ref()
        .and_then(|id| lib.books.get(id))
        .into_iter()
        .chain(reading)
        .chain(to_read)
        .chain(finished)
        .collect()
}

fn title_of(b: &Book) -> String {
    display_title(b.meta_title.as_deref(), &b.file_name)
}

pub fn folder_status(folder: Option<&str>, readable: bool, book_count: usize) -> FolderStatus {
    match folder {
        None => FolderStatus::Unset,
        Some(_) if !readable => FolderStatus::Missing,
        Some(_) if book_count == 0 => FolderStatus::Empty,
        Some(_) => FolderStatus::Ok,
    }
}

pub fn book_view(store: &Store, book: &Book) -> BookView {
    let present = store.present.contains(&book.id);
    // id is checked first: it becomes part of a file path
    let cover = store.covers_dir().join(format!("{}.png", book.id));
    let cover_path = (is_valid_id(&book.id) && cover.is_file())
        .then(|| cover.to_string_lossy().into_owned());
    BookView {
        book: book.clone(),
        present,
        section: section(book, present),
        display_title: title_of(book),
        cover_path,
    }
}

pub fn state_view(store: &Store) -> StateView {
    let folder = store.lib.settings.books_folder.as_deref();
    StateView {
        settings: store.lib.settings.clone(),
        books: ordered(&store.lib, &store.present)
            .into_iter()
            .map(|b| book_view(store, b))
            .collect(),
        continue_id: continue_id(&store.lib, &store.present),
        folder_status: folder_status(folder, store.folder_readable, store.book_count),
        notice: store.notice.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{Format, PdfPosition, Position, Zoom};

    fn book(id: &str, file: &str) -> Book {
        Book {
            id: id.into(),
            path: format!("C:\\b\\{file}"),
            file_name: file.into(),
            meta_title: None,
            format: Format::Pdf,
            size_bytes: 1,
            mtime_ms: 1,
            page_count: None,
            added_at: "2026-01-01T00:00:00.000Z".into(),
            last_opened_at: None,
            position: None,
            progress: 0.0,
            finished_at: None,
            meta_author: None,
            cover_checked: false,
        }
    }

    fn started(mut b: Book, opened: &str) -> Book {
        b.position = Some(Position::Pdf(PdfPosition {
            page: 1,
            top: 0.0,
            left: 0.0,
            zoom: Zoom::Named("auto".into()),
            at_end: false,
        }));
        b.last_opened_at = Some(opened.into());
        b
    }

    fn finished(mut b: Book, at: &str) -> Book {
        b.finished_at = Some(at.into());
        b
    }

    fn lib_of(books: Vec<Book>) -> (Library, HashSet<String>) {
        let present = books.iter().map(|b| b.id.clone()).collect();
        let mut lib = Library::default();
        for b in books {
            lib.books.insert(b.id.clone(), b);
        }
        (lib, present)
    }

    fn ids(v: &[&Book]) -> Vec<String> {
        v.iter().map(|b| b.id.clone()).collect()
    }

    #[test]
    fn title_uses_a_good_meta_title_trimmed() {
        assert_eq!(display_title(Some("  Dune  "), "x.pdf"), "Dune");
        assert_eq!(display_title(Some("Python 3.12"), "x.pdf"), "Python 3.12");
        assert_eq!(display_title(Some("Dr. Who"), "x.pdf"), "Dr. Who");
    }

    #[test]
    fn junk_meta_titles_fall_back_to_the_file_name_without_pdf() {
        for junk in [
            None,
            Some(""),
            Some("   "),
            Some("untitled"),
            Some("Untitled"),
            Some("UNTITLED"),
            Some("Microsoft Word - Chapter 1"),
            Some("report.docx"),
            Some("report.DOC"),
            Some("scan.pdf"),
            Some("layout.indd"),
        ] {
            assert_eq!(display_title(junk, "My Book.PDF"), "My Book", "meta {junk:?}");
        }
    }

    #[test]
    fn file_name_without_a_pdf_suffix_is_kept_whole() {
        assert_eq!(display_title(None, "weird"), "weird");
        assert_eq!(display_title(None, ".pdf"), ".pdf");
        assert_eq!(display_title(None, "été.pdf"), "été");
    }

    #[test]
    fn epub_file_name_loses_its_suffix_in_any_case() {
        assert_eq!(display_title(None, "My Book.epub"), "My Book");
        assert_eq!(display_title(None, "My Book.EPUB"), "My Book");
        assert_eq!(display_title(Some("scan.pdf"), "été.Epub"), "été");
        assert_eq!(display_title(None, ".epub"), ".epub");
        assert_eq!(display_title(None, "pub"), "pub");
    }

    #[test]
    fn section_is_null_when_not_present() {
        let b = started(book("a", "a.pdf"), "t");
        assert_eq!(section(&b, false), None);
        assert_eq!(section(&b, true), Some(Section::Reading));
        assert_eq!(section(&book("a", "a.pdf"), true), Some(Section::ToRead));
        assert_eq!(section(&finished(book("a", "a.pdf"), "t"), true), Some(Section::Finished));
        // finished wins over a kept position
        assert_eq!(section(&finished(b, "t"), true), Some(Section::Finished));
    }

    #[test]
    fn continue_is_the_latest_present_started_unfinished_book() {
        let (lib, present) = lib_of(vec![
            started(book("a", "a.pdf"), "2026-11-01T00:00:00.000Z"),
            started(book("b", "b.pdf"), "2026-11-03T00:00:00.000Z"),
            finished(started(book("c", "c.pdf"), "2026-11-09T00:00:00.000Z"), "2026-11-09T01:00:00.000Z"),
            book("d", "d.pdf"),
        ]);
        assert_eq!(continue_id(&lib, &present), Some("b".into()));

        let mut without_b = present.clone();
        without_b.remove("b");
        assert_eq!(continue_id(&lib, &without_b), Some("a".into()), "absent book is skipped");

        let none: HashSet<String> = ["d".to_string()].into();
        assert_eq!(continue_id(&lib, &none), None);
    }

    #[test]
    fn order_is_continue_then_reading_then_to_read_then_finished() {
        let (lib, mut present) = lib_of(vec![
            started(book("r1", "r1.pdf"), "2026-11-01T00:00:00.000Z"),
            started(book("r2", "r2.pdf"), "2026-11-05T00:00:00.000Z"),
            started(book("r3", "r3.pdf"), "2026-11-03T00:00:00.000Z"),
            book("t2", "banana.pdf"),
            book("t1", "Apple.pdf"),
            book("t3", "cherry.pdf"),
            finished(book("f1", "f1.pdf"), "2026-10-01T00:00:00.000Z"),
            finished(book("f2", "f2.pdf"), "2026-10-09T00:00:00.000Z"),
            started(book("gone", "gone.pdf"), "2026-12-01T00:00:00.000Z"),
        ]);
        present.remove("gone");
        let got = ids(&ordered(&lib, &present));
        assert_eq!(
            got,
            ["r2", "r3", "r1", "t1", "t2", "t3", "f2", "f1"],
            "r2 is Continue, so it is first and not repeated in Reading; not-present book is out"
        );
    }

    #[test]
    fn to_read_ties_break_on_id_and_ignore_case() {
        let (lib, present) = lib_of(vec![
            book("b2", "same.pdf"),
            book("b1", "SAME.pdf"),
            book("a", "Zed.pdf"),
        ]);
        assert_eq!(ids(&ordered(&lib, &present)), ["b1", "b2", "a"]);
    }

    #[test]
    fn to_read_sorts_by_display_title_not_file_name() {
        let mut z = book("z", "aaa.pdf");
        z.meta_title = Some("Zebra".into());
        let (lib, present) = lib_of(vec![z, book("m", "mmm.pdf")]);
        assert_eq!(ids(&ordered(&lib, &present)), ["m", "z"]);
    }

    #[test]
    fn folder_status_cases() {
        assert_eq!(folder_status(None, false, 0), FolderStatus::Unset);
        assert_eq!(folder_status(Some("x"), false, 0), FolderStatus::Missing);
        assert_eq!(folder_status(Some("x"), true, 0), FolderStatus::Empty);
        assert_eq!(folder_status(Some("x"), true, 3), FolderStatus::Ok);
    }

    #[test]
    fn view_serializes_with_the_contract_names() {
        let d = tempfile::tempdir().unwrap();
        let mut store = Store::open(d.path(), chrono::Utc::now());
        let id = format!("{}-1", "a".repeat(64));
        let b = started(book(&id, "a.pdf"), "2026-11-01T00:00:00.000Z");
        store.lib.books.insert(id.clone(), b);
        store.present.insert(id.clone());
        store.lib.settings.books_folder = Some("C:\\b".into());
        store.folder_readable = true;
        store.book_count = 1;
        let v = serde_json::to_value(state_view(&store)).unwrap();
        assert_eq!(v["folderStatus"], "ok");
        assert_eq!(v["continueId"], id.as_str());
        assert!(v["notice"].is_null());
        let b = &v["books"][0];
        assert_eq!(b["section"], "reading");
        assert_eq!(b["displayTitle"], "a");
        assert_eq!(b["fileName"], "a.pdf");
        assert_eq!(b["present"], true);
        assert!(b["coverPath"].is_null());
        assert_eq!(b["format"], "pdf");
        assert_eq!(b["position"]["kind"], "pdf");
        assert!(b["metaAuthor"].is_null());
        assert_eq!(b["coverChecked"], false);
        assert_eq!(v["settings"]["epubFontSize"], 100);
        assert_eq!(v["settings"]["epubLayout"], "paginated");

        std::fs::create_dir_all(store.covers_dir()).unwrap();
        std::fs::write(store.covers_dir().join(format!("{id}.png")), b"x").unwrap();
        let v = serde_json::to_value(state_view(&store)).unwrap();
        assert!(v["books"][0]["coverPath"].as_str().unwrap().ends_with(".png"));
    }
}
