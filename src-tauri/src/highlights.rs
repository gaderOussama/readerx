//! Highlights: one JSON file per book in `highlights\` under the app data
//! folder, keyed by the book id. Every command reads the file, changes it and
//! saves it whole, under one mutex per book id (separate from the library
//! `Store` lock), so two calls for the same book never lose an update.
//! Error strings are the bare codes the screen expects.

use crate::library::{is_valid_id, iso, save_file, tmp_path_of};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_TEXT: usize = 5_000;
pub const MAX_NOTE: usize = 10_000;
pub const MAX_RECTS: usize = 200;
/// Shown in the panel while a book's damaged file notice is active.
pub const DAMAGED_NOTICE: &str = "Highlights for this book were damaged and set aside.";

/// `[x1, y1, x2, y2]` in PDF page units, `x1 < x2` and `y1 < y2`.
pub type Rect = [f64; 4];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Highlight {
    pub id: String,
    pub page: u32,
    pub rects: Vec<Rect>,
    pub text: String,
    pub note: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HighlightFile {
    pub schema_version: u32,
    pub book_id: String,
    pub export_name: Option<String>,
    pub highlights: Vec<Highlight>,
}

impl HighlightFile {
    fn new(id: &str) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            book_id: id.to_string(),
            export_name: None,
            highlights: Vec::new(),
        }
    }
}

/// One selection piece from the screen. Loose types (i64, plain vectors) so a
/// bad value reaches validation and comes back as `bad_highlight`.
#[derive(Deserialize, Clone, Debug)]
pub struct Item {
    pub page: i64,
    pub rects: Vec<Vec<f64>>,
    pub text: String,
}

/// What `get_highlights` needs from this module; the wrapper adds the export fields.
#[derive(Debug, PartialEq)]
pub struct Listing {
    pub highlights: Vec<Highlight>,
    pub notice: Option<String>,
}

/// A book id that passed `is_valid_id`, so it is safe in a path, plus the
/// page count when known (the upper bound for `page`).
#[derive(Clone, Copy)]
pub struct Ctx<'a> {
    dir: &'a Path,
    id: &'a str,
    page_count: Option<u32>,
}

impl<'a> Ctx<'a> {
    pub fn new(dir: &'a Path, id: &'a str, page_count: Option<u32>) -> Result<Self, String> {
        if is_valid_id(id) {
            Ok(Self { dir, id, page_count })
        } else {
            Err("unknown_book".to_string())
        }
    }

    pub fn id(&self) -> &str {
        self.id
    }

    pub fn file(&self) -> PathBuf {
        highlights_dir(self.dir).join(format!("{}.json", self.id))
    }
}

pub fn highlights_dir(dir: &Path) -> PathBuf {
    dir.join("highlights")
}

/// Session-only state of one book, behind that book's mutex.
#[derive(Default)]
pub struct Slot {
    undo: Option<Highlight>,
    /// A damaged file was set aside this session; reported until the app closes.
    damaged: bool,
}

#[derive(Default)]
pub struct Books(Mutex<HashMap<String, Arc<Mutex<Slot>>>>);

impl Books {
    fn slot(&self, id: &str) -> Arc<Mutex<Slot>> {
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(id.to_string()).or_default().clone()
    }

    /// Called when the reader leaves a book: the one delete slot is dropped.
    pub fn forget_undo(&self, id: &str) {
        let slot = self.0.lock().unwrap_or_else(|e| e.into_inner()).get(id).cloned();
        if let Some(slot) = slot {
            lock_slot(&slot).undo = None;
        }
    }
}

fn lock_slot(slot: &Arc<Mutex<Slot>>) -> MutexGuard<'_, Slot> {
    slot.lock().unwrap_or_else(|e| e.into_inner())
}

fn err<T>(code: &str) -> Result<T, String> {
    Err(code.to_string())
}

// ---- validation ----

fn rect_ok(r: &[f64]) -> bool {
    r.len() == 4 && r.iter().all(|v| v.is_finite()) && r[0] < r[2] && r[1] < r[3]
}

fn is_hex16(s: &str) -> bool {
    s.len() == 16 && s.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
}

fn page_ok(page: i64, page_count: Option<u32>) -> bool {
    page >= 1 && page <= i64::from(u32::MAX) && page_count.is_none_or(|n| page <= i64::from(n))
}

/// The field rules of the data model, for a record read back from disk.
fn highlight_ok(h: &Highlight, page_count: Option<u32>) -> bool {
    let text_len = h.text.chars().count();
    is_hex16(&h.id)
        && page_ok(i64::from(h.page), page_count)
        && (1..=MAX_RECTS).contains(&h.rects.len())
        && h.rects.iter().all(|r| rect_ok(r))
        && (1..=MAX_TEXT).contains(&text_len)
        && h.note.chars().count() <= MAX_NOTE
        && DateTime::parse_from_rfc3339(&h.created_at).is_ok()
        && DateTime::parse_from_rfc3339(&h.updated_at).is_ok()
}

fn file_ok(f: &HighlightFile, ctx: &Ctx) -> bool {
    let mut ids = std::collections::HashSet::new();
    f.schema_version == SCHEMA_VERSION
        && f.book_id == ctx.id
        && f.highlights.iter().all(|h| highlight_ok(h, ctx.page_count) && ids.insert(&h.id))
}

// ---- load and save ----

enum Loaded {
    Missing,
    Found(HighlightFile),
    Damaged,
}

fn load(ctx: &Ctx, now: DateTime<Utc>) -> Loaded {
    let path = ctx.file();
    let _ = std::fs::remove_file(tmp_path_of(&path));
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Loaded::Missing,
        Err(_) => return set_aside(ctx, now),
    };
    match serde_json::from_slice::<HighlightFile>(&bytes) {
        Ok(mut f) if file_ok(&f, ctx) => {
            sort_list(&mut f.highlights);
            Loaded::Found(f)
        }
        _ => set_aside(ctx, now),
    }
}

/// `<id>.corrupt-YYYYMMDD-HHMMSS.json` (UTC), `-2`, `-3` ... if that name exists.
fn set_aside(ctx: &Ctx, now: DateTime<Utc>) -> Loaded {
    let base = format!("{}.corrupt-{}", ctx.id, now.format("%Y%m%d-%H%M%S"));
    let dir = highlights_dir(ctx.dir);
    let target = (1..)
        .map(|n| if n == 1 { format!("{base}.json") } else { format!("{base}-{n}.json") })
        .map(|name| dir.join(name))
        .find(|p| !p.exists())
        .expect("unbounded range");
    if let Err(e) = std::fs::rename(ctx.file(), &target) {
        eprintln!("readerx: could not set aside {}: {e}", ctx.file().display());
    }
    Loaded::Damaged
}

/// The book's file, or None when it has none or it was just set aside.
fn read(ctx: &Ctx, slot: &mut Slot, now: DateTime<Utc>) -> Option<HighlightFile> {
    match load(ctx, now) {
        Loaded::Found(f) => Some(f),
        Loaded::Missing => None,
        Loaded::Damaged => {
            slot.damaged = true;
            None
        }
    }
}

fn write(ctx: &Ctx, file: &mut HighlightFile) -> Result<(), String> {
    sort_list(&mut file.highlights);
    let path = ctx.file();
    let bytes = serde_json::to_vec_pretty(file).map_err(|_| "save_failed".to_string())?;
    std::fs::create_dir_all(highlights_dir(ctx.dir))
        .and_then(|()| save_file(&path, &bytes))
        .map_err(|_| "save_failed".to_string())
}

/// Ids that have a highlights file, damaged or not.
pub fn list_ids(dir: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(highlights_dir(dir)) else {
        return Vec::new();
    };
    rd.flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter_map(|n| n.strip_suffix(".json").map(str::to_string))
        .filter(|id| is_valid_id(id))
        .collect()
}

// ---- ordering ----

/// Top of the first box, highest first, then its left edge.
fn reading_cmp(a: &Rect, b: &Rect) -> Ordering {
    b[3].total_cmp(&a[3]).then(a[0].total_cmp(&b[0]))
}

fn first_rect(h: &Highlight) -> Rect {
    h.rects.first().copied().unwrap_or([0.0; 4])
}

fn sort_list(list: &mut [Highlight]) {
    list.sort_by(|a, b| {
        a.page
            .cmp(&b.page)
            .then_with(|| reading_cmp(&first_rect(a), &first_rect(b)))
            .then_with(|| a.created_at.cmp(&b.created_at))
            .then_with(|| a.id.cmp(&b.id))
    });
}

// ---- add and merge ----

/// 16 hex characters from the standard library's randomly keyed hasher.
fn random_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    h.write_u128(nanos);
    format!("{:016x}", h.finish())
}

fn fresh_id(list: &[Highlight]) -> String {
    loop {
        let id = random_id();
        if !list.iter().any(|h| h.id == id) {
            return id;
        }
    }
}

fn candidate(item: Item, ctx: &Ctx, list: &[Highlight], stamp: &str) -> Result<Highlight, String> {
    if !page_ok(item.page, ctx.page_count) || item.rects.is_empty() {
        return err("bad_highlight");
    }
    if item.rects.len() > MAX_RECTS {
        return err("too_long");
    }
    if !item.rects.iter().all(|r| rect_ok(r)) {
        return err("bad_highlight");
    }
    let text = item.text.trim().to_string();
    let len = text.chars().count();
    if len == 0 {
        return err("bad_highlight");
    }
    if len > MAX_TEXT {
        return err("too_long");
    }
    Ok(Highlight {
        id: fresh_id(list),
        page: item.page as u32,
        rects: item.rects.iter().map(|r| [r[0], r[1], r[2], r[3]]).collect(),
        text,
        note: String::new(),
        created_at: stamp.to_string(),
        updated_at: stamp.to_string(),
    })
}

/// Boxes that only touch along an edge do not intersect.
fn overlaps(a: &[Rect], b: &[Rect]) -> bool {
    a.iter().any(|p| b.iter().any(|q| p[0] < q[2] && q[0] < p[2] && p[1] < q[3] && q[1] < p[3]))
}

fn contains_words(hay: &[&str], needle: &[&str]) -> bool {
    needle.is_empty() || hay.windows(needle.len()).any(|w| w == needle)
}

/// Quotes in reading order joined with the words repeated at each seam dropped.
fn join_quotes(quotes: &[&str]) -> String {
    let mut acc: Vec<&str> = Vec::new();
    for q in quotes {
        let w: Vec<&str> = q.split_whitespace().collect();
        if contains_words(&acc, &w) {
            continue;
        }
        if contains_words(&w, &acc) {
            acc = w;
            continue;
        }
        let k = (1..=acc.len().min(w.len())).rev().find(|&k| acc[acc.len() - k..] == w[..k]);
        acc.extend_from_slice(&w[k.unwrap_or(0)..]);
    }
    acc.join(" ")
}

/// Merge `cand` with the highlights it overlaps. The oldest keeps its id and
/// `createdAt`; `too_long` when the result passes a limit.
fn merge(absorbed: &[&Highlight], cand: &Highlight, stamp: &str) -> Result<Highlight, String> {
    let mut pool: Vec<&Highlight> = absorbed.to_vec();
    pool.push(cand);
    // stable, and the candidate is last, so a tie keeps the older list entry
    pool.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    let oldest = pool[0];

    let covers = |t: &str| contains_words(
        &cand.text.split_whitespace().collect::<Vec<_>>(),
        &t.split_whitespace().collect::<Vec<_>>(),
    );
    let text = if absorbed.iter().all(|h| covers(&h.text)) {
        cand.text.clone()
    } else {
        let mut by_reading = pool.clone();
        by_reading.sort_by(|a, b| reading_cmp(&first_rect(a), &first_rect(b)));
        join_quotes(&by_reading.iter().map(|h| h.text.as_str()).collect::<Vec<_>>())
    };

    let mut rects: Vec<Rect> = Vec::new();
    for r in pool.iter().flat_map(|h| h.rects.iter()) {
        if !rects.contains(r) {
            rects.push(*r);
        }
    }
    rects.sort_by(reading_cmp);

    let note = pool
        .iter()
        .map(|h| h.note.as_str())
        .filter(|n| !n.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");

    if text.chars().count() > MAX_TEXT || rects.len() > MAX_RECTS || note.chars().count() > MAX_NOTE
    {
        return err("too_long");
    }
    Ok(Highlight {
        id: oldest.id.clone(),
        page: cand.page,
        rects,
        text,
        note,
        created_at: oldest.created_at.clone(),
        updated_at: stamp.to_string(),
    })
}

/// Put `cand` in the list, merging it with every highlight it overlaps on its
/// page, directly or through a chain. The list is untouched on error.
fn place(list: &mut Vec<Highlight>, cand: Highlight, stamp: &str) -> Result<(), String> {
    let mut taken: Vec<usize> = Vec::new();
    let mut reach = cand.rects.clone();
    while let Some(i) = (0..list.len()).find(|i| {
        !taken.contains(i) && list[*i].page == cand.page && overlaps(&list[*i].rects, &reach)
    }) {
        reach.extend(list[i].rects.iter().copied());
        taken.push(i);
    }
    if taken.is_empty() {
        list.push(cand);
    } else {
        taken.sort_unstable();
        let absorbed: Vec<&Highlight> = taken.iter().map(|&i| &list[i]).collect();
        let merged = merge(&absorbed, &cand, stamp)?;
        for &i in taken.iter().rev() {
            list.remove(i);
        }
        list.push(merged);
    }
    sort_list(list);
    Ok(())
}

// ---- commands ----

pub fn get(ctx: &Ctx, books: &Books, now: DateTime<Utc>) -> Listing {
    let slot = books.slot(ctx.id);
    let mut slot = lock_slot(&slot);
    let file = read(ctx, &mut slot, now);
    Listing {
        highlights: file.map(|f| f.highlights).unwrap_or_default(),
        notice: slot.damaged.then(|| DAMAGED_NOTICE.to_string()),
    }
}

/// All items are checked and placed in memory first; one save at the end.
pub fn add(
    ctx: &Ctx,
    books: &Books,
    items: Vec<Item>,
    now: DateTime<Utc>,
) -> Result<Vec<Highlight>, String> {
    if items.is_empty() {
        return err("bad_highlight");
    }
    let slot = books.slot(ctx.id);
    let mut slot = lock_slot(&slot);
    let mut file = read(ctx, &mut slot, now).unwrap_or_else(|| HighlightFile::new(ctx.id));
    let stamp = iso(now);
    for item in items {
        let cand = candidate(item, ctx, &file.highlights, &stamp)?;
        place(&mut file.highlights, cand, &stamp)?;
    }
    write(ctx, &mut file)?;
    Ok(file.highlights)
}

pub fn set_note(
    ctx: &Ctx,
    books: &Books,
    highlight_id: &str,
    note: &str,
    now: DateTime<Utc>,
) -> Result<Vec<Highlight>, String> {
    if note.chars().count() > MAX_NOTE {
        return err("bad_highlight");
    }
    let slot = books.slot(ctx.id);
    let mut slot = lock_slot(&slot);
    let mut file = read(ctx, &mut slot, now).unwrap_or_else(|| HighlightFile::new(ctx.id));
    let Some(h) = file.highlights.iter_mut().find(|h| h.id == highlight_id) else {
        return err("unknown_highlight");
    };
    if h.note != note {
        h.note = note.to_string();
        h.updated_at = iso(now);
        write(ctx, &mut file)?;
    }
    Ok(file.highlights)
}

pub fn delete(
    ctx: &Ctx,
    books: &Books,
    highlight_id: &str,
    now: DateTime<Utc>,
) -> Result<Vec<Highlight>, String> {
    let slot = books.slot(ctx.id);
    let mut slot = lock_slot(&slot);
    let mut file = read(ctx, &mut slot, now).unwrap_or_else(|| HighlightFile::new(ctx.id));
    let Some(i) = file.highlights.iter().position(|h| h.id == highlight_id) else {
        return err("unknown_highlight");
    };
    let removed = file.highlights.remove(i);
    write(ctx, &mut file)?;
    slot.undo = Some(removed);
    Ok(file.highlights)
}

/// Puts the last deleted record back as it was; merges like an add only when
/// it overlaps a newer highlight.
pub fn restore(ctx: &Ctx, books: &Books, now: DateTime<Utc>) -> Result<Vec<Highlight>, String> {
    let slot = books.slot(ctx.id);
    let mut slot = lock_slot(&slot);
    let Some(h) = slot.undo.clone() else {
        return err("nothing_to_restore");
    };
    let mut file = read(ctx, &mut slot, now).unwrap_or_else(|| HighlightFile::new(ctx.id));
    place(&mut file.highlights, h, &iso(now))?;
    write(ctx, &mut file)?;
    slot.undo = None;
    Ok(file.highlights)
}

// ---- used by the export worker ----

/// The book's file for export; None when it has none or it is damaged.
pub fn read_for_export(ctx: &Ctx, books: &Books, now: DateTime<Utc>) -> Option<HighlightFile> {
    let slot = books.slot(ctx.id);
    let mut slot = lock_slot(&slot);
    read(ctx, &mut slot, now)
}

/// Sets `exportName` on the file as it is now (not the copy the export read).
pub fn set_export_name(
    ctx: &Ctx,
    books: &Books,
    name: Option<&str>,
    now: DateTime<Utc>,
) -> Result<(), String> {
    let slot = books.slot(ctx.id);
    let mut slot = lock_slot(&slot);
    match read(ctx, &mut slot, now) {
        Some(mut f) if f.export_name.as_deref() != name => {
            f.export_name = name.map(str::to_string);
            write(ctx, &mut f)
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t(sec: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 17, 9, 5, sec).unwrap()
    }

    fn bid() -> String {
        format!("{}-5", "c".repeat(64))
    }

    struct Fx {
        d: tempfile::TempDir,
        id: String,
        books: Books,
    }

    impl Fx {
        fn new() -> Self {
            Self { d: tempfile::tempdir().unwrap(), id: bid(), books: Books::default() }
        }
        fn ctx(&self) -> Ctx<'_> {
            Ctx::new(self.d.path(), &self.id, Some(50)).unwrap()
        }
        fn add(&self, items: Vec<Item>, sec: u32) -> Result<Vec<Highlight>, String> {
            add(&self.ctx(), &self.books, items, t(sec))
        }
        fn get(&self) -> Listing {
            get(&self.ctx(), &self.books, t(59))
        }
        fn raw(&self) -> serde_json::Value {
            serde_json::from_slice(&std::fs::read(self.ctx().file()).unwrap()).unwrap()
        }
        fn damaged_names(&self) -> Vec<String> {
            let mut v: Vec<String> = std::fs::read_dir(highlights_dir(self.d.path()))
                .unwrap()
                .flatten()
                .map(|e| e.file_name().into_string().unwrap())
                .filter(|n| n.contains(".corrupt-"))
                .collect();
            v.sort();
            v
        }
    }

    fn item(page: i64, x1: f64, y1: f64, x2: f64, y2: f64, text: &str) -> Item {
        Item { page, rects: vec![vec![x1, y1, x2, y2]], text: text.into() }
    }

    fn texts(l: &[Highlight]) -> Vec<&str> {
        l.iter().map(|h| h.text.as_str()).collect()
    }

    // ---- add, validation, sorting ----

    #[test]
    fn add_saves_a_camel_case_file_with_a_random_hex_id_and_stamps() {
        let fx = Fx::new();
        let out = fx.add(vec![item(3, 10.0, 20.0, 50.0, 32.0, "  hello world ")], 1).unwrap();
        assert_eq!(out.len(), 1);
        assert!(is_hex16(&out[0].id));
        assert_eq!(out[0].text, "hello world");
        assert_eq!(out[0].note, "");
        assert_eq!(out[0].created_at, "2026-11-17T09:05:01.000Z");
        let v = fx.raw();
        assert_eq!(v["schemaVersion"], 1);
        assert_eq!(v["bookId"], bid());
        assert!(v["exportName"].is_null());
        let h = &v["highlights"][0];
        assert_eq!(h["page"], 3);
        assert_eq!(h["rects"][0][2], 50.0);
        assert_eq!(h["createdAt"], h["updatedAt"]);
        assert!(!tmp_path_of(&fx.ctx().file()).exists());
    }

    #[test]
    fn get_never_creates_a_file_or_the_folder() {
        let fx = Fx::new();
        let l = fx.get();
        assert_eq!(l, Listing { highlights: vec![], notice: None });
        assert!(!highlights_dir(fx.d.path()).exists());
    }

    #[test]
    fn ids_are_random_and_unique() {
        let ids: std::collections::HashSet<String> = (0..500).map(|_| random_id()).collect();
        assert_eq!(ids.len(), 500);
        assert!(ids.iter().all(|i| is_hex16(i)));
    }

    #[test]
    fn a_bad_id_never_reaches_a_path() {
        let d = tempfile::tempdir().unwrap();
        let slashed = format!("{}-5/..", "c".repeat(64));
        let no_size = "c".repeat(64);
        for bad in ["", "../x", slashed.as_str(), no_size.as_str()] {
            assert_eq!(Ctx::new(d.path(), bad, None).err().as_deref(), Some("unknown_book"));
        }
    }

    #[test]
    fn validation_rejects_bad_pages_rects_and_text() {
        let fx = Fx::new();
        let bad = |it: Item| fx.add(vec![it], 1).unwrap_err();
        assert_eq!(bad(item(0, 0.0, 0.0, 1.0, 1.0, "x")), "bad_highlight");
        assert_eq!(bad(item(-1, 0.0, 0.0, 1.0, 1.0, "x")), "bad_highlight");
        assert_eq!(bad(item(51, 0.0, 0.0, 1.0, 1.0, "x")), "bad_highlight");
        assert_eq!(bad(item(1, 5.0, 0.0, 5.0, 1.0, "x")), "bad_highlight");
        assert_eq!(bad(item(1, 0.0, 9.0, 1.0, 1.0, "x")), "bad_highlight");
        assert_eq!(bad(item(1, f64::NAN, 0.0, 1.0, 1.0, "x")), "bad_highlight");
        assert_eq!(bad(item(1, 0.0, 0.0, f64::INFINITY, 1.0, "x")), "bad_highlight");
        assert_eq!(bad(item(1, 0.0, 0.0, 1.0, 1.0, "   ")), "bad_highlight");
        assert_eq!(bad(Item { page: 1, rects: vec![], text: "x".into() }), "bad_highlight");
        assert_eq!(bad(Item { page: 1, rects: vec![vec![0.0, 0.0, 1.0]], text: "x".into() }), "bad_highlight");
        assert_eq!(fx.add(vec![], 1).unwrap_err(), "bad_highlight");
        assert!(!fx.ctx().file().exists());
    }

    #[test]
    fn size_limits_are_too_long_and_the_edges_are_allowed() {
        let fx = Fx::new();
        let many = |n: usize| Item {
            page: 1,
            rects: (0..n).map(|i| vec![i as f64, 0.0, i as f64 + 0.5, 1.0]).collect(),
            text: "x".into(),
        };
        assert_eq!(fx.add(vec![many(201)], 1).unwrap_err(), "too_long");
        assert_eq!(fx.add(vec![item(1, 0.0, 0.0, 1.0, 1.0, &"a".repeat(5001))], 1).unwrap_err(), "too_long");
        assert!(fx.add(vec![many(200)], 1).is_ok());
        assert!(fx.add(vec![item(2, 0.0, 0.0, 1.0, 1.0, &"a".repeat(5000))], 2).is_ok());
    }

    #[test]
    fn page_is_unbounded_above_only_while_the_page_count_is_unknown() {
        let d = tempfile::tempdir().unwrap();
        let books = Books::default();
        let id = bid();
        let ctx = Ctx::new(d.path(), &id, None).unwrap();
        assert!(add(&ctx, &books, vec![item(9999, 0.0, 0.0, 1.0, 1.0, "x")], t(1)).is_ok());
    }

    #[test]
    fn one_bad_item_saves_nothing_from_the_batch() {
        let fx = Fx::new();
        let r = fx.add(
            vec![item(1, 0.0, 0.0, 1.0, 1.0, "ok"), item(0, 0.0, 0.0, 1.0, 1.0, "bad")],
            1,
        );
        assert_eq!(r.unwrap_err(), "bad_highlight");
        assert!(!fx.ctx().file().exists());
    }

    #[test]
    fn cross_page_selection_is_two_highlights_in_one_save() {
        let fx = Fx::new();
        let out = fx
            .add(
                vec![item(4, 0.0, 10.0, 90.0, 20.0, "end of three"), item(3, 0.0, 10.0, 90.0, 20.0, "start of four")],
                1,
            )
            .unwrap();
        assert_eq!(out.iter().map(|h| h.page).collect::<Vec<_>>(), vec![3, 4]);
        assert_eq!(fx.raw()["highlights"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn list_is_sorted_by_page_then_top_descending_then_left() {
        let fx = Fx::new();
        fx.add(vec![item(2, 0.0, 100.0, 10.0, 110.0, "p2 low")], 1).unwrap();
        fx.add(vec![item(1, 50.0, 700.0, 60.0, 710.0, "p1 high right")], 2).unwrap();
        fx.add(vec![item(1, 5.0, 700.0, 15.0, 710.0, "p1 high left")], 3).unwrap();
        fx.add(vec![item(1, 5.0, 300.0, 15.0, 310.0, "p1 low")], 4).unwrap();
        fx.add(vec![item(2, 0.0, 400.0, 10.0, 410.0, "p2 high")], 5).unwrap();
        let l = fx.get().highlights;
        assert_eq!(texts(&l), vec!["p1 high left", "p1 high right", "p1 low", "p2 high", "p2 low"]);
    }

    // ---- merge ----

    #[test]
    fn overlapping_boxes_on_the_same_page_merge_and_keep_oldest_id_and_date() {
        let fx = Fx::new();
        let first = fx.add(vec![item(1, 0.0, 0.0, 50.0, 10.0, "one two three four five")], 1).unwrap();
        let out = fx
            .add(vec![item(1, 30.0, 0.0, 90.0, 10.0, "three four five six seven eight nine")], 2)
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, first[0].id);
        assert_eq!(out[0].created_at, first[0].created_at);
        assert_eq!(out[0].updated_at, "2026-11-17T09:05:02.000Z");
    }

    #[test]
    fn merged_quote_uses_the_screens_combined_text_when_it_covers_the_old_one() {
        let fx = Fx::new();
        fx.add(vec![item(1, 0.0, 0.0, 50.0, 10.0, "one two three")], 1).unwrap();
        let out = fx.add(vec![item(1, 30.0, 0.0, 90.0, 10.0, "one two three four five")], 2).unwrap();
        assert_eq!(texts(&out), vec!["one two three four five"]);
    }

    #[test]
    fn merged_quote_falls_back_to_joining_without_repeated_words() {
        let fx = Fx::new();
        fx.add(vec![item(1, 0.0, 0.0, 50.0, 10.0, "w1 w2 w3 w4 w5")], 1).unwrap();
        let out = fx.add(vec![item(1, 30.0, 0.0, 90.0, 10.0, "w3 w4 w5 w6 w7 w8 w9")], 2).unwrap();
        assert_eq!(texts(&out), vec!["w1 w2 w3 w4 w5 w6 w7 w8 w9"]);
    }

    #[test]
    fn join_quotes_handles_seams_containment_and_no_overlap() {
        assert_eq!(join_quotes(&["a b c", "c d"]), "a b c d");
        assert_eq!(join_quotes(&["a b c d", "b c"]), "a b c d");
        assert_eq!(join_quotes(&["b c", "a b c d"]), "a b c d");
        assert_eq!(join_quotes(&["a b", "x y"]), "a b x y");
        assert_eq!(join_quotes(&["a b a", "a b a c"]), "a b a c");
    }

    #[test]
    fn a_selection_inside_an_old_highlight_keeps_the_old_quote() {
        let fx = Fx::new();
        fx.add(vec![item(1, 0.0, 0.0, 90.0, 10.0, "w1 w2 w3 w4 w5")], 1).unwrap();
        let out = fx.add(vec![item(1, 20.0, 0.0, 40.0, 10.0, "w3 w4")], 2).unwrap();
        assert_eq!(texts(&out), vec!["w1 w2 w3 w4 w5"]);
    }

    #[test]
    fn one_new_selection_can_bridge_several_old_highlights() {
        let fx = Fx::new();
        fx.add(vec![item(1, 0.0, 0.0, 20.0, 10.0, "aa bb")], 1).unwrap();
        fx.add(vec![item(1, 40.0, 0.0, 60.0, 10.0, "cc dd")], 2).unwrap();
        fx.add(vec![item(1, 80.0, 0.0, 100.0, 10.0, "ee ff")], 3).unwrap();
        let first_id = fx.get().highlights[0].id.clone();
        let out = fx.add(vec![item(1, 10.0, 0.0, 90.0, 10.0, "aa bb cc dd ee ff")], 4).unwrap();
        assert_eq!(texts(&out), vec!["aa bb cc dd ee ff"]);
        assert_eq!(out[0].id, first_id);
        assert_eq!(out[0].rects.len(), 4);
    }

    #[test]
    fn bridging_without_a_covering_quote_joins_all_in_reading_order() {
        let fx = Fx::new();
        fx.add(vec![item(1, 0.0, 0.0, 20.0, 10.0, "aa bb")], 1).unwrap();
        fx.add(vec![item(1, 80.0, 0.0, 100.0, 10.0, "ee ff")], 2).unwrap();
        let out = fx.add(vec![item(1, 10.0, 0.0, 90.0, 10.0, "cc dd")], 3).unwrap();
        assert_eq!(texts(&out), vec!["aa bb cc dd ee ff"]);
    }

    #[test]
    fn merged_notes_join_oldest_first_with_a_blank_line() {
        let fx = Fx::new();
        let a = fx.add(vec![item(1, 0.0, 0.0, 20.0, 10.0, "aa")], 1).unwrap()[0].id.clone();
        let b = fx.add(vec![item(1, 40.0, 0.0, 60.0, 10.0, "bb")], 2).unwrap()[1].id.clone();
        set_note(&fx.ctx(), &fx.books, &b, "note B", t(3)).unwrap();
        set_note(&fx.ctx(), &fx.books, &a, "note A", t(4)).unwrap();
        let out = fx.add(vec![item(1, 10.0, 0.0, 50.0, 10.0, "aa bb")], 5).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].note, "note A\n\nnote B");
        assert_eq!(out[0].id, a);
    }

    #[test]
    fn an_empty_note_adds_no_blank_lines_when_merging() {
        let fx = Fx::new();
        let a = fx.add(vec![item(1, 0.0, 0.0, 20.0, 10.0, "aa")], 1).unwrap()[0].id.clone();
        set_note(&fx.ctx(), &fx.books, &a, "only", t(2)).unwrap();
        let out = fx.add(vec![item(1, 10.0, 0.0, 50.0, 10.0, "aa bb")], 3).unwrap();
        assert_eq!(out[0].note, "only");
    }

    #[test]
    fn merged_rects_are_the_union_without_exact_duplicates() {
        let fx = Fx::new();
        fx.add(vec![item(1, 0.0, 0.0, 50.0, 10.0, "aa")], 1).unwrap();
        let out = fx
            .add(
                vec![Item {
                    page: 1,
                    rects: vec![vec![0.0, 0.0, 50.0, 10.0], vec![50.0, 0.0, 70.0, 10.0]],
                    text: "aa bb".into(),
                }],
                2,
            )
            .unwrap();
        assert_eq!(out[0].rects, vec![[0.0, 0.0, 50.0, 10.0], [50.0, 0.0, 70.0, 10.0]]);
    }

    #[test]
    fn boxes_that_only_touch_or_sit_on_other_pages_do_not_merge() {
        let fx = Fx::new();
        fx.add(vec![item(1, 0.0, 0.0, 50.0, 10.0, "aa")], 1).unwrap();
        fx.add(vec![item(1, 50.0, 0.0, 60.0, 10.0, "bb")], 2).unwrap();
        fx.add(vec![item(1, 0.0, 10.0, 50.0, 20.0, "cc")], 3).unwrap();
        let out = fx.add(vec![item(2, 0.0, 0.0, 50.0, 10.0, "dd")], 4).unwrap();
        assert_eq!(out.len(), 4);
    }

    #[test]
    fn two_overlapping_items_in_one_call_merge_with_each_other() {
        let fx = Fx::new();
        let out = fx
            .add(vec![item(1, 0.0, 0.0, 50.0, 10.0, "aa bb"), item(1, 40.0, 0.0, 90.0, 10.0, "bb cc")], 1)
            .unwrap();
        assert_eq!(texts(&out), vec!["aa bb cc"]);
    }

    #[test]
    fn a_merge_past_a_limit_is_refused_and_changes_nothing() {
        let fx = Fx::new();
        fx.add(vec![item(1, 0.0, 0.0, 50.0, 10.0, &"a ".repeat(2000))], 1).unwrap();
        let before = fx.get().highlights;
        let r = fx.add(vec![item(1, 40.0, 0.0, 90.0, 10.0, &"b ".repeat(2000))], 2);
        assert_eq!(r.unwrap_err(), "too_long");
        assert_eq!(fx.get().highlights, before);

        let id = before[0].id.clone();
        set_note(&fx.ctx(), &fx.books, &id, &"n".repeat(6000), t(3)).unwrap();
        fx.add(vec![item(1, 0.0, 50.0, 10.0, 60.0, "zz")], 4).unwrap();
        let first = fx.get().highlights.iter().find(|h| h.text == "zz").unwrap().id.clone();
        set_note(&fx.ctx(), &fx.books, &first, &"m".repeat(6000), t(5)).unwrap();
        let r = fx.add(vec![item(1, 0.0, 0.0, 10.0, 60.0, "a zz")], 6);
        assert_eq!(r.unwrap_err(), "too_long");
        assert_eq!(fx.get().highlights.len(), 2);
    }

    // ---- notes, delete, undo ----

    #[test]
    fn set_note_saves_stamps_and_skips_an_unchanged_note() {
        let fx = Fx::new();
        let id = fx.add(vec![item(1, 0.0, 0.0, 20.0, 10.0, "aa")], 1).unwrap()[0].id.clone();
        let out = set_note(&fx.ctx(), &fx.books, &id, "my note", t(5)).unwrap();
        assert_eq!(out[0].note, "my note");
        assert_eq!(out[0].updated_at, "2026-11-17T09:05:05.000Z");
        assert_eq!(out[0].created_at, "2026-11-17T09:05:01.000Z");
        let out = set_note(&fx.ctx(), &fx.books, &id, "my note", t(9)).unwrap();
        assert_eq!(out[0].updated_at, "2026-11-17T09:05:05.000Z");
        let out = set_note(&fx.ctx(), &fx.books, &id, "", t(10)).unwrap();
        assert_eq!(out[0].note, "");
    }

    #[test]
    fn set_note_errors() {
        let fx = Fx::new();
        let id = fx.add(vec![item(1, 0.0, 0.0, 20.0, 10.0, "aa")], 1).unwrap()[0].id.clone();
        assert_eq!(set_note(&fx.ctx(), &fx.books, "nope", "x", t(2)).unwrap_err(), "unknown_highlight");
        let long = "n".repeat(10_001);
        assert_eq!(set_note(&fx.ctx(), &fx.books, &id, &long, t(2)).unwrap_err(), "bad_highlight");
        let ok = "n".repeat(10_000);
        assert!(set_note(&fx.ctx(), &fx.books, &id, &ok, t(2)).is_ok());
    }

    #[test]
    fn delete_then_restore_brings_back_the_exact_record() {
        let fx = Fx::new();
        let id = fx.add(vec![item(1, 0.0, 0.0, 20.0, 10.0, "aa")], 1).unwrap()[0].id.clone();
        set_note(&fx.ctx(), &fx.books, &id, "keep me", t(2)).unwrap();
        let before = fx.get().highlights;
        let out = delete(&fx.ctx(), &fx.books, &id, t(3)).unwrap();
        assert!(out.is_empty());
        assert_eq!(fx.raw()["highlights"].as_array().unwrap().len(), 0);
        let out = restore(&fx.ctx(), &fx.books, t(4)).unwrap();
        assert_eq!(out, before);
        assert_eq!(restore(&fx.ctx(), &fx.books, t(5)).unwrap_err(), "nothing_to_restore");
    }

    #[test]
    fn only_the_latest_delete_can_be_undone() {
        let fx = Fx::new();
        let a = fx.add(vec![item(1, 0.0, 0.0, 20.0, 10.0, "aa")], 1).unwrap()[0].id.clone();
        let b = fx.add(vec![item(2, 0.0, 0.0, 20.0, 10.0, "bb")], 2).unwrap()[1].id.clone();
        delete(&fx.ctx(), &fx.books, &a, t(3)).unwrap();
        delete(&fx.ctx(), &fx.books, &b, t(4)).unwrap();
        let out = restore(&fx.ctx(), &fx.books, t(5)).unwrap();
        assert_eq!(texts(&out), vec!["bb"]);
        assert_eq!(restore(&fx.ctx(), &fx.books, t(6)).unwrap_err(), "nothing_to_restore");
    }

    #[test]
    fn delete_unknown_highlight_and_forget_undo() {
        let fx = Fx::new();
        let id = fx.add(vec![item(1, 0.0, 0.0, 20.0, 10.0, "aa")], 1).unwrap()[0].id.clone();
        assert_eq!(delete(&fx.ctx(), &fx.books, "nope", t(2)).unwrap_err(), "unknown_highlight");
        delete(&fx.ctx(), &fx.books, &id, t(3)).unwrap();
        fx.books.forget_undo(&fx.id);
        assert_eq!(restore(&fx.ctx(), &fx.books, t(4)).unwrap_err(), "nothing_to_restore");
        fx.books.forget_undo("never seen");
    }

    #[test]
    fn restore_merges_like_an_add_when_it_overlaps_a_newer_highlight() {
        let fx = Fx::new();
        let a = fx.add(vec![item(1, 0.0, 0.0, 50.0, 10.0, "w1 w2 w3")], 1).unwrap()[0].id.clone();
        set_note(&fx.ctx(), &fx.books, &a, "old note", t(2)).unwrap();
        let created = fx.get().highlights[0].created_at.clone();
        delete(&fx.ctx(), &fx.books, &a, t(3)).unwrap();
        fx.add(vec![item(1, 40.0, 0.0, 90.0, 10.0, "w3 w4 w5")], 4).unwrap();
        let out = restore(&fx.ctx(), &fx.books, t(5)).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, a);
        assert_eq!(out[0].created_at, created);
        assert_eq!(out[0].text, "w1 w2 w3 w4 w5");
        assert_eq!(out[0].note, "old note");
    }

    #[test]
    fn deleting_the_last_highlight_keeps_the_file_and_its_export_name() {
        let fx = Fx::new();
        let id = fx.add(vec![item(1, 0.0, 0.0, 20.0, 10.0, "aa")], 1).unwrap()[0].id.clone();
        set_export_name(&fx.ctx(), &fx.books, Some("Book.md"), t(2)).unwrap();
        delete(&fx.ctx(), &fx.books, &id, t(3)).unwrap();
        let v = fx.raw();
        assert_eq!(v["exportName"], "Book.md");
        assert_eq!(v["highlights"].as_array().unwrap().len(), 0);
    }

    // ---- load rules ----

    fn write_raw(fx: &Fx, body: &str) {
        std::fs::create_dir_all(highlights_dir(fx.d.path())).unwrap();
        std::fs::write(fx.ctx().file(), body).unwrap();
    }

    fn good_highlight() -> serde_json::Value {
        serde_json::json!({
            "id": "0123456789abcdef", "page": 1, "rects": [[0.0, 0.0, 1.0, 1.0]],
            "text": "x", "note": "", "createdAt": "2026-11-17T09:05:01.000Z",
            "updatedAt": "2026-11-17T09:05:01.000Z"
        })
    }

    fn file_json(mutate: impl FnOnce(&mut serde_json::Value)) -> String {
        let mut v = serde_json::json!({
            "schemaVersion": 1, "bookId": bid(), "exportName": null,
            "highlights": [good_highlight()]
        });
        mutate(&mut v);
        v.to_string()
    }

    #[test]
    fn a_good_file_loads() {
        let fx = Fx::new();
        write_raw(&fx, &file_json(|_| {}));
        let l = fx.get();
        assert_eq!(l.highlights.len(), 1);
        assert!(l.notice.is_none());
        assert!(fx.damaged_names().is_empty());
    }

    #[test]
    fn damaged_files_are_set_aside_with_a_utc_stamp_and_a_session_notice() {
        let cases: Vec<(&str, String)> = vec![
            ("not json", "{ nope".to_string()),
            ("wrong schema", file_json(|v| v["schemaVersion"] = 2.into())),
            ("wrong book id", file_json(|v| v["bookId"] = format!("{}-6", "c".repeat(64)).into())),
            ("bad id", file_json(|v| v["highlights"][0]["id"] = "XYZ".into())),
            ("uppercase id", file_json(|v| v["highlights"][0]["id"] = "0123456789ABCDEF".into())),
            ("page 0", file_json(|v| v["highlights"][0]["page"] = 0.into())),
            ("page past count", file_json(|v| v["highlights"][0]["page"] = 51.into())),
            ("no rects", file_json(|v| v["highlights"][0]["rects"] = serde_json::json!([]))),
            ("flat rect", file_json(|v| v["highlights"][0]["rects"] = serde_json::json!([[1.0, 0.0, 1.0, 1.0]]))),
            ("empty text", file_json(|v| v["highlights"][0]["text"] = "".into())),
            ("long note", file_json(|v| v["highlights"][0]["note"] = "n".repeat(10_001).into())),
            ("bad date", file_json(|v| v["highlights"][0]["createdAt"] = "yesterday".into())),
            ("missing field", file_json(|v| { v["highlights"][0].as_object_mut().unwrap().remove("text"); })),
            ("duplicate ids", file_json(|v| {
                let h = good_highlight();
                v["highlights"] = serde_json::json!([h.clone(), h]);
            })),
        ];
        for (name, body) in cases {
            let fx = Fx::new();
            write_raw(&fx, &body);
            let l = fx.get();
            assert!(l.highlights.is_empty(), "{name}");
            assert_eq!(l.notice.as_deref(), Some(DAMAGED_NOTICE), "{name}");
            assert_eq!(fx.damaged_names(), vec![format!("{}.corrupt-20261117-090559.json", bid())], "{name}");
            assert!(!fx.ctx().file().exists(), "{name}");
            // the notice stays for the session
            assert!(fx.get().notice.is_some(), "{name}");
        }
    }

    #[test]
    fn a_second_set_aside_in_the_same_second_gets_a_numbered_name() {
        let fx = Fx::new();
        for _ in 0..3 {
            write_raw(&fx, "{ nope");
            fx.get();
        }
        let b = bid();
        assert_eq!(
            fx.damaged_names(),
            vec![
                format!("{b}.corrupt-20261117-090559-2.json"),
                format!("{b}.corrupt-20261117-090559-3.json"),
                format!("{b}.corrupt-20261117-090559.json"),
            ]
        );
    }

    #[test]
    fn after_a_set_aside_the_book_starts_empty_and_saves_normally() {
        let fx = Fx::new();
        write_raw(&fx, "{ nope");
        fx.get();
        let out = fx.add(vec![item(1, 0.0, 0.0, 20.0, 10.0, "aa")], 1).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(fx.damaged_names().len(), 1);
    }

    #[test]
    fn a_leftover_tmp_is_removed_on_load_and_the_real_file_wins() {
        let fx = Fx::new();
        write_raw(&fx, &file_json(|_| {}));
        let tmp = tmp_path_of(&fx.ctx().file());
        std::fs::write(&tmp, b"half writ").unwrap();
        assert_eq!(fx.get().highlights.len(), 1);
        assert!(!tmp.exists());
    }

    #[test]
    fn a_file_for_a_book_not_in_the_library_is_listed_and_kept() {
        let fx = Fx::new();
        fx.add(vec![item(1, 0.0, 0.0, 20.0, 10.0, "aa")], 1).unwrap();
        std::fs::write(highlights_dir(fx.d.path()).join("junk.json"), b"{}").unwrap();
        assert_eq!(list_ids(fx.d.path()), vec![bid()]);
    }

    #[test]
    fn save_replaces_the_file_whole_through_the_book_tmp() {
        let fx = Fx::new();
        fx.add(vec![item(1, 0.0, 0.0, 20.0, 10.0, "aa")], 1).unwrap();
        fx.add(vec![item(2, 0.0, 0.0, 20.0, 10.0, "bb")], 2).unwrap();
        assert_eq!(fx.raw()["highlights"].as_array().unwrap().len(), 2);
        assert!(!tmp_path_of(&fx.ctx().file()).exists());
    }

    #[test]
    fn export_name_is_set_only_on_the_current_file_and_read_for_export() {
        let fx = Fx::new();
        assert!(read_for_export(&fx.ctx(), &fx.books, t(1)).is_none());
        set_export_name(&fx.ctx(), &fx.books, Some("X.md"), t(1)).unwrap();
        assert!(!fx.ctx().file().exists());
        fx.add(vec![item(1, 0.0, 0.0, 20.0, 10.0, "aa")], 2).unwrap();
        set_export_name(&fx.ctx(), &fx.books, Some("X.md"), t(3)).unwrap();
        let f = read_for_export(&fx.ctx(), &fx.books, t(4)).unwrap();
        assert_eq!(f.export_name.as_deref(), Some("X.md"));
        set_export_name(&fx.ctx(), &fx.books, None, t(5)).unwrap();
        assert!(read_for_export(&fx.ctx(), &fx.books, t(6)).unwrap().export_name.is_none());
    }

    #[test]
    fn the_book_mutex_serialises_parallel_adds() {
        let fx = Arc::new(Fx::new());
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let fx = fx.clone();
                std::thread::spawn(move || {
                    fx.add(vec![item(1 + i, 0.0, 0.0, 20.0, 10.0, &format!("t{i}"))], 1).unwrap();
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(fx.get().highlights.len(), 8);
    }
}
