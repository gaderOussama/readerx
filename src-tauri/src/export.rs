//! The Markdown export of a book's highlights into the export folder (an
//! Obsidian vault). Rendering and merging are pure functions of the text; the
//! rest is a background worker that never holds the `Store` lock or a book
//! lock while it touches the vault.

use crate::derive::display_title;
use crate::highlights::{self, Books, Ctx, Highlight};
use crate::library::{atomic_write, lock, Book, Store};
use chrono::{DateTime, Local, TimeZone, Utc};
use std::collections::HashMap;
use std::fmt::Display;
use std::io;
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const START: &str = "<!-- readerx:start -->";
const END: &str = "<!-- readerx:end -->";
/// The keys ReaderX rewrites, in the order they are written.
const MANAGED: [&str; 5] = ["title", "pages", "progress", "last-read", "readerx"];
const COALESCE: Duration = Duration::from_millis(300);
const MAX_NAME_CHARS: usize = 100;
const RESERVED: [&str; 4] = ["CON", "PRN", "AUX", "NUL"];

/// What the properties and the heading are made from.
pub struct BookInfo {
    pub id: String,
    pub title: String,
    pub pages: Option<u32>,
    /// `round(progress * 100)`, the same rounding as `ui/lib/format.js`.
    pub progress: i64,
    pub last_read: Option<String>,
}

impl BookInfo {
    pub fn of(book: &Book) -> Self {
        Self {
            id: book.id.clone(),
            title: display_title(book.meta_title.as_deref(), &book.file_name),
            pages: book.page_count,
            progress: (book.progress * 100.0).round() as i64,
            last_read: book.last_opened_at.as_deref().and_then(local_date),
        }
    }
}

fn date_in<Tz: TimeZone>(iso: &str, tz: &Tz) -> Option<String>
where
    Tz::Offset: Display,
{
    let d = DateTime::parse_from_rfc3339(iso).ok()?;
    Some(d.with_timezone(tz).format("%Y-%m-%d").to_string())
}

/// `YYYY-MM-DD` in the machine's local time zone.
fn local_date(iso: &str) -> Option<String> {
    date_in(iso, &Local)
}

// ---- rendering ----

fn yaml_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `(key, full line)` for each managed key; None keeps the file's own line.
fn managed(info: &BookInfo) -> [(&'static str, Option<String>); 5] {
    [
        ("title", Some(format!("title: {}", yaml_quote(&info.title)))),
        ("pages", info.pages.map(|n| format!("pages: {n}"))),
        ("progress", Some(format!("progress: {}%", info.progress))),
        ("last-read", info.last_read.as_ref().map(|d| format!("last-read: {d}"))),
        ("readerx", Some(format!("readerx: {}", yaml_quote(&info.id)))),
    ]
}

fn is_continuation(l: &str) -> bool {
    l.starts_with([' ', '\t']) || l == "-" || l.starts_with("- ")
}

/// The key of a `key: value` line at column 0.
fn key_of(l: &str) -> Option<&str> {
    let c = l.chars().next()?;
    if c.is_whitespace() || c == '#' || is_continuation(l) {
        return None;
    }
    l.char_indices()
        .filter(|(_, c)| *c == ':')
        .map(|(i, _)| i)
        .find(|&i| l[i + 1..].is_empty() || l[i + 1..].starts_with(char::is_whitespace))
        .filter(|&i| i > 0)
        .map(|i| l[..i].trim_end())
}

/// Index of the closing `---` when the lines start with a valid properties section.
fn find_props(lines: &[&str]) -> Option<usize> {
    if lines.first().map(|l| l.trim_end()) != Some("---") {
        return None;
    }
    let end = 1 + lines[1..].iter().position(|l| l.trim_end() == "---")?;
    let valid = lines[1..end].iter().all(|l| {
        l.trim().is_empty()
            || l.trim_start().starts_with('#')
            || is_continuation(l)
            || key_of(l).is_some()
    });
    valid.then_some(end)
}

/// The lines between the `---` lines: managed keys replaced (with their
/// continuation lines), missing ones appended, `tags` added only when absent.
fn merge_props(body: &[&str], info: &BookInfo) -> Vec<String> {
    let new = managed(info);
    let mut seen = [false; 5];
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < body.len() {
        let slot = key_of(body[i]).and_then(|k| MANAGED.iter().position(|m| *m == k));
        let Some(m) = slot else {
            out.push(body[i].to_string());
            i += 1;
            continue;
        };
        let mut j = i + 1;
        while j < body.len() && is_continuation(body[j]) {
            j += 1;
        }
        if !seen[m] {
            seen[m] = true;
            match &new[m].1 {
                Some(line) => out.push(line.clone()),
                None => out.extend(body[i..j].iter().map(|s| s.to_string())),
            }
        }
        i = j;
    }
    for (m, (_, line)) in new.iter().enumerate() {
        if let (false, Some(line)) = (seen[m], line) {
            out.push(line.clone());
        }
    }
    if !body.iter().any(|l| l.starts_with("tags:")) {
        out.push("tags: [book, readerx]".to_string());
    }
    out
}

fn lf(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

/// User text can never contain a comment opener that could end our block.
fn escape(s: &str) -> String {
    lf(s).replace("<!--", "<\\!--")
}

fn block_lines(hl: &[Highlight]) -> Vec<String> {
    let mut out = vec![START.to_string(), "## Highlights".to_string(), String::new()];
    if hl.is_empty() {
        out.push("_No highlights yet._".to_string());
    }
    for (i, h) in hl.iter().enumerate() {
        if i > 0 {
            out.push(String::new());
        }
        let quote = escape(&h.text);
        let n = quote.split('\n').count();
        for (k, line) in quote.split('\n').enumerate() {
            let tail = if k + 1 == n { format!(" (p. {})", h.page) } else { String::new() };
            out.push(format!("> {line}{tail}"));
        }
        let note = escape(h.note.trim());
        if !note.is_empty() {
            out.push(String::new());
            out.extend(note.split('\n').map(str::to_string));
        }
    }
    out.push(END.to_string());
    out
}

/// From the first end marker that has a start before it, back to the nearest
/// start. A lone start or a lone end marker is left alone.
fn find_block(t: &str) -> Option<(usize, usize)> {
    let first_start = t.find(START)?;
    let end = first_start + START.len() + t[first_start + START.len()..].find(END)?;
    let start = t[..end].rfind(START)?;
    Some((start, end + END.len()))
}

fn put_block(t: &str, block: &str) -> String {
    match find_block(t) {
        Some((a, b)) => format!("{}{}{}", &t[..a], block, &t[b..]),
        None => format!("{}\n\n{}", t.trim_end_matches('\n'), block),
    }
}

fn finish(t: &str, bom: bool, crlf: bool) -> String {
    let mut s = format!("{}\n", t.trim_end_matches('\n'));
    if crlf {
        s = s.replace('\n', "\r\n");
    }
    if bom {
        s.insert(0, '\u{feff}');
    }
    s
}

/// The new text of the export file from the text now on disk (None for a new
/// file). Keeps the file's byte order mark and line ending.
pub fn render(existing: Option<&str>, info: &BookInfo, hl: &[Highlight]) -> String {
    let block = block_lines(hl).join("\n");
    let front = |body: &[&str]| -> Vec<String> {
        let mut v = vec!["---".to_string()];
        v.extend(merge_props(body, info));
        v.push("---".to_string());
        v
    };
    let Some(raw) = existing else {
        let mut lines = front(&[]);
        let heading: String = info.title.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
        lines.extend(["".to_string(), format!("# {heading}"), String::new()]);
        lines.push(block);
        return finish(&lines.join("\n"), false, true);
    };
    let (bom, rest) = match raw.strip_prefix('\u{feff}') {
        Some(r) => (true, r),
        None => (false, raw),
    };
    let crlf = rest.find('\n').is_none_or(|i| rest[..i].ends_with('\r'));
    let text = lf(rest);
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = match find_props(&lines) {
        Some(end) => {
            let mut v = front(&lines[1..end]);
            v.extend(lines[end + 1..].iter().map(|s| s.to_string()));
            v
        }
        None => {
            let mut v = front(&[]);
            if !text.trim().is_empty() {
                v.push(String::new());
                v.extend(lines.iter().map(|s| s.to_string()));
            }
            v
        }
    };
    let joined = out.join("\n");
    out.clear();
    finish(&put_block(&joined, &block), bom, crlf)
}

/// The `readerx` property of a file's text, if it has valid properties.
pub fn readerx_of(text: &str) -> Option<String> {
    let text = lf(text.strip_prefix('\u{feff}').unwrap_or(text));
    let lines: Vec<&str> = text.split('\n').collect();
    let end = find_props(&lines)?;
    let l = lines[1..end].iter().find(|l| key_of(l) == Some("readerx"))?;
    let v = l[l.find(':')? + 1..].trim();
    Some(v.trim_matches(['"', '\'']).to_string())
}

// ---- file names ----

pub fn clean_name(title: &str) -> String {
    let stripped: String = title
        .chars()
        .filter(|c| !c.is_control() && !"\\/:*?\"<>|".contains(*c))
        .collect();
    let trim = |s: &str| s.trim_end_matches(['.', ' ']).trim_start().to_string();
    let cut: String = trim(&stripped).chars().take(MAX_NAME_CHARS).collect();
    let name = trim(&cut);
    if name.is_empty() {
        return "Untitled".to_string();
    }
    let stem = name.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    let numbered = |p: &str| {
        stem.strip_prefix(p).is_some_and(|d| d.len() == 1 && matches!(d.as_bytes()[0], b'1'..=b'9'))
    };
    if RESERVED.contains(&stem.as_str()) || numbered("COM") || numbered("LPT") {
        format!("_{name}")
    } else {
        name
    }
}

/// A name read from the highlights file is used in a path, so it is checked.
fn safe_name(n: &str) -> bool {
    n.len() > 3
        && n.ends_with(".md")
        && !n.chars().any(|c| c.is_control() || "\\/:*?\"<>|".contains(c))
}

pub enum Probe {
    Missing,
    /// A file with this book's `readerx` id, and its text.
    Ours(String),
    Other,
    Unreadable(&'static str),
}

/// `<Title>.md`, then `<Title> (2).md`, ... until a name is free or ours.
fn pick_name(title: &str, probe: &mut dyn FnMut(&str) -> Probe) -> (String, Option<String>) {
    let base = clean_name(title);
    for n in 1.. {
        let name = if n == 1 { format!("{base}.md") } else { format!("{base} ({n}).md") };
        match probe(&name) {
            Probe::Missing => return (name, None),
            Probe::Ours(text) => return (name, Some(text)),
            Probe::Other | Probe::Unreadable(_) => {}
        }
    }
    unreachable!("the range is unbounded")
}

// ---- vault I/O ----

/// Error code for the screen: `folder_missing`, `locked` or `write_failed`.
fn classify(e: &io::Error, target_exists: bool) -> &'static str {
    match e.raw_os_error() {
        Some(32 | 33) => return "locked",
        _ => {}
    }
    match e.kind() {
        io::ErrorKind::NotFound => "folder_missing",
        io::ErrorKind::PermissionDenied if target_exists => "locked",
        _ => "write_failed",
    }
}

fn probe_fs(folder: &Path, id: &str, name: &str) -> Probe {
    match std::fs::read(folder.join(name)) {
        Ok(b) => match String::from_utf8(b) {
            Ok(s) if readerx_of(&s).as_deref() == Some(id) => Probe::Ours(s),
            _ => Probe::Other,
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => Probe::Missing,
        Err(e) => Probe::Unreadable(classify(&e, true)),
    }
}

fn remove_stale_tmps(folder: &Path) {
    let Ok(rd) = std::fs::read_dir(folder) else { return };
    for e in rd.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(".readerx-") && name.ends_with(".tmp") {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Writes (or skips, when nothing changed) the book's file in `folder` and
/// returns the file name used. Never touches the store or a book lock.
pub fn export_file(
    folder: &Path,
    info: &BookInfo,
    saved_name: Option<&str>,
    hl: &[Highlight],
) -> Result<String, &'static str> {
    if !folder.is_dir() {
        return Err("folder_missing");
    }
    remove_stale_tmps(folder);
    let mut probe = |name: &str| probe_fs(folder, &info.id, name);
    let (name, existing) = match saved_name.filter(|n| safe_name(n)) {
        Some(n) => match probe(n) {
            Probe::Missing => (n.to_string(), None),
            Probe::Ours(text) => (n.to_string(), Some(text)),
            Probe::Unreadable(code) => return Err(code),
            Probe::Other => pick_name(&info.title, &mut probe),
        },
        None => pick_name(&info.title, &mut probe),
    };
    let out = render(existing.as_deref(), info, hl);
    if existing.as_deref() != Some(out.as_str()) {
        let target = folder.join(&name);
        let tmp = folder.join(format!(".readerx-{}.tmp", info.id));
        atomic_write(
            &target,
            &tmp,
            out.as_bytes(),
            &mut |a, b| std::fs::rename(a, b),
            5,
            Duration::from_millis(50),
        )
        .map_err(|e| classify(&e, target.exists()))?;
    }
    Ok(name)
}

/// One export of one book. None: nothing to do (no folder, unknown book, no
/// or damaged highlights file). Some(None): written or already current.
/// Some(Some(code)): failed; the result is also kept in `settings.exportErrors`.
pub fn export_book(
    shared: &Mutex<Store>,
    books: &Books,
    id: &str,
    now: DateTime<Utc>,
) -> Option<Option<String>> {
    let (dir, folder, book) = {
        let s = lock(shared);
        (s.dir.clone(), s.lib.settings.export_folder.clone()?, s.lib.books.get(id)?.clone())
    };
    let ctx = Ctx::new(&dir, id, book.page_count).ok()?;
    let file = highlights::read_for_export(&ctx, books, now)?;
    let info = BookInfo::of(&book);
    let result = export_file(Path::new(&folder), &info, file.export_name.as_deref(), &file.highlights);
    let code = match &result {
        Ok(name) => {
            if file.export_name.as_deref() != Some(name) {
                let _ = highlights::set_export_name(&ctx, books, Some(name), now);
            }
            None
        }
        Err(code) => Some(code.to_string()),
    };
    let mut s = lock(shared);
    let errors = &mut s.lib.settings.export_errors;
    let changed = match &code {
        None => errors.remove(id).is_some(),
        Some(c) => errors.insert(id.to_string(), c.clone()).as_ref() != Some(c),
    };
    if changed {
        s.persist();
    }
    Some(code)
}

/// Sets the folder, forgets every file name (names are chosen again in the new
/// folder) and returns the ids of the books that have a highlights file, for
/// the caller to queue.
pub fn set_folder(
    shared: &Mutex<Store>,
    books: &Books,
    path: &str,
    now: DateTime<Utc>,
) -> Result<Vec<String>, String> {
    if !Path::new(path).is_dir() {
        return Err("not_a_folder".to_string());
    }
    let (dir, pages): (_, HashMap<String, Option<u32>>) = {
        let mut s = lock(shared);
        s.lib.settings.export_folder = Some(path.to_string());
        s.lib.settings.export_errors.clear();
        s.persist();
        (s.dir.clone(), s.lib.books.iter().map(|(k, b)| (k.clone(), b.page_count)).collect())
    };
    let ids = highlights::list_ids(&dir);
    for id in &ids {
        let pc = pages.get(id).copied().flatten();
        if let Ok(ctx) = Ctx::new(&dir, id, pc) {
            let _ = highlights::set_export_name(&ctx, books, None, now);
        }
    }
    Ok(ids)
}

// ---- the background queue ----

/// Book ids waiting for export, each due 300 ms after its first request, so a
/// burst of changes to one book is one export.
#[derive(Default)]
pub struct Pending(HashMap<String, Instant>);

impl Pending {
    pub fn add(&mut self, id: &str, now: Instant) {
        self.0.entry(id.to_string()).or_insert(now + COALESCE);
    }

    /// How long until the earliest is due; None when nothing waits.
    pub fn wait(&self, now: Instant) -> Option<Duration> {
        self.0.values().min().map(|due| due.saturating_duration_since(now))
    }

    pub fn take_due(&mut self, now: Instant) -> Vec<String> {
        let due: Vec<String> =
            self.0.iter().filter(|(_, d)| **d <= now).map(|(id, _)| id.clone()).collect();
        for id in &due {
            self.0.remove(id);
        }
        due
    }
}

/// Starts the worker thread; `job` runs once per due book id, one at a time.
pub fn spawn(mut job: impl FnMut(&str) + Send + 'static) -> Sender<String> {
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut pending = Pending::default();
        loop {
            let got = match pending.wait(Instant::now()) {
                None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
                Some(d) => rx.recv_timeout(d),
            };
            match got {
                Ok(id) => pending.add(&id, Instant::now()),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            for id in pending.take_due(Instant::now()) {
                job(&id);
            }
        }
    });
    tx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Format;
    use chrono::{FixedOffset, TimeZone};
    use std::sync::Arc;

    fn id() -> String {
        format!("{}-5", "c".repeat(64))
    }

    fn cid() -> &'static str {
        static ID: std::sync::LazyLock<String> = std::sync::LazyLock::new(id);
        &ID
    }

    fn info() -> BookInfo {
        BookInfo {
            id: id(),
            title: "My Book".into(),
            pages: Some(10),
            progress: 20,
            last_read: Some("2026-11-17".into()),
        }
    }

    fn hl(page: u32, text: &str, note: &str) -> Highlight {
        Highlight {
            id: "0123456789abcdef".into(),
            page,
            rects: vec![[0.0, 0.0, 1.0, 1.0]],
            text: text.into(),
            note: note.into(),
            created_at: "2026-11-17T09:00:00.000Z".into(),
            updated_at: "2026-11-17T09:00:00.000Z".into(),
        }
    }

    fn crlf(s: &str) -> String {
        s.replace('\n', "\r\n")
    }

    const PROPS: &str = "---\ntitle: \"My Book\"\npages: 10\nprogress: 20%\nlast-read: 2026-11-17\nreaderx: \"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc-5\"\ntags: [book, readerx]\n---";

    // ---- rendering ----

    #[test]
    fn a_new_file_has_properties_a_heading_and_the_block_with_crlf() {
        let out = render(None, &info(), &[hl(3, "a quote", "my note")]);
        let want = format!(
            "{PROPS}\n\n# My Book\n\n<!-- readerx:start -->\n## Highlights\n\n> a quote (p. 3)\n\nmy note\n<!-- readerx:end -->\n"
        );
        assert_eq!(out, crlf(&want));
    }

    #[test]
    fn quotes_get_a_marker_on_every_line_and_the_page_on_the_last() {
        let out = render(None, &info(), &[hl(2, "line one\nline two", ""), hl(7, "other", "")]);
        assert!(out.contains("> line one\r\n> line two (p. 2)\r\n\r\n> other (p. 7)\r\n"));
    }

    #[test]
    fn notes_are_paragraphs_with_their_line_breaks_kept() {
        let out = render(None, &info(), &[hl(1, "q", "first para\n\nsecond\nsame para\r\nwin")]);
        assert!(out.contains("> q (p. 1)\r\n\r\nfirst para\r\n\r\nsecond\r\nsame para\r\nwin\r\n<!-- readerx:end -->"));
    }

    #[test]
    fn no_highlights_says_so() {
        let out = render(None, &info(), &[]);
        assert!(out.contains("## Highlights\r\n\r\n_No highlights yet._\r\n<!-- readerx:end -->\r\n"));
    }

    #[test]
    fn comment_openers_in_user_text_are_escaped() {
        let out = render(None, &info(), &[hl(1, "a <!-- readerx:end --> b", "n <!-- x")]);
        assert!(out.contains("> a <\\!-- readerx:end --> b (p. 1)"));
        assert!(out.contains("n <\\!-- x"));
        assert_eq!(out.matches("<!--").count(), 2);
        // the rewritten file still has exactly one block
        let again = render(Some(&out), &info(), &[hl(1, "a <!-- readerx:end --> b", "n <!-- x")]);
        assert_eq!(again, out);
    }

    #[test]
    fn rendering_twice_gives_the_same_text() {
        let list = [hl(1, "q", "n")];
        let once = render(None, &info(), &list);
        assert_eq!(render(Some(&once), &info(), &list), once);
        let with_bom_lf = format!("\u{feff}Hello\n\n{}", once.replace("\r\n", "\n"));
        let a = render(Some(&with_bom_lf), &info(), &list);
        assert_eq!(render(Some(&a), &info(), &list), a);
    }

    #[test]
    fn the_title_is_always_quoted_and_escaped() {
        let mut i = info();
        i.title = "He said \"hi\" \\ there\nline".into();
        let out = render(None, &i, &[]);
        assert!(out.contains("title: \"He said \\\"hi\\\" \\\\ there\\nline\"\r\n"));
        assert!(out.contains("# He said \"hi\" \\ there line\r\n"));
    }

    #[test]
    fn progress_rounds_like_javascript() {
        let mut b = book();
        b.progress = 0.125;
        assert_eq!(BookInfo::of(&b).progress, 13);
        b.progress = 0.2;
        assert_eq!(BookInfo::of(&b).progress, 20);
        b.progress = 1.0;
        assert_eq!(BookInfo::of(&b).progress, 100);
        b.progress = 0.0;
        assert_eq!(BookInfo::of(&b).progress, 0);
    }

    #[test]
    fn text_outside_the_block_and_your_properties_survive() {
        let existing = format!(
            "---\nrating: 5\ntitle: old\nreaderx: \"{}\"\naliases:\n  - a\n---\n\nmy intro\n\n<!-- readerx:start -->\nold stuff\n<!-- readerx:end -->\n\nmy outro\n",
            id()
        );
        let out = render(Some(&existing), &info(), &[hl(1, "q", "")]);
        assert_eq!(
            out,
            "---\nrating: 5\ntitle: \"My Book\"\nreaderx: \"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc-5\"\naliases:\n  - a\npages: 10\nprogress: 20%\nlast-read: 2026-11-17\ntags: [book, readerx]\n---\n\nmy intro\n\n<!-- readerx:start -->\n## Highlights\n\n> q (p. 1)\n<!-- readerx:end -->\n\nmy outro\n"
        );
    }

    #[test]
    fn a_managed_key_is_replaced_together_with_its_continuation_lines() {
        let existing = "---\ntitle:\n  - x\n  - y\nkeep: 1\nprogress: 5%\n# comment\n---\n";
        let out = render(Some(existing), &info(), &[]);
        assert!(out.starts_with(
            "---\ntitle: \"My Book\"\nkeep: 1\nprogress: 20%\n# comment\npages: 10\nlast-read: 2026-11-17\nreaderx:"
        ));
        assert!(!out.contains("- x"));
    }

    #[test]
    fn tags_are_written_only_when_the_file_has_none() {
        let with = "---\ntags:\n  - mine\n---\n";
        let out = render(Some(with), &info(), &[]);
        assert!(out.contains("tags:\n  - mine\n"));
        assert!(!out.contains("[book, readerx]"));
        let without = render(Some("---\nrating: 1\n---\n"), &info(), &[]);
        assert!(without.contains("tags: [book, readerx]"));
    }

    #[test]
    fn pages_and_last_read_stay_as_they_are_when_unknown() {
        let mut i = info();
        i.pages = None;
        i.last_read = None;
        let existing = "---\npages: 99\nlast-read: 2020-01-01\n---\n";
        let out = render(Some(existing), &i, &[]);
        assert!(out.contains("pages: 99\nlast-read: 2020-01-01\n"));
        let fresh = render(None, &i, &[]);
        assert!(!fresh.contains("pages:") && !fresh.contains("last-read:"));
    }

    #[test]
    fn without_valid_properties_a_new_section_goes_on_top_and_old_lines_stay_below() {
        let out = render(Some("Just my notes\nsecond line\n"), &info(), &[]);
        let lf_out = out.replace("\r\n", "\n");
        assert!(lf_out.starts_with(&format!("{PROPS}\n\nJust my notes\nsecond line\n\n<!-- readerx:start -->\n")));
        // unterminated or odd properties count as none
        for bad in ["---\nnot a property\n---\nbody\n", "---\nrating: 1\nbody\n"] {
            let o = render(Some(bad), &info(), &[]).replace("\r\n", "\n");
            assert!(o.starts_with(&format!("{PROPS}\n\n---\n")), "{bad}");
            assert!(o.contains("body\n"), "{bad}");
        }
    }

    #[test]
    fn an_empty_file_gets_properties_and_a_block() {
        let out = render(Some(""), &info(), &[]).replace("\r\n", "\n");
        assert_eq!(out, format!("{PROPS}\n\n<!-- readerx:start -->\n## Highlights\n\n_No highlights yet._\n<!-- readerx:end -->\n"));
    }

    #[test]
    fn missing_markers_get_a_fresh_block_at_the_end_after_one_blank_line() {
        let existing = format!("{PROPS}\nBody text\n\n\n");
        let out = render(Some(&existing), &info(), &[]);
        assert!(out.ends_with("Body text\n\n<!-- readerx:start -->\n## Highlights\n\n_No highlights yet._\n<!-- readerx:end -->\n"));
    }

    #[test]
    fn a_lone_marker_stays_and_the_fresh_block_is_found_again_next_time() {
        let user = "my text between";
        let existing = format!("{PROPS}\n\n<!-- readerx:start -->\n{user}\n");
        let first = render(Some(&existing), &info(), &[hl(1, "q", "")]);
        assert!(first.contains(user));
        assert_eq!(first.matches(START).count(), 2);
        let second = render(Some(&first), &info(), &[hl(1, "q2", "")]);
        assert!(second.contains(user), "text after a lone start marker must survive");
        assert!(second.contains("q2") && !second.contains("> q (p. 1)"));
        assert_eq!(second.matches(START).count(), 2);

        let lone_end = format!("{PROPS}\n\ntext\n{END}\nmore\n");
        let out = render(Some(&lone_end), &info(), &[]);
        assert_eq!(out.matches(END).count(), 2);
        assert!(out.contains("text\n<!-- readerx:end -->\nmore\n\n<!-- readerx:start -->"));
    }

    #[test]
    fn only_the_first_pair_is_replaced() {
        let existing = format!("{PROPS}\n\n{START}\na\n{END}\nmid\n{START}\nb\n{END}\n");
        let out = render(Some(&existing), &info(), &[]);
        assert!(out.contains("mid\n<!-- readerx:start -->\nb\n"));
        assert!(!out.contains("\na\n"));
    }

    #[test]
    fn byte_order_mark_and_line_endings_are_kept() {
        let lf_text = format!("{PROPS}\nx\n{START}\n{END}\n");
        let crlf_text = format!("\u{feff}{}", crlf(&lf_text));
        let out = render(Some(&crlf_text), &info(), &[hl(1, "q", "")]);
        assert!(out.starts_with('\u{feff}'));
        assert_eq!(out.matches("\r\n").count(), out.matches('\n').count());
        let out = render(Some(&lf_text), &info(), &[hl(1, "q", "")]);
        assert!(!out.starts_with('\u{feff}'));
        assert!(!out.contains('\r'));
    }

    #[test]
    fn the_file_always_ends_with_exactly_one_line_ending() {
        for tail in ["", "\n", "\n\n\n"] {
            let existing = format!("{PROPS}\nx\n{START}\n{END}{tail}");
            let out = render(Some(&existing), &info(), &[]);
            assert!(out.ends_with(&format!("{END}\n")) && !out.ends_with("\n\n"), "{tail:?}");
        }
        let out = render(None, &info(), &[]);
        assert!(out.ends_with("-->\r\n") && !out.ends_with("\r\n\r\n"));
    }

    #[test]
    fn readerx_is_read_from_valid_properties_only() {
        let text = render(None, &info(), &[]);
        assert_eq!(readerx_of(&text), Some(id()));
        assert_eq!(readerx_of(&format!("\u{feff}{text}")), Some(id()));
        assert_eq!(readerx_of("---\nreaderx: 'abc'\n---\n"), Some("abc".into()));
        assert_eq!(readerx_of("readerx: \"abc\"\n"), None);
        assert_eq!(readerx_of("---\nother: 1\n---\n"), None);
    }

    #[test]
    fn local_date_uses_the_given_zone() {
        let plus2 = FixedOffset::east_opt(2 * 3600).unwrap();
        assert_eq!(date_in("2026-11-17T23:30:00.000Z", &plus2).as_deref(), Some("2026-11-18"));
        assert_eq!(date_in("2026-11-17T23:30:00.000Z", &Utc).as_deref(), Some("2026-11-17"));
        assert_eq!(date_in("nope", &Utc), None);
        let l = local_date("2026-11-17T12:00:00.000Z").unwrap();
        assert!(l == "2026-11-17" || l == "2026-11-18" || l == "2026-11-16");
    }

    // ---- file names ----

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("A/B\\C:D*E?F\"G<H>I|J"), "ABCDEFGHIJ");
        assert_eq!(clean_name("Tab\there\u{7}bell"), "Tabherebell");
        assert_eq!(clean_name("Title...  "), "Title");
        assert_eq!(clean_name("  Lead  "), "Lead");
        assert_eq!(clean_name("Dr. Who"), "Dr. Who");
        assert_eq!(clean_name(""), "Untitled");
        assert_eq!(clean_name("?*:"), "Untitled");
        assert_eq!(clean_name(" ..  "), "Untitled");
        assert_eq!(clean_name("Ünïcode ✓"), "Ünïcode ✓");
    }

    #[test]
    fn names_are_cut_to_100_characters() {
        let long = "é".repeat(150);
        assert_eq!(clean_name(&long).chars().count(), 100);
        let spaced = format!("{} tail", "a".repeat(99));
        assert_eq!(clean_name(&spaced), "a".repeat(99));
    }

    #[test]
    fn windows_reserved_names_get_an_underscore() {
        for n in ["CON", "con", "Prn", "AUX", "NUL", "COM1", "com9", "LPT1", "lpt9", "CON.txt"] {
            assert_eq!(clean_name(n), format!("_{n}"), "{n}");
        }
        for n in ["COM0", "COM10", "CONSOLE", "LPT", "Nul Book"] {
            assert_eq!(clean_name(n), n, "{n}");
        }
    }

    #[test]
    fn clashes_are_numbered_after_the_cut_and_our_own_file_is_reused() {
        let mut taken = vec!["Notes.md", "Notes (2).md"];
        let mut p = |n: &str| if taken.contains(&n) { Probe::Other } else { Probe::Missing };
        assert_eq!(pick_name("Notes", &mut p), ("Notes (3).md".to_string(), None));
        taken.clear();
        let mut p = |n: &str| if n == "Notes.md" { Probe::Ours("x".into()) } else { Probe::Missing };
        assert_eq!(pick_name("Notes", &mut p), ("Notes.md".to_string(), Some("x".to_string())));
        let long = "x".repeat(120);
        let mut p = |n: &str| if n == format!("{}.md", "x".repeat(100)) { Probe::Unreadable("locked") } else { Probe::Missing };
        assert_eq!(pick_name(&long, &mut p).0, format!("{} (2).md", "x".repeat(100)));
    }

    #[test]
    fn saved_names_are_checked_before_use() {
        assert!(safe_name("Book.md"));
        assert!(safe_name("Book (2).md"));
        for bad in ["", ".md", "a.txt", "../x.md", "a/b.md", "a\\b.md", "a:b.md", "a\n.md", "C:x.md"] {
            assert!(!safe_name(bad), "{bad}");
        }
    }

    #[test]
    fn write_errors_map_to_codes() {
        let e = |k| io::Error::from(k);
        assert_eq!(classify(&e(io::ErrorKind::NotFound), true), "folder_missing");
        assert_eq!(classify(&io::Error::from_raw_os_error(32), false), "locked");
        assert_eq!(classify(&io::Error::from_raw_os_error(33), false), "locked");
        assert_eq!(classify(&e(io::ErrorKind::PermissionDenied), true), "locked");
        assert_eq!(classify(&e(io::ErrorKind::PermissionDenied), false), "write_failed");
        assert_eq!(classify(&e(io::ErrorKind::Other), true), "write_failed");
    }

    // ---- writing into a folder ----

    fn vault() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn read(d: &Path, name: &str) -> String {
        std::fs::read_to_string(d.join(name)).unwrap()
    }

    #[test]
    fn export_file_creates_the_file_and_leaves_no_tmp() {
        let v = vault();
        let name = export_file(v.path(), &info(), None, &[hl(1, "q", "")]).unwrap();
        assert_eq!(name, "My Book.md");
        assert!(read(v.path(), &name).contains("> q (p. 1)"));
        let tmps: Vec<_> = std::fs::read_dir(v.path()).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).collect();
        assert!(tmps.is_empty());
    }

    #[test]
    fn a_stale_hidden_tmp_is_removed_before_each_write() {
        let v = vault();
        std::fs::write(v.path().join(".readerx-old.tmp"), "x").unwrap();
        std::fs::write(v.path().join(".readerx-keep.txt"), "x").unwrap();
        std::fs::write(v.path().join("mine.tmp"), "x").unwrap();
        export_file(v.path(), &info(), None, &[]).unwrap();
        assert!(!v.path().join(".readerx-old.tmp").exists());
        assert!(v.path().join(".readerx-keep.txt").exists());
        assert!(v.path().join("mine.tmp").exists());
    }

    #[test]
    fn a_missing_folder_is_folder_missing() {
        let v = vault();
        let gone = v.path().join("gone");
        assert_eq!(export_file(&gone, &info(), None, &[]), Err("folder_missing"));
    }

    #[test]
    fn another_books_file_with_the_same_name_is_never_overwritten() {
        let v = vault();
        let other = "---\nreaderx: \"other-1\"\n---\nTheirs\n";
        std::fs::write(v.path().join("My Book.md"), other).unwrap();
        let name = export_file(v.path(), &info(), None, &[hl(1, "q", "")]).unwrap();
        assert_eq!(name, "My Book (2).md");
        assert_eq!(read(v.path(), "My Book.md"), other);
        let again = export_file(v.path(), &info(), Some(&name), &[hl(1, "q", "")]).unwrap();
        assert_eq!(again, "My Book (2).md");
    }

    #[test]
    fn a_note_the_user_already_had_is_not_overwritten_either() {
        let v = vault();
        std::fs::write(v.path().join("My Book.md"), "no properties at all\n").unwrap();
        let name = export_file(v.path(), &info(), None, &[]).unwrap();
        assert_eq!(name, "My Book (2).md");
        assert_eq!(read(v.path(), "My Book.md"), "no properties at all\n");
    }

    #[cfg(windows)]
    #[test]
    fn the_clash_check_ignores_case() {
        let v = vault();
        std::fs::write(v.path().join("NOTES.md"), "---\nreaderx: \"other-1\"\n---\n").unwrap();
        let mut i = info();
        i.title = "notes".into();
        assert_eq!(export_file(v.path(), &i, None, &[]).unwrap(), "notes (2).md");
    }

    #[test]
    fn a_deleted_or_renamed_file_is_created_again() {
        let v = vault();
        let name = export_file(v.path(), &info(), None, &[hl(1, "q", "")]).unwrap();
        std::fs::rename(v.path().join(&name), v.path().join("Renamed.md")).unwrap();
        let again = export_file(v.path(), &info(), Some(&name), &[hl(1, "q", "")]).unwrap();
        assert_eq!(again, name);
        assert!(v.path().join(&name).exists());
        assert!(v.path().join("Renamed.md").exists());
    }

    #[test]
    fn a_saved_name_that_now_belongs_to_another_book_picks_a_new_name() {
        let v = vault();
        std::fs::write(v.path().join("Saved.md"), "---\nreaderx: \"other-1\"\n---\n").unwrap();
        let name = export_file(v.path(), &info(), Some("Saved.md"), &[]).unwrap();
        assert_eq!(name, "My Book.md");
    }

    #[test]
    fn an_unsafe_saved_name_is_ignored() {
        let v = vault();
        let name = export_file(v.path(), &info(), Some("..\\evil.md"), &[]).unwrap();
        assert_eq!(name, "My Book.md");
        assert!(!v.path().parent().unwrap().join("evil.md").exists());
    }

    #[test]
    fn your_edits_made_in_the_file_survive_the_next_export() {
        let v = vault();
        let name = export_file(v.path(), &info(), None, &[hl(1, "q", "")]).unwrap();
        let edited = read(v.path(), &name).replace("tags:", "rating: 5\ntags:").replace("# My Book", "# My Book\n\nMy thoughts");
        std::fs::write(v.path().join(&name), &edited).unwrap();
        export_file(v.path(), &info(), Some(&name), &[hl(1, "q", ""), hl(2, "q2", "")]).unwrap();
        let now = read(v.path(), &name);
        assert!(now.contains("rating: 5") && now.contains("My thoughts") && now.contains("> q2 (p. 2)"));
    }

    #[test]
    fn an_unchanged_file_is_not_written_again() {
        let v = vault();
        let name = export_file(v.path(), &info(), None, &[hl(1, "q", "")]).unwrap();
        let path = v.path().join(&name);
        let mut perm = std::fs::metadata(&path).unwrap().permissions();
        perm.set_readonly(true);
        std::fs::set_permissions(&path, perm.clone()).unwrap();
        // a write over a read-only file fails, so success proves there was none
        assert_eq!(export_file(v.path(), &info(), Some(&name), &[hl(1, "q", "")]), Ok(name.clone()));
        assert!(export_file(v.path(), &info(), Some(&name), &[hl(1, "changed", "")]).is_err());
        perm.set_readonly(false);
        std::fs::set_permissions(&path, perm).unwrap();
    }

    // ---- the whole export of one book ----

    fn book() -> Book {
        Book {
            id: id(),
            path: "C:\\b\\my-book.pdf".into(),
            file_name: "my-book.pdf".into(),
            meta_title: Some("My Book".into()),
            format: Format::Pdf,
            size_bytes: 5,
            mtime_ms: 1,
            page_count: Some(10),
            added_at: "2026-11-01T00:00:00.000Z".into(),
            last_opened_at: Some("2026-11-17T12:00:00.000Z".into()),
            position: None,
            progress: 0.2,
            finished_at: None,
        }
    }

    fn t0() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 17, 9, 0, 0).unwrap()
    }

    fn app() -> (tempfile::TempDir, Mutex<Store>, Books) {
        let d = tempfile::tempdir().unwrap();
        let mut s = Store::open(d.path(), t0());
        s.lib.books.insert(id(), book());
        (d, Mutex::new(s), Books::default())
    }

    fn add_one(d: &Path, books: &Books, text: &str, page: i64) {
        let ctx = Ctx::new(d, cid(), Some(10)).unwrap();
        let item = highlights::Item { page, rects: vec![vec![0.0, 0.0, 5.0, 5.0]], text: text.into() };
        highlights::add(&ctx, books, vec![item], t0()).unwrap();
    }

    #[test]
    fn nothing_is_written_until_a_folder_is_chosen() {
        let (d, shared, books) = app();
        add_one(d.path(), &books, "q", 1);
        assert_eq!(export_book(&shared, &books, &id(), t0()), None);
    }

    #[test]
    fn nothing_happens_for_unknown_books_and_books_without_highlights() {
        let (d, shared, books) = app();
        let v = vault();
        set_folder(&shared, &books, v.path().to_str().unwrap(), t0()).unwrap();
        assert_eq!(export_book(&shared, &books, &id(), t0()), None);
        assert_eq!(export_book(&shared, &books, &format!("{}-9", "d".repeat(64)), t0()), None);
        drop(d);
    }

    #[test]
    fn export_book_writes_remembers_the_name_and_clears_errors() {
        let (d, shared, books) = app();
        add_one(d.path(), &books, "q", 3);
        let v = vault();
        let folder = v.path().join("vault");
        std::fs::create_dir(&folder).unwrap();
        set_folder(&shared, &books, folder.to_str().unwrap(), t0()).unwrap();
        std::fs::remove_dir(&folder).unwrap();

        assert_eq!(export_book(&shared, &books, &id(), t0()), Some(Some("folder_missing".into())));
        assert_eq!(lock(&shared).lib.settings.export_errors.get(&id()).map(String::as_str), Some("folder_missing"));

        std::fs::create_dir(&folder).unwrap();
        assert_eq!(export_book(&shared, &books, &id(), t0()), Some(None));
        assert!(lock(&shared).lib.settings.export_errors.is_empty());
        let text = read(&folder, "My Book.md");
        assert!(text.contains("> q (p. 3)"));
        assert!(text.contains("last-read:") && text.contains("progress: 20%"));
        let ctx = Ctx::new(d.path(), cid(), Some(10)).unwrap();
        let f = highlights::read_for_export(&ctx, &books, t0()).unwrap();
        assert_eq!(f.export_name.as_deref(), Some("My Book.md"));
        // the error state reached library.json
        let (lib, _) = crate::library::load(d.path(), t0());
        assert!(lib.settings.export_errors.is_empty());
    }

    #[test]
    fn a_failed_export_keeps_the_highlights() {
        let (d, shared, books) = app();
        add_one(d.path(), &books, "q", 3);
        let v = vault();
        set_folder(&shared, &books, v.path().to_str().unwrap(), t0()).unwrap();
        drop(v);
        assert_eq!(export_book(&shared, &books, &id(), t0()), Some(Some("folder_missing".into())));
        let ctx = Ctx::new(d.path(), cid(), Some(10)).unwrap();
        assert_eq!(highlights::get(&ctx, &books, t0()).highlights.len(), 1);
    }

    #[test]
    fn a_damaged_highlights_file_is_not_exported() {
        let (d, shared, books) = app();
        add_one(d.path(), &books, "q", 3);
        let v = vault();
        set_folder(&shared, &books, v.path().to_str().unwrap(), t0()).unwrap();
        export_book(&shared, &books, &id(), t0());
        let ctx = Ctx::new(d.path(), cid(), Some(10)).unwrap();
        std::fs::write(ctx.file(), "{ nope").unwrap();
        assert_eq!(export_book(&shared, &books, &id(), t0()), None);
        assert!(read(v.path(), "My Book.md").contains("> q (p. 3)"));
    }

    #[test]
    fn choosing_a_folder_checks_it_clears_names_and_lists_books_to_export() {
        let (d, shared, books) = app();
        add_one(d.path(), &books, "q", 3);
        assert_eq!(set_folder(&shared, &books, "Z:\\definitely\\not\\here", t0()), Err("not_a_folder".into()));
        assert!(lock(&shared).lib.settings.export_folder.is_none());

        let a = vault();
        let b = vault();
        assert_eq!(set_folder(&shared, &books, a.path().to_str().unwrap(), t0()).unwrap(), vec![id()]);
        export_book(&shared, &books, &id(), t0());
        let ctx = Ctx::new(d.path(), cid(), Some(10)).unwrap();
        assert!(highlights::read_for_export(&ctx, &books, t0()).unwrap().export_name.is_some());

        lock(&shared).lib.settings.export_errors.insert(id(), "locked".into());
        set_folder(&shared, &books, b.path().to_str().unwrap(), t0()).unwrap();
        assert!(highlights::read_for_export(&ctx, &books, t0()).unwrap().export_name.is_none());
        assert!(lock(&shared).lib.settings.export_errors.is_empty());
        let (lib, _) = crate::library::load(d.path(), t0());
        assert_eq!(lib.settings.export_folder.as_deref(), b.path().to_str());

        export_book(&shared, &books, &id(), t0());
        assert!(b.path().join("My Book.md").exists());
        // the old folder's file is left alone
        assert!(a.path().join("My Book.md").exists());
    }

    // ---- the queue ----

    #[test]
    fn pending_merges_requests_within_300_ms_of_the_first() {
        let mut p = Pending::default();
        let t = Instant::now();
        assert_eq!(p.wait(t), None);
        p.add("a", t);
        p.add("a", t + Duration::from_millis(200));
        p.add("b", t + Duration::from_millis(250));
        assert_eq!(p.wait(t), Some(Duration::from_millis(300)));
        assert!(p.take_due(t + Duration::from_millis(299)).is_empty());
        assert_eq!(p.take_due(t + Duration::from_millis(300)), vec!["a".to_string()]);
        assert_eq!(p.wait(t + Duration::from_millis(300)), Some(Duration::from_millis(250)));
        assert_eq!(p.take_due(t + Duration::from_millis(600)), vec!["b".to_string()]);
        assert_eq!(p.wait(t), None);
        p.add("a", t + Duration::from_millis(700));
        assert_eq!(p.wait(t + Duration::from_millis(700)), Some(COALESCE));
    }

    #[test]
    fn the_worker_runs_one_job_per_burst_per_book() {
        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let c = calls.clone();
        let tx = spawn(move |id| c.lock().unwrap().push(id.to_string()));
        for _ in 0..3 {
            tx.send("a".into()).unwrap();
        }
        tx.send("b".into()).unwrap();
        std::thread::sleep(Duration::from_millis(250));
        assert!(calls.lock().unwrap().is_empty());
        std::thread::sleep(Duration::from_millis(900));
        let mut got = calls.lock().unwrap().clone();
        got.sort();
        assert_eq!(got, vec!["a".to_string(), "b".to_string()]);
    }
}
