# 0002. ReaderX v2: highlights and notes with Obsidian export

**Date**: 2026-11-17
**Status**: Proposed

## Summary

While reading, you select text and click a small Highlight button to mark it in one soft color, and you can add a plain text note to any highlight. A side panel in the reader lists the book's highlights in page order and jumps to each one. Every book's highlights are also kept up to date as one Markdown file per book in a folder you choose (your Obsidian vault). Highlights are stored beside the library, keyed by the book id, and are never written into the PDF, so renames, moves and reading progress all keep working.

## Requirements

**User stories**:
- As a reader, I want to mark passages while I read so that I can find them again later.
- As a reader, I want to add a short note to a passage so that my thought stays with the quote.
- As an Obsidian user, I want each book's highlights and notes to appear in my vault on their own so that my reading notes live with my other notes without copy and paste.

**Acceptance criteria**:
- **AC-1**: Selecting text in the reader shows a small Highlight button just below the end of the selection. Clicking it, or pressing `H` while text is selected, marks the selection in one soft color and clears the selection. A selection across a page break becomes one highlight per page, saved in one step. Pages with no text layer (scanned images) never show the button. Without a selection, `H` keeps its normal PDF.js meaning.
- **AC-2**: Highlights are drawn at the right place on their page at any zoom, on rotated pages, right after they are made, after closing and reopening the book, after an app restart, and in the Original, Paper and Night themes. Text under a highlight still meets the 4.5:1 contrast rule of spec 0001 AC-10 on a white page and in Night.
- **AC-3**: A new selection whose boxes intersect an existing highlight's boxes on the same page merges with it into one highlight: the quote is the text of the combined passage (no repeated words), the notes join oldest first with a blank line between them, and the oldest highlight's id and `createdAt` are kept. A merge that would pass the size limits is refused with the bar message `That highlight is too long.`
- **AC-4**: Clicking a highlight on the page shows a small menu with **Note** and **Delete**. **Note** opens the side panel, scrolls to that highlight and focuses its note field. A note saves by itself 1 second after typing stops, and also when the field loses focus, the panel closes, or you leave the book. There is no Save button.
- **AC-5**: **Delete** (from the page menu or from the panel item) removes the highlight at once and shows `Highlight deleted · Undo` for 6 seconds. **Undo** brings back the exact same highlight (id, quote, note, dates). Only the latest delete can be undone; leaving the book ends the chance to undo.
- **AC-6**: The reader bar has a `Highlights (N)` button that opens and closes a side panel laid over the right edge of the page (the page does not shrink or move). The panel lists the book's highlights in page order, each with its quote (cut to 3 lines), `p. N`, its own note field and a Delete button. Clicking the quote jumps the reader so the highlight sits a little below the top of the view. With no highlights it says `Select text to highlight it.`
- **AC-7**: Highlights belong to the book id. Renaming or moving the PDF keeps them, a missing PDF keeps them, and the PDF file is never written to (its bytes and its book id stay the same after highlighting).
- **AC-8**: The bottom of the panel shows `Export to: <folder>` (a long path is cut in the middle, the full path in a tooltip) with a **Change** button, or `Export to Obsidian or Markdown` with **Choose folder** when none is set. Until a folder is chosen, nothing is written outside the app's own data. Choosing or changing the folder writes the file of every book that has highlights, in the background.
- **AC-9**: After every highlight change (add, merge, note, delete, undo), that book's Markdown file in the export folder is up to date within 2 seconds. Opening a book whose last export failed tries again.
- **AC-10**: The export file starts with properties `title`, `pages`, `progress` (same % as the shelf), `last-read` (local date), `readerx` (the book id) and `tags: [book, readerx]`, then a block between `<!-- readerx:start -->` and `<!-- readerx:end -->` holding a `## Highlights` heading and, per highlight in page order, the quote as a `>` blockquote ending with `(p. N)`, followed by its note as plain paragraphs when it has one. With no highlights left, the block holds the heading and `_No highlights yet._`
- **AC-11**: Your own edits survive every export: text outside the marked block is kept, and properties you added are kept (ReaderX only rewrites its own keys, and writes `tags` only when the file has no `tags` yet). If the markers were removed or broken, a fresh block is added at the end of the file. A file with the same name (ignoring case) whose `readerx` property is not this book's id is never overwritten; the book uses `<Title> (2).md` (then 3, 4, ...) instead. If you delete or rename the exported file, the next export creates it again. Export files are never deleted by ReaderX.
- **AC-12**: When a book's export cannot be written (folder missing, drive unplugged, file locked, no permission), the panel shows a quiet line `Export failed: <reason in words>` under the folder for that book. Highlights are still saved, the next change or the next open of that book tries again, and the line disappears after that book's next successful export.
- **AC-13**: `Escape` closes the highlight menu or the Highlight button first, then the side panel (saving a note being typed), then goes back to the shelf. This works whether focus is in the page or in the panel. The reader lockdown of spec 0001 AC-17 still holds.
- **AC-14**: A crash or power cut never leaves a highlights file half written. A highlights file that can't be read or fails any check is renamed `<bookId>.corrupt-YYYYMMDD-HHMMSS.json` (UTC, with `-2`, `-3` added if that name exists), the book starts with no highlights, and the panel says `Highlights for this book were damaged and set aside.` until it is closed.

## Decision

**Chosen option**: our own thin highlight layer over PDF.js, with highlights in one JSON file per book in the app data folder, and a Rust side Markdown export, run in the background, that rewrites only its own marked block and properties.

**Implementation skills**: none (Agent Skills for Tauri and PDF.js were declined, see `AGENTS.md`).

**Exception to the monochrome rule**: the highlight tint becomes the one interface color besides covers. `AGENTS.md` and `ui/AGENTS.md` should say so once this ships (a `/sync` job, not this spec).

## Rationale

Reasoning and options: see [rationale.md](rationale.md).

## Feature design

**Data model sketch**:

| Entity | Field | Type | Rule |
|---|---|---|---|
| Settings (in `library.json`) | `exportFolder` | string, nullable | new; null means no export |
| Settings (in `library.json`) | `exportErrors` | map bookId → code | new; a book's entry is set when its export fails and removed when it succeeds |
| HighlightFile (`highlights\<bookId>.json` in `%APPDATA%\com.readerx.desktop`) | `schemaVersion` | int | 1; any other value counts as damaged |
| | `bookId` | string | equals the file name, checked with `is_valid_id` |
| | `exportName` | string, nullable | the `.md` file name in the export folder, set at the first export and set again when the folder changes or the file is found to belong to another book |
| | `highlights` | Highlight[] | kept sorted by (`page`, top of first rect descending in PDF units, left of first rect) |
| Highlight | `id` | string | 16 lowercase hex characters, random, made in Rust, unique within the book |
| | `page` | int | 1 or more, and at most `book.pageCount` when it is known |
| | `rects` | `[[x1, y1, x2, y2], ...]` | PDF page units with `x1 < x2` and `y1 < y2`, 1 to 200 boxes, all numbers finite |
| | `text` | string | the quote after cleanup, 1 to 5,000 characters |
| | `note` | string | 0 to 10,000 characters; empty means no note |
| | `createdAt`, `updatedAt` | ISO 8601 UTC | set in Rust |

Relationships: one book has 0..N highlights (by book id in the file name) and at most one export file. No highlights file is created for a book until its first highlight; the `highlights\` folder is created when first needed. A highlights file whose book is no longer in the library is kept and never deleted.

**State transitions**: a highlight is created (add), changed (merge or note), or deleted. Rust keeps the last deleted highlight of each book in memory (one slot per book, replaced by the next delete, cleared when the reader leaves that book); `restore_highlight` puts that exact record back without merging, as long as it does not overlap a newer highlight (if it does, it merges like an add).

**Reader mechanism** (all in `ui/`, using only public PDF.js events):
1. Selection: on `selectionchange` and `mouseup` inside the viewer, take `getSelection()`. Show the Highlight button only when the selection is not empty and every text node it touches sits inside a `.textLayer`. The button lives inside the viewer document, placed just below the end of the selection; its `mousedown` calls `preventDefault` so the selection stays. It hides when the selection collapses, on scroll, on zoom and on theme switch.
2. Turning a selection into highlights: for each `.page[data-page-number]` the selection touches, clone the range clipped to that page's `.textLayer`, take `getClientRects()` and keep only boxes of text nodes (drop element boxes), merge boxes on the same line (same top and bottom within 2 px, gaps under 4 px), convert each corner with that page view's `viewport.convertToPdfPoint` and normalize to min and max. Take the text per page from the clipped range and clean it: line breaks, tabs and no break spaces become single spaces, soft hyphens are removed, runs of spaces collapse, ends are trimmed. Pages that are not loaded are skipped. Send all pages in one `add_highlights` call, then clear the selection.
3. Merged quote: when the new boxes intersect existing highlights on the same page, the screen sends the quote of the combined passage. It extends the selection range to cover the existing highlights' first and last boxes using `caretRangeFromPoint`, then reads and cleans it as in step 2. If that fails, Rust falls back to joining the quotes in reading order and dropping words repeated where they meet.
4. Drawing: each page div gets a `div.rx-hl-layer` placed above the canvas and below the text layer, with one box per rect. The positions are percentages of the page size, worked out with `viewport.convertToViewportRectangle` and normalized, so zoom keeps them in place without a redraw. The layer has `pointer-events: none`, so text selection and PDF links keep working. The layer is redrawn from the current highlight list after every fetch and every change, and again on `pagerendered`.
5. Color: the palette gains `--highlight`. On Original and Paper it is a warm yellow at about 35% opacity with `mix-blend-mode: multiply`. On Night it is a muted amber at about 30% opacity, no blend. The text over it must keep 4.5:1, checked on a white page and on the Night page color.
6. Clicking a highlight: on `click` inside a page with an empty selection, convert the click point to PDF units and test it against that page's rects. A click on a link annotation is left to PDF.js. A hit opens the small menu (Note, Delete) at the click point; it closes on scroll, Escape or a click elsewhere. The panel is the keyboard path to Note and Delete.
7. Jumping from the panel: `pdfViewer.scrollPageIntoView({ pageNumber, destArray: [null, { name: "XYZ" }, x1, y2 + 36, null] })` using the first rect (36 PDF units, about half an inch, of room above).
8. The panel and the Undo bar live in the parent page, laid over the right side of the reader, so they follow the palette, are not affected by the viewer's CSS, and survive a theme reload of the iframe. Panel width is 360 px; its open or closed state is kept for the session only.
9. Keys: the viewer key listener (capture phase, as in v1) handles `H` when the selection is not empty (`preventDefault` and `stopPropagation`, so PDF.js does not switch to the hand tool), and Escape in the order of AC-13. A matching Escape listener on the parent document covers focus inside the panel. Escape inside a note field saves the note, then closes the panel.
10. Ordering: every highlight call (add, note, delete, restore) goes through the reader session's ordered send queue from v1, and the queue is flushed on Back, window close, theme switch and Delete, so a note save never overtakes an add or lands after a delete. A note field is limited to 10,000 characters with `maxlength`.

**API surface** (Tauri commands, logic in `src-tauri/src/highlights.rs` and `export.rs`, thin wrappers in `lib.rs`, each added to `capabilities/default.json`):

| Command | Inputs | Output | Key errors |
|---|---|---|---|
| `get_highlights` | `id` (req) | `{ highlights, exportFolder, exportError, notice }`; retries a failed export of this book in the background | `unknown_book` |
| `add_highlights` | `id`; `items: [{ page, rects, text }]` (1 or more, req) | `{ highlights }` (after merge) | `unknown_book`, `bad_highlight`, `too_long` |
| `set_note` | `id`, `highlightId`, `note` (all req) | `{ highlights }` | `unknown_book`, `unknown_highlight`, `bad_highlight` |
| `delete_highlight` | `id`, `highlightId` (req) | `{ highlights }` | `unknown_book`, `unknown_highlight` |
| `restore_highlight` | `id` (req) | `{ highlights }` | `unknown_book`, `nothing_to_restore` |
| `forget_undo` | `id` (req) | nothing (called when the reader leaves a book) | none |
| `set_export_folder` | `path` (req) | `{ exportFolder }`; starts a background export of every book with highlights | `not_a_folder` |

`get_highlights` works for any book id with a valid highlights file, so export never needs the book to be present. Every change command saves the highlights file first and returns; the export runs afterwards on a background worker, and the result reaches the screen as a `highlights-exported` event `{ id, exportError }`. `exportError` is `null`, `folder_missing`, `locked` or `write_failed`; the screen maps them to `folder not found`, `the file is open in another program` and `could not write the file`.

**Rust state and the export worker**: highlights files are read from disk on each command, under one mutex per book id (separate from the library `Store` lock), so two calls for the same book never lose an update. Vault writes never hold the `Store` lock or a book lock. The worker takes book ids from a queue, merges repeated requests for the same book that arrive within 300 ms, renders the file, and skips the write when the result equals what is on disk. It records each book's result in `settings.exportErrors`.

**Value sourcing**:

| Action | Value | Source |
|---|---|---|
| add_highlights | `page`, `rects`, `text` | the selection in the viewer (mechanism steps 1 and 2) |
| add_highlights | `id`, `createdAt`, `updatedAt` | made in Rust (`id` from 8 random bytes; on the rare clash, draw again) |
| merge | `text` | the combined passage from the screen (mechanism step 3), else the Rust fallback |
| merge | `rects` | the union of all boxes, with exact duplicates removed |
| merge | `id`, `createdAt` | the oldest of the merged highlights; `updatedAt` is now |
| merge | `note` | non empty notes joined oldest first with a blank line |
| panel | `p. N` | `highlight.page` |
| panel | `Highlights (N)` | `highlights.length` |
| export | file name | `exportName`; when unset: `displayTitle` with control characters and `\ / : * ? " < > |` removed, trailing dots and spaces removed, trimmed, cut to 100 characters; `Untitled` when empty; `_` added in front of a Windows reserved name (`CON`, `PRN`, `AUX`, `NUL`, `COM1` to `COM9`, `LPT1` to `LPT9`); then ` (2)`, ` (3)`, ... after the cut, if a file with that name (ignoring case) has a `readerx` other than this id or cannot be read; then `.md` |
| export | `title` | `displayTitle` (spec 0001 rule), always in double quotes |
| export | `pages` | `book.pageCount` (left out while null; added once known) |
| export | `progress` | `round(book.progress * 100)` + `%`, the same rounding as `ui/lib/format.js` |
| export | `last-read` | `book.lastOpenedAt` as a local date `YYYY-MM-DD` (Windows local time zone); left as it is in the file when the book was never opened |
| export | `readerx` | the book id, in double quotes |
| panel | export folder line | `settings.exportFolder` and `settings.exportErrors[bookId]`, updated by `highlights-exported` |

**Export file rules** (in Rust, all unit tested):
- Reading: the file is read just before it is rewritten, so the newest edits from Obsidian are kept. A leading byte order mark is kept, and the file's own line ending (CRLF or LF, CRLF for a new file) is used for every line written. The file always ends with one line ending. If Obsidian changes the file between the read and the write, that edit can be lost; this window is accepted.
- Properties: the file has properties only if its first line is `---`, a later line is `---`, and every line between is empty, a comment, a `key: value` line, or an indented or `- ` line that continues the line above. Replace the lines of the keys `title`, `pages`, `progress`, `last-read` and `readerx` together with their continuation lines, append the missing ones, and add `tags: [book, readerx]` only when no line starts with `tags:`. Every other line stays as it was. Without valid properties, a new properties section is put at the top and the old first lines stay below it.
- Block: the block is the text from the first `<!-- readerx:start -->` to the first `<!-- readerx:end -->` after it, and it is replaced as a whole. If there is no such pair, a fresh block is added at the end after one blank line, and any lone marker stays where it is.
- Quotes and notes: every line of a quote starts with `> `, and the last line ends with ` (p. N)`. A note is written as plain paragraphs (its own line breaks kept) after a blank line. Any `<!--` inside a quote or note is written as `<\!--`, so user text can never end the block. One blank line separates highlights.
- New file: properties, a blank line, `# <title>`, a blank line, then the block.
- Writing: the file is written to `.readerx-<bookId>.tmp` in the export folder (a dot name that Obsidian ignores), flushed, then renamed over the target, using the `library.rs` atomic write after it is changed to take a target path. A leftover `.readerx-*.tmp` is removed before each write.
- Identity: before writing, if the file named `exportName` is missing, it is created again; if it exists with another `readerx` id, a new name is chosen and `exportName` is updated. Changing the export folder clears every `exportName`, so names are chosen again in the new folder. Old files are never touched.

**Highlights file load rules**: the file is set aside as damaged (AC-14) if the JSON can't be read, `schemaVersion` is not 1, `bookId` differs from the file name, or any highlight fails a field rule. A leftover `<bookId>.json.tmp` is removed on load. The damaged notice is kept for the session until the panel is closed.

**Key invariants**:
- The PDF file is opened read only, always. Book ids never change because of highlights.
- Highlights on the same page never overlap after any command.
- Nothing is written outside the app data folder except `<exportFolder>\<exportName>` and its temp file, and only after the folder was chosen.
- Every `bookId` is checked with `is_valid_id` before it is used in a path.
- A highlight change is saved before any export is tried, and a slow or broken export folder never blocks the reader.

**Security model**: single user, local files only, no network (spec 0001). The export folder is chosen by the user through the system folder picker (`dialog:allow-open` is already granted); the asset protocol scope does not change, because only Rust writes the export. File names are cleaned so nothing can escape the export folder.

**Configuration required**: none.

**Critical test scenarios**:
- Happy path: highlight two passages on different pages, add a note, close and reopen the app; both are drawn in place and listed in order, and `Book.md` in the chosen folder holds both quotes and the note. Verifies **AC-1**, **AC-2**, **AC-6**, **AC-9**, **AC-10**.
- Cross page: select from the end of page 3 to the start of page 4; two highlights appear and one export runs. Verifies **AC-1**.
- Rotated page: highlight on a rotated page and change zoom; the boxes stay on the text. Verifies **AC-2**.
- Merge: highlight words 1 to 5, then 3 to 9 on the same line; one highlight remains with the quote words 1 to 9 once each. Verifies **AC-3**.
- Your edits: add a line above the block and a `rating: 5` property in the exported file, then add a highlight; both survive. Delete the end marker; a fresh block is added at the end. A file saved with CRLF and a byte order mark keeps both. Verifies **AC-11**.
- Name clash and rename: two books both titled `Notes`; the second exports to `Notes (2).md` and the first file is untouched. Rename `Notes.md` in Obsidian, add a highlight; `Notes.md` is created again. Verifies **AC-11**.
- Failure: rename the export folder away and add a highlight; the panel shows `Export failed: folder not found`, the highlight is kept, Back is not delayed, and the file appears after the folder returns and the book is opened again. Verifies **AC-9**, **AC-12**.
- Undo: delete a highlight with a note, click Undo; it returns with the note and the same id and `createdAt`. Verifies **AC-5**.
- Keys: with text selected, `H` highlights; without, `H` switches PDF.js to the hand tool; Escape in a note field saves and closes the panel. Verifies **AC-1**, **AC-13**.
- File safety: sha256 of the PDF is the same before and after adding highlights; a highlights file with a wrong `bookId` is set aside with the panel message. Verifies **AC-7**, **AC-14**.
- Unit tests (`cargo test`): merge rule (intersection, kept id and dates, notes, limits), sorting, validation and load rules, file name cleaning, reserved names and clash numbering, property merge (replace with continuation lines, append, keep user keys, tags only when absent, quoting), marker pair rules, marker escaping in notes, line endings and byte order mark, skip unchanged write, atomic save and corrupt file handling.

## Build plan

Tracer Bullet: a thin highlight path end to end first, then the panel, editing, and export.

1. Rust `highlights.rs`: HighlightFile load rules and atomic save (shared atomic write now takes a target path), corrupt file set aside, per book lock, `add_highlights` (no merge yet) and `get_highlights` with validation, unit tests; wrappers and capabilities. Satisfies **AC-7**, **AC-14**.
2. Reader: selection detection, the Highlight button and `H`, per page boxes and text cleanup, `add_highlights`, and the drawing layer with percentage boxes and redraw after changes. **The thin path works end to end here.** Satisfies **AC-1**, **AC-2**.
3. Merge rule in Rust (box intersection, combined quote from the screen with the Rust fallback, kept id and dates, joined notes, limits), unit tests; the screen sends the combined quote. Satisfies **AC-3**.
4. Side panel laid over the reader: `Highlights (N)` button, list in page order, jump on click, empty state. Satisfies **AC-6**.
5. Click menu on a highlight, per item note field with `set_note` through the send queue, `delete_highlight`, the Rust undo slot with `restore_highlight` and `forget_undo`, and the Undo bar. Satisfies **AC-4**, **AC-5**.
6. Rust `export.rs`: Markdown rendering, property merge, marker pair rules and escaping, line endings, file name cleaning and clash numbering, identity checks, atomic write with the hidden temp file, unit tests. Satisfies **AC-10**, **AC-11**.
7. Export worker and wiring: `exportFolder` and `exportErrors` settings, `set_export_folder`, the background queue with merging and unchanged skip, the `highlights-exported` event, retry on open, the panel folder row and the `Export failed` line. Satisfies **AC-8**, **AC-9**, **AC-12**.
8. Keys and look: `H` and `Escape` in both documents, the `--highlight` color per theme with a contrast check, and a lockdown check. Satisfies **AC-1**, **AC-2**, **AC-13**.
9. Check every criterion in the real app with the test books (including a rotated page and a scanned page) and a test vault folder. Satisfies **AC-1** to **AC-14**.

## Consequences

**Positive**:
- Highlights reach Obsidian with no manual step, and edits made there are kept.
- Book identity, progress and the PDF files are untouched; v1 behavior does not change.
- The layer uses only public PDF.js events, so it survives a PDF.js upgrade better than the built in editor's internals would.
- A slow or missing vault folder never slows the reader, because export runs in the background.

**Negative / tradeoffs**:
- Highlights are not visible in other PDF readers, because they are not in the PDF.
- Scanned books without a text layer can't be highlighted in v2.
- We own the selection to box math (line merging, rotated pages), and it needs real app testing on odd PDFs.
- `progress` and `last-read` in the vault file only refresh when that book's highlights change or the book is opened, so they can lag behind the shelf.
- An edit made in Obsidian in the instant between ReaderX reading and writing the file can be lost.
- The interface gains a second color (the highlight tint), a deliberate exception to the monochrome rule.

**Neutral**:
- A new folder `highlights\` and two new settings appear in the app data; old v1 data needs no migration (`exportFolder` defaults to null, `exportErrors` to empty).

## Follow-up

- [ ] Update `AGENTS.md` and `ui/AGENTS.md` for the highlight color exception and the new commands (`/sync` after the build).
- [ ] Draw boxes on scanned pages (needs an area highlight), if missed in use.
- [ ] An all books highlights view from the shelf, if the per book panel is not enough.
- [ ] More highlight colors with tags, if one color proves too limiting.
