//! Command logic as plain functions over the store, so it is testable without
//! a running Tauri app. Error strings are the bare codes the screen expects.

use crate::derive::{book_view, BookView};
use crate::library::{iso, is_valid_id, PageTheme, PdfPosition, Position, Store, Zoom};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::path::Path;

const MAX_COVER_BYTES: usize = 2 * 1024 * 1024;
const PNG_SIGNATURE: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
const ZOOM_NAMES: [&str; 4] = ["auto", "page-actual", "page-fit", "page-width"];

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

pub fn save_position(
    store: &mut Store,
    id: &str,
    position: Position,
    page_count: i64,
    now: DateTime<Utc>,
) -> Result<Saved, String> {
    let Position::Pdf(p) = &position;
    let Some(book) = store.lib.books.get_mut(id) else {
        return err("unknown_book");
    };
    validate_position(p, page_count)?;

    let at_end = |q: &PdfPosition| q.at_end || q.page == page_count;
    // A null previous position counts as not at end.
    let was_at_end = matches!(&book.position, Some(Position::Pdf(q)) if at_end(q));
    if !was_at_end && at_end(p) && book.finished_at.is_none() {
        book.finished_at = Some(iso(now));
    }
    book.progress = (p.page as f64 / page_count as f64).clamp(0.0, 1.0);
    book.page_count = Some(page_count as u32);
    book.position = Some(position);
    let saved = Saved { progress: book.progress, finished_at: book.finished_at.clone() };
    store.persist();
    Ok(saved)
}

pub fn set_meta(
    store: &mut Store,
    id: &str,
    meta_title: Option<String>,
    page_count: i64,
) -> Result<(), String> {
    let Some(book) = store.lib.books.get_mut(id) else {
        return err("unknown_book");
    };
    book.meta_title = meta_title.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    if let Ok(n) = u32::try_from(page_count) {
        if n >= 1 {
            book.page_count = Some(n);
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::derive::Section;
    use crate::library::{Book, Format};
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
            },
        );
        s.present.insert(id());
        (d, s)
    }

    fn save(s: &mut Store, p: Position, count: i64) -> Result<Saved, String> {
        save_position(s, &id(), p, count, t(30))
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
        save_position(&mut s, &id(), pos(1, false), 380, t(55)).unwrap();
        save_position(&mut s, &id(), pos(380, false), 380, t(58)).unwrap();
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
        assert_eq!(save_position(&mut s, &nope, pos(1, false), 10, t(1)), Err("unknown_book".into()));
        assert_eq!(set_meta(&mut s, &nope, None, 3), Err("unknown_book".into()));
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
        set_meta(&mut s, &id(), Some("  Dune ".into()), 412).unwrap();
        let b = &s.lib.books[&id()];
        assert_eq!(b.meta_title.as_deref(), Some("Dune"));
        assert_eq!(b.page_count, Some(412));
        set_meta(&mut s, &id(), Some("   ".into()), 0).unwrap();
        let b = &s.lib.books[&id()];
        assert_eq!(b.meta_title, None);
        assert_eq!(b.page_count, Some(412), "a bad page count is ignored");
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
