//! Command logic as plain functions over the store, so it is testable without
//! a running Tauri app. Error strings are the bare codes the screen expects.

use crate::derive::{book_view, BookView};
use crate::library::{
    iso, is_valid_id, EpubLayout, EpubPosition, Format, PageTheme, PdfPosition, Position, Store, Zoom,
    EPUB_FONT_SIZES,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::path::{Path, PathBuf};

const MAX_COVER_BYTES: usize = 2 * 1024 * 1024;
const PNG_SIGNATURE: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
const ZOOM_NAMES: [&str; 4] = ["auto", "page-actual", "page-fit", "page-width"];
const MAX_CFI_CHARS: usize = 2000;
const MAX_CHAPTER_CHARS: usize = 200;
/// An EPUB counts as finished from this fraction on (or on its last page).
const EPUB_FINISHED_FRACTION: f64 = 0.98;

#[derive(Serialize, Debug, PartialEq)]
pub struct Opened {
    pub path: String,
    pub position: Option<Position>,
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    pub progress: f64,
    pub finished_at: Option<String>,
}

fn err<T>(code: &str) -> Result<T, String> {
    Err(code.to_string())
}

/// Sets the folder only; the caller widens the asset scope and rescans.
pub fn set_folder(store: &mut Store, path: &str) -> Result<(), String> {
    if !Path::new(path).is_dir() {
        return err("not_a_folder");
    }
    store.lib.settings.books_folder = Some(path.to_string());
    store.persist();
    Ok(())
}

pub fn dismiss_notice(store: &mut Store) {
    store.notice = None;
}

pub fn open_book(store: &mut Store, id: &str, now: DateTime<Utc>) -> Result<Opened, String> {
    let present = store.present.contains(id);
    let Some(book) = store.lib.books.get_mut(id) else {
        return err("unknown_book");
    };
    if !present {
        return err("not_present");
    }
    book.last_opened_at = Some(iso(now));
    let opened = Opened { path: book.path.clone(), position: book.position.clone() };
    store.persist();
    Ok(opened)
}

fn validate_position(p: &PdfPosition, page_count: i64) -> Result<(), String> {
    let zoom_ok = match &p.zoom {
        Zoom::Scale(z) => z.is_finite() && *z > 0.0,
        Zoom::Named(n) => ZOOM_NAMES.contains(&n.as_str()),
    };
    let page_ok = page_count <= i64::from(u32::MAX) && (1..=page_count).contains(&p.page);
    if page_ok && zoom_ok && p.top.is_finite() && p.left.is_finite() {
        Ok(())
    } else {
        err("bad_position")
    }
}

/// Refuses a bad cfi or fraction. The chapter label comes from the book, so a
/// long one is cut to 200 characters (on a character boundary) instead of
/// refusing the save.
fn validate_epub_position(p: &mut EpubPosition) -> Result<(), String> {
    if !p.cfi.starts_with("epubcfi(")
        || p.cfi.chars().count() > MAX_CFI_CHARS
        || !p.fraction.is_finite()
        || !(0.0..=1.0).contains(&p.fraction)
    {
        return err("bad_position");
    }
    p.chapter = p
        .chapter
        .take()
        .map(|c| c.trim().chars().take(MAX_CHAPTER_CHARS).collect::<String>())
        .filter(|c| !c.is_empty());
    Ok(())
}

/// `page_count` is required for a PDF position and ignored for an EPUB one;
/// `at_end` is only read for an EPUB (a PDF position carries its own flag).
pub fn save_position(
    store: &mut Store,
    id: &str,
    mut position: Position,
    page_count: Option<i64>,
    at_end: bool,
    now: DateTime<Utc>,
) -> Result<Saved, String> {
    let Some(book) = store.lib.books.get_mut(id) else {
        return err("unknown_book");
    };
    // A null previous position counts as not at end.
    let (progress, is_end, was_end) = match (&mut position, book.format) {
        (Position::Pdf(p), Format::Pdf) => {
            let Some(page_count) = page_count else {
                return err("bad_position");
            };
            validate_position(p, page_count)?;
            let end = |q: &PdfPosition| q.at_end || q.page == page_count;
            let was = matches!(&book.position, Some(Position::Pdf(q)) if end(q));
            book.page_count = Some(page_count as u32);
            ((p.page as f64 / page_count as f64).clamp(0.0, 1.0), end(p), was)
        }
        (Position::Epub(p), Format::Epub) => {
            validate_epub_position(p)?;
            let was = matches!(
                &book.position,
                Some(Position::Epub(q)) if q.fraction >= EPUB_FINISHED_FRACTION
            );
            (p.fraction, at_end || p.fraction >= EPUB_FINISHED_FRACTION, was)
        }
        _ => return err("bad_position"),
    };
    if !was_end && is_end && book.finished_at.is_none() {
        book.finished_at = Some(iso(now));
    }
    book.progress = progress;
    book.position = Some(position);
    let saved = Saved { progress: book.progress, finished_at: book.finished_at.clone() };
    store.persist();
    Ok(saved)
}

/// `page_count` is null for an EPUB. Always marks the cover as checked.
pub fn set_meta(
    store: &mut Store,
    id: &str,
    meta_title: Option<String>,
    meta_author: Option<String>,
    page_count: Option<i64>,
) -> Result<(), String> {
    let Some(book) = store.lib.books.get_mut(id) else {
        return err("unknown_book");
    };
    let clean = |s: String| Some(s.trim().to_string()).filter(|t| !t.is_empty());
    book.meta_title = meta_title.and_then(clean);
    book.meta_author = meta_author.and_then(clean);
    if let Some(n) = page_count.and_then(|n| u32::try_from(n).ok()).filter(|&n| n >= 1) {
        book.page_count = Some(n);
    }
    book.cover_checked = true;
    store.persist();
    Ok(())
}

pub fn save_cover(store: &Store, id: &str, png: &[u8]) -> Result<(), String> {
    // An id that is not the exact hash shape never reaches a file path.
    if !is_valid_id(id) || !store.lib.books.contains_key(id) {
        return err("unknown_book");
    }
    if png.len() > MAX_COVER_BYTES || !png.starts_with(PNG_SIGNATURE) {
        return err("bad_image");
    }
    let dir = store.covers_dir();
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(&dir)?;
        let tmp = dir.join(format!("{id}.png.tmp"));
        std::fs::write(&tmp, png)?;
        std::fs::rename(&tmp, dir.join(format!("{id}.png")))
    };
    write().map_err(|_| "cover_write_failed".to_string())
}

pub fn set_finished(
    store: &mut Store,
    id: &str,
    finished: bool,
    now: DateTime<Utc>,
) -> Result<BookView, String> {
    let Some(book) = store.lib.books.get_mut(id) else {
        return err("unknown_book");
    };
    if !finished {
        book.finished_at = None;
    } else if book.finished_at.is_none() {
        book.finished_at = Some(iso(now));
    }
    let book = book.clone();
    store.persist();
    Ok(book_view(store, &book))
}

pub fn set_page_theme(store: &mut Store, theme: &str) -> Result<(), String> {
    let Some(theme) = PageTheme::parse(theme) else {
        return err("bad_theme");
    };
    store.lib.settings.page_theme = theme;
    store.persist();
    Ok(())
}

pub fn set_epub_settings(store: &mut Store, font_size: i64, layout: &str) -> Result<(), String> {
    let size = u32::try_from(font_size).ok().filter(|n| EPUB_FONT_SIZES.contains(n));
    let (Some(size), Some(layout)) = (size, EpubLayout::parse(layout)) else {
        return err("bad_setting");
    };
    store.lib.settings.epub_font_size = size;
    store.lib.settings.epub_layout = layout;
    store.persist();
    Ok(())
}

/// The data folder and page count for a highlight command. `unknown_book` for
/// an unknown id (or one that is not the exact hash shape, so it never reaches
/// a path), `not_supported` for an EPUB.
pub fn highlight_book(store: &Store, id: &str) -> Result<(PathBuf, Option<u32>), String> {
    match store.lib.books.get(id) {
        Some(b) if is_valid_id(id) => match b.format {
            Format::Pdf => Ok((store.dir.clone(), b.page_count)),
            Format::Epub => err("not_supported"),
        },
        _ => err("unknown_book"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::derive::Section;
    use crate::library::{Book, EpubPosition, Format};
    use chrono::TimeZone;

    fn t(sec: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 16, 10, 0, sec).unwrap()
    }

    fn id() -> String {
        format!("{}-10", "b".repeat(64))
    }

    fn pos(page: i64, at_end: bool) -> Position {
        Position::Pdf(PdfPosition {
            page,
            top: 12.5,
            left: -4.0,
            zoom: Zoom::Scale(150.0),
            at_end,
        })
    }

    fn with_pos(page: i64, top: f64, left: f64, zoom: Zoom) -> Position {
        Position::Pdf(PdfPosition { page, top, left, zoom, at_end: false })
    }

    fn fixture() -> (tempfile::TempDir, Store) {
        let d = tempfile::tempdir().unwrap();
        let mut s = Store::open(d.path(), t(0));
        s.lib.books.insert(
            id(),
            Book {
                id: id(),
                path: "C:\\b\\a.pdf".into(),
                file_name: "a.pdf".into(),
                meta_title: None,
                format: Format::Pdf,
                size_bytes: 10,
                mtime_ms: 1,
                page_count: None,
                added_at: iso(t(0)),
                last_opened_at: None,
                position: None,
                progress: 0.0,
                finished_at: None,
                meta_author: None,
                cover_checked: false,
            },
        );
        s.present.insert(id());
        let epub = Book {
            id: epub_id(),
            path: "C:\\b\\a.epub".into(),
            file_name: "a.epub".into(),
            format: Format::Epub,
            ..s.lib.books[&id()].clone()
        };
        s.lib.books.insert(epub_id(), epub);
        s.present.insert(epub_id());
        (d, s)
    }

    fn epub_id() -> String {
        format!("{}-20", "e".repeat(64))
    }

    fn epos(cfi: &str, fraction: f64, chapter: Option<&str>) -> Position {
        Position::Epub(EpubPosition {
            cfi: cfi.into(),
            fraction,
            chapter: chapter.map(String::from),
        })
    }

    fn at(fraction: f64) -> Position {
        epos("epubcfi(/6/4!/4/2)", fraction, None)
    }

    fn save_epub(s: &mut Store, p: Position, at_end: bool) -> Result<Saved, String> {
        save_position(s, &epub_id(), p, None, at_end, t(30))
    }

    fn save(s: &mut Store, p: Position, count: i64) -> Result<Saved, String> {
        save_position(s, &id(), p, Some(count), false, t(30))
    }

    fn finished_at(s: &Store) -> Option<String> {
        s.lib.books[&id()].finished_at.clone()
    }

    #[test]
    fn id_shape_check() {
        assert!(is_valid_id(&id()));
        for bad in [
            "",
            "../../evil",
            &format!("{}-", "b".repeat(64)),
            &format!("{}-1x", "b".repeat(64)),
            &format!("{}-10", "B".repeat(64)),
            &format!("{}-10", "b".repeat(63)),
            &format!("{}-10\n", "b".repeat(64)),
            &format!("{}-10/..", "b".repeat(64)),
        ] {
            assert!(!is_valid_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn save_position_stores_position_page_count_and_progress() {
        let (_d, mut s) = fixture();
        let r = save(&mut s, pos(5, false), 10).unwrap();
        assert_eq!(r, Saved { progress: 0.5, finished_at: None });
        let b = &s.lib.books[&id()];
        assert_eq!(b.page_count, Some(10));
        assert_eq!(b.position, Some(pos(5, false)));
        assert_eq!(b.progress, 0.5);
    }

    #[test]
    fn page_379_to_380_of_380_finishes() {
        let (_d, mut s) = fixture();
        save(&mut s, pos(379, false), 380).unwrap();
        assert_eq!(finished_at(&s), None);
        let r = save(&mut s, pos(380, false), 380).unwrap();
        assert_eq!(finished_at(&s), Some(iso(t(30))));
        assert_eq!(r.finished_at, finished_at(&s));
        assert_eq!(r.progress, 1.0);
        assert_eq!(section_of(&s), Some(Section::Finished));
    }

    fn section_of(s: &Store) -> Option<Section> {
        crate::derive::section(&s.lib.books[&id()], true)
    }

    #[test]
    fn null_previous_with_page_equal_page_count_finishes_a_one_page_book() {
        let (_d, mut s) = fixture();
        save(&mut s, pos(1, false), 1).unwrap();
        assert!(finished_at(&s).is_some());
    }

    #[test]
    fn null_previous_not_at_end_does_not_finish() {
        let (_d, mut s) = fixture();
        save(&mut s, pos(3, false), 10).unwrap();
        assert!(finished_at(&s).is_none());
    }

    #[test]
    fn previous_at_end_then_reopen_at_end_does_not_finish_again() {
        let (_d, mut s) = fixture();
        save(&mut s, pos(380, false), 380).unwrap();
        // user marks it unfinished, then reopens at the same end position
        set_finished(&mut s, &id(), false, t(40)).unwrap();
        save(&mut s, pos(380, false), 380).unwrap();
        assert!(finished_at(&s).is_none());
        save(&mut s, pos(380, true), 380).unwrap();
        assert!(finished_at(&s).is_none());
        assert_eq!(section_of(&s), Some(Section::Reading));
    }

    #[test]
    fn at_end_flag_from_a_place_before_the_end_finishes() {
        let (_d, mut s) = fixture();
        save(&mut s, pos(379, false), 380).unwrap();
        save(&mut s, pos(379, true), 380).unwrap();
        assert!(finished_at(&s).is_some());
    }

    #[test]
    fn unfinish_then_save_at_end_does_not_refinish_and_keeps_position() {
        let (_d, mut s) = fixture();
        save(&mut s, pos(379, false), 380).unwrap();
        save(&mut s, pos(380, true), 380).unwrap();
        assert!(finished_at(&s).is_some());
        let v = set_finished(&mut s, &id(), false, t(50)).unwrap();
        assert_eq!(v.section, Some(Section::Reading));
        assert_eq!(s.lib.books[&id()].position, Some(pos(380, true)));
        save(&mut s, pos(380, true), 380).unwrap();
        assert!(finished_at(&s).is_none());
    }

    #[test]
    fn finishing_keeps_the_first_finished_date() {
        let (_d, mut s) = fixture();
        save(&mut s, pos(379, false), 380).unwrap();
        save(&mut s, pos(380, false), 380).unwrap();
        let first = finished_at(&s);
        save_position(&mut s, &id(), pos(1, false), Some(380), false, t(55)).unwrap();
        save_position(&mut s, &id(), pos(380, false), Some(380), false, t(58)).unwrap();
        assert_eq!(finished_at(&s), first);
    }

    #[test]
    fn set_finished_true_without_reading_moves_to_finished_and_keeps_null_position() {
        let (_d, mut s) = fixture();
        let v = set_finished(&mut s, &id(), true, t(5)).unwrap();
        assert_eq!(v.section, Some(Section::Finished));
        assert!(v.book.position.is_none());
        let v = set_finished(&mut s, &id(), true, t(9)).unwrap();
        assert_eq!(v.book.finished_at, Some(iso(t(5))), "already finished: date unchanged");
        let v = set_finished(&mut s, &id(), false, t(9)).unwrap();
        assert_eq!(v.section, Some(Section::ToRead));
    }

    #[test]
    fn bad_positions_are_rejected_and_change_nothing() {
        let (_d, mut s) = fixture();
        let before = s.lib.clone();
        let named = |n: &str| Zoom::Named(n.into());
        let cases = [
            (with_pos(0, 0.0, 0.0, Zoom::Scale(100.0)), 10),
            (with_pos(11, 0.0, 0.0, Zoom::Scale(100.0)), 10),
            (with_pos(-1, 0.0, 0.0, Zoom::Scale(100.0)), 10),
            (with_pos(1, 0.0, 0.0, Zoom::Scale(100.0)), 0),
            (with_pos(1, 0.0, 0.0, Zoom::Scale(0.0)), 10),
            (with_pos(1, 0.0, 0.0, Zoom::Scale(-50.0)), 10),
            (with_pos(1, 0.0, 0.0, Zoom::Scale(f64::NAN)), 10),
            (with_pos(1, 0.0, 0.0, named("huge")), 10),
            (with_pos(1, 0.0, 0.0, named("")), 10),
            (with_pos(1, f64::NAN, 0.0, named("auto")), 10),
            (with_pos(1, f64::INFINITY, 0.0, named("auto")), 10),
            (with_pos(1, 0.0, f64::NEG_INFINITY, named("auto")), 10),
            (with_pos(1, 0.0, f64::NAN, named("auto")), 10),
        ];
        for (p, count) in cases {
            assert_eq!(save(&mut s, p.clone(), count), Err("bad_position".into()), "{p:?} / {count}");
        }
        assert_eq!(s.lib, before);
    }

    #[test]
    fn negative_left_and_all_zoom_names_are_accepted() {
        let (_d, mut s) = fixture();
        for n in ["auto", "page-actual", "page-fit", "page-width"] {
            let p = with_pos(2, 0.0, -35.5, Zoom::Named(n.into()));
            assert!(save(&mut s, p, 10).is_ok(), "{n}");
        }
        assert!(save(&mut s, with_pos(2, -3.0, -35.5, Zoom::Scale(0.5)), 10).is_ok());
    }

    #[test]
    fn unknown_book_everywhere() {
        let (_d, mut s) = fixture();
        let nope = format!("{}-1", "c".repeat(64));
        assert_eq!(open_book(&mut s, &nope, t(1)), Err("unknown_book".into()));
        assert_eq!(
            save_position(&mut s, &nope, pos(1, false), Some(10), false, t(1)),
            Err("unknown_book".into())
        );
        assert_eq!(set_meta(&mut s, &nope, None, None, Some(3)), Err("unknown_book".into()));
        assert_eq!(save_cover(&s, &nope, PNG_SIGNATURE), Err("unknown_book".into()));
        assert_eq!(set_finished(&mut s, &nope, true, t(1)).unwrap_err(), "unknown_book");
    }

    #[test]
    fn open_book_sets_last_opened_returns_path_and_position() {
        let (_d, mut s) = fixture();
        save(&mut s, pos(4, false), 10).unwrap();
        let o = open_book(&mut s, &id(), t(20)).unwrap();
        assert_eq!(o, Opened { path: "C:\\b\\a.pdf".into(), position: Some(pos(4, false)) });
        assert_eq!(s.lib.books[&id()].last_opened_at, Some(iso(t(20))));
    }

    #[test]
    fn open_book_refuses_a_book_whose_file_is_gone() {
        let (_d, mut s) = fixture();
        s.present.clear();
        assert_eq!(open_book(&mut s, &id(), t(20)), Err("not_present".into()));
        assert!(s.lib.books[&id()].last_opened_at.is_none());
    }

    #[test]
    fn set_meta_stores_trimmed_title_and_page_count() {
        let (_d, mut s) = fixture();
        set_meta(&mut s, &id(), Some("  Dune ".into()), None, Some(412)).unwrap();
        let b = &s.lib.books[&id()];
        assert_eq!(b.meta_title.as_deref(), Some("Dune"));
        assert_eq!(b.page_count, Some(412));
        set_meta(&mut s, &id(), Some("   ".into()), None, Some(0)).unwrap();
        let b = &s.lib.books[&id()];
        assert_eq!(b.meta_title, None);
        assert_eq!(b.page_count, Some(412), "a bad page count is ignored");
    }

    #[test]
    fn epub_position_saves_and_progress_equals_fraction() {
        let (_d, mut s) = fixture();
        let p = epos("epubcfi(/6/4!/4/2)", 0.34, Some("Chapter 3"));
        let r = save_epub(&mut s, p.clone(), false).unwrap();
        assert_eq!(r, Saved { progress: 0.34, finished_at: None });
        let b = &s.lib.books[&epub_id()];
        assert_eq!(b.position, Some(p));
        assert_eq!(b.progress, 0.34);
        assert_eq!(b.page_count, None, "an EPUB never gets a page count");
    }

    #[test]
    fn bad_epub_positions_are_rejected_and_change_nothing() {
        let (_d, mut s) = fixture();
        let before = s.lib.clone();
        let long = format!("epubcfi({}", "1".repeat(2000));
        let cases = [
            epos("/6/4!/4/2", 0.5, None),
            epos("EPUBCFI(/6/4)", 0.5, None),
            epos("", 0.5, None),
            epos(&long, 0.5, None),
            epos("epubcfi(/6/4)", f64::NAN, None),
            epos("epubcfi(/6/4)", 1.5, None),
            epos("epubcfi(/6/4)", -0.1, None),
            epos("epubcfi(/6/4)", f64::INFINITY, None),
        ];
        for p in cases {
            assert_eq!(save_epub(&mut s, p.clone(), false), Err("bad_position".into()), "{p:?}");
        }
        assert_eq!(s.lib, before);
    }

    #[test]
    fn epub_cfi_of_exactly_2000_chars_and_fraction_edges_are_accepted() {
        let (_d, mut s) = fixture();
        let edge = format!("epubcfi({}", "1".repeat(2000 - "epubcfi(".len()));
        assert_eq!(edge.chars().count(), 2000);
        assert!(save_epub(&mut s, epos(&edge, 0.0, None), false).is_ok());
        assert!(save_epub(&mut s, epos(&edge, 1.0, None), false).is_ok());
    }

    #[test]
    fn epub_chapter_is_trimmed_and_cut_to_200_chars_on_a_char_boundary() {
        let (_d, mut s) = fixture();
        let chapter = |s: &Store| match &s.lib.books[&epub_id()].position {
            Some(Position::Epub(p)) => p.chapter.clone(),
            other => panic!("{other:?}"),
        };
        save_epub(&mut s, epos("epubcfi(/6/4)", 0.5, Some("  Chapter 3  ")), false).unwrap();
        assert_eq!(chapter(&s).as_deref(), Some("Chapter 3"));
        save_epub(&mut s, epos("epubcfi(/6/4)", 0.5, Some(&"é".repeat(250))), false).unwrap();
        assert_eq!(chapter(&s), Some("é".repeat(200)));
        save_epub(&mut s, epos("epubcfi(/6/4)", 0.5, Some("   ")), false).unwrap();
        assert_eq!(chapter(&s), None);
    }

    #[test]
    fn position_kind_must_match_the_book_format() {
        let (_d, mut s) = fixture();
        let before = s.lib.clone();
        assert_eq!(
            save_position(&mut s, &epub_id(), pos(1, false), Some(10), false, t(30)),
            Err("bad_position".into()),
            "pdf position on an epub book"
        );
        assert_eq!(
            save_position(&mut s, &id(), at(0.5), Some(10), false, t(30)),
            Err("bad_position".into()),
            "epub position on a pdf book"
        );
        assert_eq!(s.lib, before);
    }

    #[test]
    fn pdf_save_without_a_page_count_is_refused() {
        let (_d, mut s) = fixture();
        let before = s.lib.clone();
        assert_eq!(
            save_position(&mut s, &id(), pos(1, false), None, false, t(30)),
            Err("bad_position".into())
        );
        assert_eq!(s.lib, before);
    }

    #[test]
    fn epub_is_finished_at_98_percent_but_not_at_97() {
        let (_d, mut s) = fixture();
        save_epub(&mut s, at(0.97), false).unwrap();
        assert_eq!(s.lib.books[&epub_id()].finished_at, None);
        let r = save_epub(&mut s, at(0.98), false).unwrap();
        assert_eq!(s.lib.books[&epub_id()].finished_at, Some(iso(t(30))));
        assert_eq!(r.finished_at, Some(iso(t(30))));
    }

    #[test]
    fn epub_at_end_flag_finishes_at_half_way_and_a_later_save_at_10_percent_keeps_it() {
        let (_d, mut s) = fixture();
        save_epub(&mut s, at(0.5), true).unwrap();
        let first = s.lib.books[&epub_id()].finished_at.clone();
        assert!(first.is_some());
        save_position(&mut s, &epub_id(), at(0.1), None, false, t(45)).unwrap();
        assert_eq!(s.lib.books[&epub_id()].finished_at, first);
        assert_eq!(s.lib.books[&epub_id()].progress, 0.1);
    }

    #[test]
    fn epub_marked_unfinished_at_the_end_is_not_finished_again_by_the_same_spot() {
        let (_d, mut s) = fixture();
        save_epub(&mut s, at(0.99), false).unwrap();
        set_finished(&mut s, &epub_id(), false, t(40)).unwrap();
        save_epub(&mut s, at(0.99), false).unwrap();
        assert_eq!(s.lib.books[&epub_id()].finished_at, None);
    }

    #[test]
    fn set_meta_with_an_author_and_no_page_count_stores_the_author_and_sets_cover_checked() {
        let (_d, mut s) = fixture();
        set_meta(&mut s, &epub_id(), Some("Dune".into()), Some("  Frank Herbert ".into()), None)
            .unwrap();
        let b = &s.lib.books[&epub_id()];
        assert_eq!(b.meta_title.as_deref(), Some("Dune"));
        assert_eq!(b.meta_author.as_deref(), Some("Frank Herbert"));
        assert_eq!(b.page_count, None);
        assert!(b.cover_checked);
        set_meta(&mut s, &epub_id(), None, Some("  ".into()), None).unwrap();
        assert_eq!(s.lib.books[&epub_id()].meta_author, None);
    }

    #[test]
    fn set_meta_on_a_pdf_also_sets_cover_checked() {
        let (_d, mut s) = fixture();
        assert!(!s.lib.books[&id()].cover_checked);
        set_meta(&mut s, &id(), None, None, Some(3)).unwrap();
        assert!(s.lib.books[&id()].cover_checked);
    }

    #[test]
    fn epub_settings_store_valid_values_and_refuse_others_with_bad_setting() {
        let (_d, mut s) = fixture();
        assert_eq!(s.lib.settings.epub_font_size, 100);
        for size in [80, 90, 100, 115, 130, 160, 200] {
            set_epub_settings(&mut s, size, "scrolled").unwrap();
            assert_eq!(s.lib.settings.epub_font_size, size as u32);
        }
        assert_eq!(s.lib.settings.epub_layout, EpubLayout::Scrolled);
        set_epub_settings(&mut s, 115, "paginated").unwrap();
        assert_eq!(s.lib.settings.epub_layout, EpubLayout::Paginated);

        let before = s.lib.settings.clone();
        for (size, layout) in [(95, "paginated"), (115, "columns"), (115, "Paginated"), (-100, "paginated")] {
            assert_eq!(set_epub_settings(&mut s, size, layout), Err("bad_setting".into()), "{size} {layout}");
        }
        assert_eq!(s.lib.settings, before);
    }

    #[test]
    fn highlight_commands_refuse_an_epub_book_but_a_pdf_book_still_works() {
        let (d, s) = fixture();
        // every highlight command in lib.rs finds its book through this one function
        assert_eq!(highlight_book(&s, &epub_id()), Err("not_supported".into()));
        assert_eq!(highlight_book(&s, &id()), Ok((d.path().to_path_buf(), None)));
        let nope = format!("{}-1", "c".repeat(64));
        assert_eq!(highlight_book(&s, &nope), Err("unknown_book".into()));
        assert_eq!(highlight_book(&s, "../../evil"), Err("unknown_book".into()));
    }

    #[test]
    fn save_cover_writes_a_png_under_covers() {
        let (d, s) = fixture();
        let mut png = PNG_SIGNATURE.to_vec();
        png.extend_from_slice(b"rest");
        save_cover(&s, &id(), &png).unwrap();
        assert_eq!(std::fs::read(d.path().join("covers").join(format!("{}.png", id()))).unwrap(), png);
        assert!(!d.path().join("covers").join(format!("{}.png.tmp", id())).exists());
    }

    #[test]
    fn save_cover_rejects_a_traversal_id_and_writes_nothing() {
        let (d, s) = fixture();
        let outer = d.path().parent().unwrap().to_path_buf();
        for evil in ["../../evil", "..\\..\\evil", "../evil"] {
            assert_eq!(save_cover(&s, evil, PNG_SIGNATURE), Err("unknown_book".into()), "{evil}");
        }
        assert!(!outer.join("evil.png").exists());
        assert!(!d.path().join("evil.png").exists());
        assert!(!d.path().join("covers").exists());
    }

    #[test]
    fn save_cover_rejects_non_png_and_oversize() {
        let (d, s) = fixture();
        assert_eq!(save_cover(&s, &id(), b"GIF89a not a png"), Err("bad_image".into()));
        assert_eq!(save_cover(&s, &id(), b""), Err("bad_image".into()));
        let mut big = PNG_SIGNATURE.to_vec();
        big.resize(MAX_COVER_BYTES + 1, 0);
        assert_eq!(save_cover(&s, &id(), &big), Err("bad_image".into()));
        big.truncate(MAX_COVER_BYTES);
        assert!(save_cover(&s, &id(), &big).is_ok(), "exactly 2 MB is allowed");
        assert!(d.path().join("covers").exists());
    }

    #[test]
    fn page_theme_validation() {
        let (_d, mut s) = fixture();
        set_page_theme(&mut s, "night").unwrap();
        assert_eq!(s.lib.settings.page_theme, PageTheme::Night);
        assert_eq!(set_page_theme(&mut s, "sepia"), Err("bad_theme".into()));
        assert_eq!(set_page_theme(&mut s, "Night"), Err("bad_theme".into()));
        assert_eq!(s.lib.settings.page_theme, PageTheme::Night);
    }

    #[test]
    fn set_folder_requires_a_directory_and_dismiss_clears_the_notice() {
        let (d, mut s) = fixture();
        let file = d.path().join("f.txt");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(set_folder(&mut s, &file.to_string_lossy()), Err("not_a_folder".into()));
        assert_eq!(set_folder(&mut s, "Z:\\definitely\\not\\here"), Err("not_a_folder".into()));
        assert!(s.lib.settings.books_folder.is_none());
        set_folder(&mut s, &d.path().to_string_lossy()).unwrap();
        assert_eq!(s.lib.settings.books_folder.as_deref(), Some(&*d.path().to_string_lossy()));

        s.notice = Some("hi".into());
        dismiss_notice(&mut s);
        assert!(s.notice.is_none());
    }

    #[test]
    fn mutations_are_saved_to_disk() {
        let (d, mut s) = fixture();
        save(&mut s, pos(2, false), 10).unwrap();
        let (back, _) = crate::library::load(d.path(), t(0));
        assert_eq!(back.books[&id()].position, Some(pos(2, false)));
    }
}
