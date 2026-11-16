# 0001. ReaderX v1: a bookshelf that reopens every book where you left it

**Date**: 2026-11-16
**Status**: Proposed

## Summary

ReaderX v1 is a small Windows app. It watches one books folder and shows every PDF in it on a shelf, with a big Continue card for the book you are reading now. One click opens a book in the PDF.js viewer (the reader built into Firefox) at the exact page, scroll spot and zoom you left. The app is built with Tauri 2: Rust does the file work and saving, and a plain HTML/CSS screen does the display. The look is quiet and monochrome, with Paper and Night page themes for long reading.

Reasoning and options: see [rationale.md](rationale.md).

## Requirements

**User stories**:
- As a reader, I want to open the app and continue my current book with one click, so I never hunt for the file or the page again.
- As a reader, I want to see all my books with their progress, so I know what I am reading, what is waiting, and what I finished.
- As a reader, I want pages that are easy on the eyes, so long sessions don't tire me.

**Acceptance criteria** (the contract):
- **AC-1**: On first launch, with no folder chosen, the app shows a welcome screen with a Choose folder button. After you choose a folder, every PDF in it and in all its subfolders (any depth, `.pdf` in any letter case) appears on the shelf.
- **AC-2**: Clicking a book opens it in the PDF.js viewer. The position (page, scroll offset inside the page, zoom) is saved no later than 2 seconds after the last page, scroll or zoom change. Going back to the shelf or closing the window saves it at once. Nothing is saved before the viewer has finished jumping to the saved position.
- **AC-3**: Reopening a book, from the shelf or after a restart of the app or the PC, shows the same page, scroll offset and zoom it had when you left it.
- **AC-4**: The shelf opens with a Continue card for the most recently opened book that is present and not finished. One click opens it at its saved position. When no book qualifies, the card is hidden.
- **AC-5**: Below the card are three sections. **Reading** lists started, unfinished books, newest last read first, without the Continue book. **To read** lists books never opened, by title A to Z (letter case ignored). **Finished** lists finished books, newest finished first. A section with no books is hidden.
- **AC-6**: Each card shows the cover (page 1, in color), the title, and for started books `p. X / Y · Z%` plus the last read date. The date counts calendar days in local time and reads `Today`, `Yesterday`, `N days ago` (2 to 6 days), else a date like `3 Nov 2026`. Until the cover is ready, a monochrome tile with the title is shown instead.
- **AC-7**: Reaching the end of a book while reading marks it Finished. The end means the last page becomes the current page, or the view is scrolled to the very bottom. Each card has a small menu with Mark as finished / Mark as unfinished. Marking a book unfinished keeps its position. Reopening it at the end does not mark it finished again: only arriving at the end from a place before it does.
- **AC-8**: Renaming a PDF, or moving it to another place inside the books folder, keeps its progress.
- **AC-9**: While the app is open, a PDF added to the folder appears on the shelf within 2 seconds after its copy has finished. A removed one disappears within 2 seconds. A removed book's progress is kept, and if the same file comes back later, its progress returns with it. When the app was closed during the change, the shelf is correct at the next launch.
- **AC-10**: The reader has a page theme toggle: Original, Paper, Night. Paper and Night recolor the pages, and the whole app follows the chosen theme, the PDF.js viewer toolbar included. The choice is remembered across restarts. The interface is monochrome (grays only, no accent color) except the covers, and all text has a contrast ratio of at least 4.5:1.
- **AC-11**: A PDF that can't be read (corrupt, or locked with a password) still appears on the shelf with a `Can't preview` tile, and it never blocks or crashes the shelf or other books. Opening a password PDF shows the PDF.js password prompt; opening a corrupt one shows the PDF.js error message, with the Back button working.
- **AC-12**: A crash or power cut never leaves `library.json` half written or empty. If it is unreadable at startup, it is renamed to `library.corrupt-YYYYMMDD-HHMMSS.json` (UTC time), the app starts with an empty library (folder not chosen), and a banner you can dismiss says what happened.
- **AC-13**: Only one ReaderX window exists. Launching it again brings the open window to the front, even when it was minimized. Window size and position are remembered, and the window never flashes white on open.
- **AC-14**: A Windows installer installs ReaderX with a Start menu shortcut and an optional desktop shortcut. With 200 unchanged books whose covers are already made, the shelf is visible in under 2 seconds after launch.
- **AC-15**: If the books folder is missing (deleted, renamed, or a drive unplugged), at launch or while the app is open, the app shows `Books folder not found` with a Choose folder button. No progress is deleted.
- **AC-16**: The shelf header has a Change folder button. A chosen folder that holds no PDFs shows `No PDFs found in <folder>` instead of a blank shelf.
- **AC-17**: Inside the reader you can only read the current book: the viewer's Open file, Print, Download and Bookmark buttons are hidden, dropping a file on it does nothing, and web links in a PDF open in your normal browser.

## Decision

**Chosen option**: Option 2: Tauri 2 with the PDF.js viewer.

Build ReaderX as a Tauri 2 app. A Rust core owns the library file, folder scan, fingerprints and folder watching. A plain HTML/CSS/JS screen (no framework, no bundler) shows the shelf and embeds the prebuilt PDF.js viewer for reading. All data lives in one JSON file.

Choices made inside this decision (pick, why, runner up):
- **Reader**: the prebuilt PDF.js viewer (`web/viewer.html` from the official `pdfjs-dist` release), copied into `ui/vendor/pdfjs/` at one pinned version and shown in an iframe below our own slim bar. It brings zoom, search, outline, thumbnails, keyboard shortcuts and text selection for free. Pin a version whose `web/app_options.js` has `forcePageColors`. Runner up: a custom reader on the PDF.js core library (more control, far more code).
- **Book identity**: SHA-256 of the first 64 KiB of the file plus its size in bytes, written as `<64 hex chars>-<size>`. It survives renames and moves and works for EPUB later. Each book also caches the `path`, `sizeBytes` and `mtimeMs` it was hashed at. A scan re-hashes a file only when one of these differs, so a normal launch reads no file contents. Runner up: the PDF.js document fingerprint (PDF only, needs the file parsed).
- **Storage**: one `library.json` in the app data folder, saved atomically: write `library.json.tmp`, call `sync_all()` (forces it to disk), then `std::fs::rename` over the old file (on Windows this replaces it). Runner up: SQLite (only worth it at thousands of books or heavy notes).
- **Logic lives in Rust**: which section a book is in, its display title, whether it is present, and the finished rule are all worked out in Rust and covered by `cargo test`. The screen only displays what Rust returns.
- **Watching**: the `notify-debouncer-mini` crate with a 500 ms debounce (it waits for changes to settle) triggers a full rescan, then tells the screen to refresh. A full rescan is cheap thanks to the hash cache, so no incremental bookkeeping. Any watcher error or overflow also triggers a full rescan.
- **Covers**: made in the screen by the PDF.js core library (render page 1 at 240 px wide), one book at a time, in the background after the shelf is shown. The PNG goes to Rust, which saves it. The same pass reads the page count and the PDF title. Runner up: rendering in Rust with PDFium (adds a native DLL to ship).
- **Interface chrome follows the page theme**: Original and Paper use the paper palette, Night uses the night palette. One switch, no separate Windows light/dark setting.
- **Small official plugins**: `tauri-plugin-dialog` (folder picker), `tauri-plugin-single-instance` and `tauri-plugin-window-state` (AC-13), `tauri-plugin-opener` (web links, AC-17).
- **Tauri CLI through cargo** (`cargo install tauri-cli --version "^2"`, then `cargo tauri dev` / `cargo tauri build`), so the project needs no `package.json`.
- **App identifier**: `com.readerx.desktop` (sets the data folder `%APPDATA%\com.readerx.desktop\`).

## Feature design

**Data model sketch** (`library.json` in `%APPDATA%\com.readerx.desktop\`):

| Entity | Field | Type | Required | Notes |
|---|---|---|---|---|
| settings | schemaVersion | integer | yes | `1` |
| settings | booksFolder | string (absolute path) | nullable | null until chosen |
| settings | pageTheme | `original` \| `paper` \| `night` | yes | default `paper` |
| book (map key = id) | id | string `<sha256 hex>-<size>` | yes | unique; primary key |
| book | path | string (absolute path) | yes | last seen location |
| book | fileName | string | yes | from path |
| book | metaTitle | string | nullable | the title inside the PDF, from the cover probe |
| book | format | `pdf` | yes | `epub` arrives in v3 |
| book | sizeBytes | integer | yes | also part of the hash cache |
| book | mtimeMs | integer | yes | file modified time when hashed (hash cache) |
| book | pageCount | integer | nullable | null until the first cover probe or first open |
| book | addedAt | ISO 8601 date time | yes | first seen |
| book | lastOpenedAt | ISO 8601 date time | nullable | set by `open_book` |
| book | position | tagged object | nullable | `{ kind: "pdf", page, top, left, zoom, atEnd }`; v3 adds `{ kind: "epub", cfi }` |
| book | progress | number 0..1 | yes | default 0; for PDF `page / pageCount` |
| book | finishedAt | ISO 8601 date time | nullable | |

Files beside it: `covers\<id>.png` (one per book, made once).
Worked out on every read, never stored: `present` (file found in the last scan), `section` (null when not present) and `displayTitle`.
Kept in memory only: the startup `notice` (Rust, for this session) and the set of ids whose probe failed (screen; those are probed again at each launch).
Not in v1 but reserved: v2 adds a top level `annotations` list keyed by book id.

`position.zoom` holds the PDF.js zoom value as the viewer gives it: a number above 0 (percent), or `auto`, `page-actual`, `page-fit` or `page-width`. `top` and `left` are PDF points from the viewer's `location`. Any finite number is valid for them; `left` is negative when the page is narrower than the window. `atEnd` is true when the viewer is scrolled to the bottom (within 2 px).

**Display title rule**: use the trimmed `metaTitle` unless it is empty, equals `untitled` (any letter case), starts with `Microsoft Word - `, or ends in a file extension (`.docx`, `.pdf`, `.doc` and the like). Otherwise use `fileName` without `.pdf`.

**State transitions** (book section, derived):
- `to-read` (position null) → `reading` (first `save_position`)
- `reading` → `finished` (`save_position` reaches the end from a place before it, or `set_finished(true)`)
- `finished` → `reading` (`set_finished(false)`; position kept)
- `to-read` → `finished` (`set_finished(true)` without reading)
- Any state ↔ not present (file disappears or returns; the stored record never changes because of it)

**Finished rule** (in `save_position`): the book is "at end" when `page == pageCount` or `atEnd` is true. A null previous position counts as not at end. If the previous position was not at end, the new one is, and `finishedAt` is null, then set `finishedAt` to now. So a one page PDF is finished on its first open, which is accepted.

**API surface** (Tauri commands called from the screen, plus one event):

| Command | Key inputs | Key outputs | Access | Key errors |
|---|---|---|---|---|
| `get_state` | none | `{ settings, books: BookView[], folderStatus: ok \| missing \| unset \| empty, notice? }` where `BookView` = book + `present` + `section` + `displayTitle` + `coverPath?` | screen only | none (always answers) |
| `choose_folder` | `path: string` (req) | same as `get_state` | screen only | `not_a_folder` |
| `open_book` | `id: string` (req) | `{ path, position }`, sets `lastOpenedAt` | screen only | `unknown_book`, `not_present` |
| `save_position` | `id` (req), `position` (req), `pageCount: integer` (req) | `{ progress, finishedAt }`; also stores `pageCount` on the book | screen only | `unknown_book`, `bad_position` (page outside 1..pageCount, zoom not a positive number or one of the four names, top or left not finite) |
| `set_meta` | `id` (req), `metaTitle?: string`, `pageCount: integer` (req) | ok | screen only | `unknown_book` |
| `save_cover` | `id` (req), `png: bytes` (req, max 2 MB) | ok | screen only | `unknown_book`, `bad_image` |
| `set_finished` | `id` (req), `finished: bool` (req) | `BookView` | screen only | `unknown_book` |
| `set_page_theme` | `theme` (req) | ok | screen only | `bad_theme` |
| `dismiss_notice` | none | ok | screen only | none |
| event `library-changed` | sent by Rust after each watcher rescan, and when the folder goes missing or returns | none; the screen calls `get_state` again | | |

The screen turns every `path` and `coverPath` into a URL with `window.__TAURI__.core.convertFileSrc(path)` (that function exists only on the JavaScript side).

**Reader mechanism** (how the iframe is driven; the viewer's own `?file=` link is never used, because the viewer rejects files from another origin, and books come from `http://asset.localhost` while the app runs on `http://tauri.localhost`):
1. Before setting the iframe source, the parent page listens on its own `document` for the viewer's `webviewerloaded` event. In it, it sets these `PDFViewerApplicationOptions`: `defaultUrl: ""`, `disablePreferences: true`, `externalLinkTarget: 2` (open in a new window), `viewerCssTheme` (1 for Paper and Original, 2 for Night), and for Paper and Night `forcePageColors: true` with `pageColorsBackground` and `pageColorsForeground` from the palette.
2. The iframe loads `ui/vendor/pdfjs/web/viewer.html#page=<p>&zoom=<z>,<left>,<top>`, with no hash when the position is null. After `PDFViewerApplication.initializedPromise` resolves, the parent calls `PDFViewerApplication.open({ url: convertFileSrc(path) })`. The hash sets the first view.
3. Saving: listen to `eventBus` `updateviewarea`. Ignore every event until `PDFViewerApplication.isInitialViewSet` is true, and never flush before then. After that, debounce to 1 second and call `save_position` with `location.pageNumber`, `top`, `left`, `scale` (as `zoom`), `atEnd` (from the viewer container: `scrollTop + clientHeight >= scrollHeight - 2`) and `PDFViewerApplication.pagesCount`.
4. Flush (an immediate save, still only after the initial view is set): on Back, on `Escape`, and on window close via `getCurrentWindow().onCloseRequested(async …)` (not `beforeunload`).
5. `Escape`: the listener is attached to the iframe's document, because key presses inside the iframe never reach the parent. Ignore it while the PDF.js find bar or a dialog (password, document properties) is open.
6. Lock the viewer to one book: CSS hides `#openFile`, `#print`, `#download`, `#viewBookmark` and their secondary toolbar copies. Capture `dragover` and `drop` on the iframe document and cancel them. Replace `iframe.contentWindow.open` with a function that sends `http`, `https` and `mailto` URLs to `tauri-plugin-opener`, and ignores everything else.
7. Theme switch while reading: page colors are read only when the viewer starts, so flush, then reload the iframe at the saved position with the new options.
8. Theme the viewer chrome: an extra stylesheet injected into the iframe overrides the viewer's CSS color variables with the palette, so no blue accents remain.

**Reader bar** (our own, above the iframe): Back on the left, the book title in the middle, the Original / Paper / Night toggle on the right. 40 px tall, palette colors.

**Value sourcing**:

| Action | Value produced / displayed | Source |
|---|---|---|
| shelf | book list, `present` | Rust scan of `settings.booksFolder` (recursive), matched through the hash cache and then by fingerprint |
| shelf | title | `displayTitle`, derived in Rust from `metaTitle` and `fileName` (rule above) |
| shelf | cover | `coverPath` (`covers/<id>.png`) turned into a URL by `convertFileSrc`; missing file → monochrome tile; probe failed this session → `Can't preview` tile |
| shelf | `p. X / Y` | `position.page` / `book.pageCount` |
| shelf | `Z%` | `round(book.progress * 100)` |
| shelf | last read date text | `book.lastOpenedAt`, in the PC's local time zone (JavaScript `Date`). Calendar days between that date and today; a future date shows `Today`. The month names come from a fixed list of 3 letter English abbreviations (`Jan` to `Dec`), not `toLocaleDateString`. |
| shelf | Continue book | `max(lastOpenedAt)` over books with `present && finishedAt == null && position != null`, chosen in Rust |
| shelf | section and order | derived in Rust (rules in AC-5). Title order compares `displayTitle` with letter case ignored, and breaks ties by id |
| shelf | `folderStatus` | Rust: `unset` (no folder), `missing` (path not a readable folder), `empty` (no PDFs found), else `ok` |
| shelf | notice banner | Rust session `notice`, set when a corrupt file was set aside; cleared by `dismiss_notice` |
| `save_position` | `progress` | `position.page / pageCount` (pageCount from `PDFViewerApplication.pagesCount`) |
| `save_position` | `finishedAt` | now, by the finished rule above |
| `open_book` | file URL | `convertFileSrc(book.path)` in the screen (asset protocol, scope limited to the books folder) |
| reader | starting page, offset, zoom | `book.position` from `open_book`, put in the viewer hash; null → viewer default (page 1) |
| reader | page colors, viewer theme | `settings.pageTheme` mapped to the palette below |
| cover probe | pageCount, `metaTitle` | PDF.js core library (`getDocument`, `getMetadata().info.Title`) while rendering page 1 → `set_meta` |
| all stored dates | current time | Rust `SystemTime::now()` as UTC ISO 8601 |
| corrupt file name | timestamp | Rust, UTC, formatted `YYYYMMDD-HHMMSS` (no `:` which Windows file names forbid) |

**Palette** (monochrome, no pure black or pure white):

| Token | Paper | Night |
|---|---|---|
| background | `#F4F1EA` | `#1E1E1E` |
| surface (cards) | `#ECE8DF` | `#262626` |
| text | `#2B2A28` | `#D8D5CF` |
| muted text | `#625E58` | `#9A968F` |
| lines | `#DDD8CD` | `#363636` |

Typeface: the Windows system font (`Segoe UI`, via `system-ui`); no font files shipped. Page recoloring uses the PDF.js options in the reader mechanism above. They also turn images into two tones, which is accepted. If the pinned version does not honor them for normal PDFs, fall back to a CSS filter on the viewer's page container (Paper: light sepia; Night: invert plus hue rotate). Original applies no recoloring. The main window starts hidden with its `backgroundColor` set to the theme background, and is shown once the first screen has painted.

**Scan rules**:
- Walk `booksFolder` recursively. Skip entries that can't be read (for example `System Volume Information` on a drive root), and do not follow symbolic links or junctions.
- Skip a file whose modified time is less than 2 seconds old (it may still be copying), and schedule another scan 3 seconds later.
- Look up each file in the hash cache (`path`, `sizeBytes`, `mtimeMs`). Hash only on a miss. Hashing runs outside the library lock; the lock is taken only to merge the results.
- If the folder disappears while the app is running, set `folderStatus = missing` and send `library-changed`. Keep watching the parent folder, or rescan every 10 seconds, so its return is noticed.

**Asset protocol**: enable `app.security.assetProtocol.enable: true` in `tauri.conf.json` and the Cargo feature `protocol-asset`. The runtime scope is not remembered between runs. So at every startup, and in `choose_folder`, call `asset_protocol_scope().allow_directory(folder, true)` for the books folder and the covers folder. Never call `forbid_directory` on a folder change, because it blocks that folder until restart even if chosen again.

**Key invariants**:
- Every write to `library.json` goes through one Rust `Mutex<Library>` and is atomic: write `.tmp`, `sync_all()`, rename. If the rename fails (antivirus, indexing or backup holding the file), retry up to 5 times, 50 ms apart. If it still fails, keep the data in memory, log it, and try again on the next save. A leftover `library.json.tmp` is deleted at startup. Nothing else writes the file.
- `id` is checked against `^[0-9a-f]{64}-[0-9]+$` before it is used in any file path (covers), so no input can escape the covers folder.
- A missing file never deletes or changes a book record (AC-9, AC-15).
- Two files with identical content share one id. The scan keeps the first path in sorted order, and the copy is ignored.
- A book whose content changes (same path, new bytes) gets a new id. The old record stays, with its progress, as not present.
- `progress` is always between 0 and 1. `position.page` is between 1 and `pageCount`.
- Auto finish happens only on the move to the end, never by just being there.
- No position is saved before `isInitialViewSet` is true for the open book.

**Security model**: a single user app working on local files only. No network calls, no accounts, nothing sent anywhere. The webview runs with Tauri's default isolation. Capabilities allow only: the commands above, `core:default`, `core:event:default` (for `library-changed`), `core:window:allow-destroy` (for the save on close), `dialog:allow-open`, `opener:allow-open-url`, and the window state and single instance plugins. Content Security Policy: `default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; img-src 'self' asset: http://asset.localhost blob: data:; connect-src 'self' ipc: http://ipc.localhost asset: http://asset.localhost`. The asset protocol scope covers only the chosen books folder and the covers folder. `save_cover` checks the PNG signature and the 2 MB limit. The single instance callback calls `unminimize()`, `show()`, then `set_focus()` on the main window.

**Configuration required**: no environment variables or secrets. One time machine prerequisites before coding:
- Rust stable via `rustup` (MSVC toolchain)
- Visual Studio 2022 Build Tools with the "Desktop development with C++" workload
- Microsoft Edge WebView2 Runtime (already on current Windows 10 and 11)
- `cargo install tauri-cli --version "^2"`

**Critical test scenarios**:
- Happy path: choose a folder with nested PDFs, open one, scroll to page 40 and zoom 150%, close the app, relaunch, click Continue, and land on page 40 at 150% at the same scroll spot. Verifies **AC-1**, **AC-2**, **AC-3**, **AC-4**.
- No overwrite while loading: open a large book saved at page 200 and press Back at once. The saved position is still page 200. Reopen an unfinished book saved at its last page, and it stays unfinished. Verifies **AC-2**, **AC-7**.
- Rename and move: rename a started book and move it into a subfolder while the app is open. Within 2 seconds it is still in Reading with the same progress. Verifies **AC-8**, **AC-9**.
- Crash safety: kill the process during a burst of position saves, relaunch, and `library.json` parses with the last completed save. Separately, a hand corrupted `library.json` is set aside under a valid Windows file name, and the banner is shown. Verifies **AC-12**.
- Finished rule: going from page 379 to 380 of 380 sets Finished. So does scrolling to the bottom of a book whose last page is short. Mark as unfinished, reopen (lands at the end), and the book stays in Reading. Verifies **AC-7**.
- Bad files: a zero byte `.pdf`, a corrupt PDF and a password PDF in the folder show `Can't preview` tiles, with no password prompt during the probe, while every other book loads normally. Verifies **AC-11**.
- Folder states: unplug the drive while the app is open, see `Books folder not found`; plug it back, see the shelf return. An empty folder shows `No PDFs found`. Verifies **AC-15**, **AC-16**.
- Reader lockdown: dropping a PDF onto the viewer does nothing, and a web link in a PDF opens the normal browser. Verifies **AC-17**.
- Startup: 200 unchanged books on a cold start reach the shelf in under 2 seconds, with no file contents read (hash cache hit). Verifies **AC-14**.
- Path safety: `save_cover` with id `../../evil` is rejected and no file is written outside `covers\`. Verifies the id invariant (supports **AC-6**).
- Unit tests (`cargo test`): fingerprint stability, hash cache hit and miss, scan merge (new, removed, renamed, duplicate, still copying), display title rule, section derivation and ordering, the finished rule (including a null previous position and `atEnd`), position validation, atomic save, rename retry and corrupt file handling, all with temporary folders and fixed timestamps.

## Build plan

No build approach is on record, so this plan assumes **Tracer Bullet**: first one thin working path from folder to reader and back, then make it thicker.

1. Install the prerequisites, `git init`, scaffold Tauri 2 with a plain `ui/` folder (`app.withGlobalTauri: true`, no bundler, identifier `com.readerx.desktop`, asset protocol enabled), and copy the pinned PDF.js prebuilt viewer into `ui/vendor/pdfjs/`. Satisfies **AC-14** (foundation).
2. Rust `library` module: types for the data model, load, atomic save with `sync_all` and rename retry, leftover `.tmp` cleanup, corrupt file set aside with the notice, unit tests. Satisfies **AC-12**.
3. Rust fingerprint, hash cache and recursive scan and merge (scan rules above, present flags, duplicates, rename and move keep the id, display title rule), unit tests. Satisfies **AC-1**, **AC-8**, **AC-9**, **AC-14**.
4. `get_state`, `choose_folder` (with the runtime asset scope) and `dismiss_notice`. The welcome, folder missing and no PDFs screens, the notice banner, the Change folder button, and a plain list of titles. Satisfies **AC-1**, **AC-12**, **AC-15**, **AC-16**.
5. Reader screen: the slim reader bar, `open_book`, and the reader mechanism (steps 1 to 5: options at `webviewerloaded`, `open({url})` after `initializedPromise`, saves gated on `isInitialViewSet`, 1 second debounce, flush on Back, Escape and close). **The thin path works end to end here.** Satisfies **AC-2**, **AC-3**.
6. Section derivation and the Continue choice in Rust (unit tests), and the shelf layout with the Continue card and three sections. Satisfies **AC-4**, **AC-5**.
7. Cover and metadata probe queue in the screen (one book at a time, catches password and parse errors without prompting), `save_cover` (validated), `set_meta`, the cards with `p. X / Y · Z%` and the last read date, and the `Can't preview` tiles. Satisfies **AC-6**, **AC-11**.
8. The finished rule in `save_position` with position validation (unit tests), the `atEnd` flag from the viewer, `set_finished`, and the card menu. Satisfies **AC-7**.
9. Folder watcher (`notify-debouncer-mini`, 500 ms, rescan on error or overflow, missing folder detection) → rescan → `library-changed` → shelf refresh. Satisfies **AC-9**, **AC-15**.
10. Reader lockdown (reader mechanism step 6) with `tauri-plugin-opener`. Satisfies **AC-17**.
11. Palette, the page theme toggle with page recoloring (or the CSS fallback), reload on theme switch, viewer chrome theming, `set_page_theme`, hidden window with themed background, the single instance and window state plugins, and a keyboard pass (Tab and Enter on the shelf, Escape in the reader). Satisfies **AC-10**, **AC-13**.
12. `cargo tauri build` with the NSIS installer (install for the current user, Start menu shortcut, desktop shortcut offered). Then time a cold launch with a 200 book test folder. Satisfies **AC-14**.

## Consequences

**Positive**:
- One click from launch to the exact spot in the current book: the original pain is gone.
- Small installer (roughly 5 to 10 MB) that starts quickly and uses little memory.
- The tagged position, the format field and the id based on file content let EPUB (v3) and notes (v2) fit in without changing existing data.
- The core rules sit in Rust and run under `cargo test`, so they are checked without clicking through the app.

**Negative / tradeoffs**:
- A heavier first setup than Electron would need: the Rust toolchain plus about 6 GB of C++ Build Tools, and slow first builds (several minutes).
- Two languages to work in: Rust for the core, JavaScript for the screen and the viewer glue. The viewer glue relies on PDF.js internals (`PDFViewerApplication`, its events and element ids), which is why the version is pinned; upgrading PDF.js means checking that glue again.
- Editing a PDF in another program (saving notes into the file) changes its id, so ReaderX treats it as a new book and the old progress stays orphaned. v2 must keep highlights inside ReaderX, not in the PDF, for this reason.
- Books load through the asset protocol as whole files (PDF.js can't use range requests across origins here). Fine for reading, but the first launch cover pass reads every book in full, so it runs one book at a time.
- Paper and Night turn PDF images into two tones. Original keeps them as they are.
- Switching the theme while reading reloads the viewer (a short blink), because page colors are set only at start.

**Neutral**:
- WebView2 (the Edge engine) renders everything, so the app looks the same as PDF.js does in Edge.
- If the books folder sits in OneDrive, cloud only files are downloaded the first time they are hashed. The hash cache keeps that to once per file.

## Follow-up

- [ ] Run `/audit` after scaffolding to create `AGENTS.md` (stack, commands, conventions). Record in it that Agent Skills and MCP servers for Tauri 2 and PDF.js were offered and declined on 2026-11-16.
- [ ] v2 spec: highlights and notes (PDF.js highlight editor, stored in `annotations` keyed by book id, never written into the PDF, Markdown export for Obsidian).
- [ ] v3 spec: EPUB support (pick between foliate-js and epub.js, `position.kind = "epub"` with a CFI, a position format for EPUBs, which reflow instead of having fixed pages).
- [ ] At step 11, confirm whether the pinned PDF.js version honors `forcePageColors` for normal PDFs, and keep the CSS fallback only if it does not.
- [ ] No scope file exists yet. Enroll this as a scope feature if you start using `/scope`.

## Rationale

Reasoning and options: see [rationale.md](rationale.md).
