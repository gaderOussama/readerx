# 0003. ReaderX v3: EPUB books

**Date**: 2026-11-17 (revised 2026-11-19 after the cross check)
**Status**: Proposed

## Summary

EPUB books in the books folder appear on the shelf next to the PDFs, with their cover, title and progress, and open in a reader that resumes at the exact spot. Text flows as book pages (or one long scroll, if you switch), in the Original, Paper and Night themes, with one font size setting for all EPUBs. A Contents panel lists the chapters and searches the book. Highlights and notes for EPUB come later in their own issue. The renderer is foliate-js, vendored and pinned like PDF.js, so there is still no bundler and no network.

## Requirements

**User stories**:
- As a reader, I want my EPUB books on the same shelf as my PDFs so that all my reading is in one place.
- As a reader, I want an EPUB to open at the exact spot where I stopped, even after I change the font size or the window size.
- As a reader, I want comfortable text (theme, font size, pages or scroll) and quick ways to move through a book (chapters, search).

**Acceptance criteria**:
- **AC-1**: Every `.epub` file (any letter case) in the books folder and its subfolders appears on the shelf like a PDF, with the same identity rules (spec 0001: content hash, so renames and moves keep progress). The folder watcher picks up added, removed and renamed EPUBs. The empty folder message reads `No PDF or EPUB books found in <folder>`. The first run hint's first sentence becomes `Choose the folder that holds your books.` and its second sentence stays as it is.
- **AC-2**: In the background, each EPUB gets its cover (rendered to the same 240 px wide image as PDF covers), its title and its author from the book's own metadata. A book without a cover image, or whose cover image fails to decode, shows the title tile and is not probed again on later launches. A damaged or copy protected EPUB shows `Can't preview`. A title missing from the metadata falls back to the file name without `.epub`.
- **AC-3**: An EPUB card shows the chapter name and the percent (for example `Chapter 3 · 34%`). The chapter name is cut to one line with an ellipsis; the ` · 34%` part is always shown in full. With no saved chapter (the book has no table of contents, or nothing was saved yet) the card shows `<percent>%` alone. An unopened EPUB shows nothing, as an unopened PDF does.
- **AC-4**: Opening an EPUB shows it in the reader. The first open starts at the beginning of the book (its first section, cover included). Every later open resumes at the exact saved spot, with the same text at the top of the screen, also after a font size, layout or window size change and after an app restart. If the saved CFI can't be resolved, the book opens at the saved percentage instead.
- **AC-5**: The reader saves the position 1 second after the reading spot stops changing, and at once on Back, Escape back to the shelf, theme switch and window close, as PDFs do (spec 0001).
- **AC-6**: Paginated layout is the default. Text is laid out in pages, as two columns side by side when the reader area is wider than 1000 px and one column otherwise. Right arrow, Page Down and Space go to the next page; Left arrow and Page Up go back (arrows follow the book's direction, so in a right to left book Left goes forward). In Pages mode only, a click on the left or right fifth of the reader area goes back or forward, unless the click is on a link or text is selected. In Scroll mode the keys scroll by one screen and clicks do nothing special.
- **AC-7**: A `Pages` / `Scroll` switch in the reader bar changes to continuous scrolling and back. The choice is stored once for all EPUBs and the reading spot is kept across the switch.
- **AC-8**: `A−` and `A+` buttons in the reader bar, and the keys `Ctrl` with `-` or NumpadSubtract (smaller) and `Ctrl` with `=`, `+` or NumpadAdd (bigger), step the font size through 80, 90, 100, 115, 130, 160 and 200 percent (default 100). The size is stored once for all EPUBs, applies at once, and keeps the reading spot. Books that set fixed pixel font sizes scale less; that is accepted.
- **AC-9**: Original shows the book's own colors and fonts on a white page. Paper and Night replace every text, background, border and link color in the book with the palette (spec 0001), keep images as they are (in Night, images sit on a white background so dark line art stays visible), and meet the 4.5:1 contrast rule of spec 0001 AC-10, also for books with colored headings. Switching theme applies at once without losing the spot. Fixed layout books change only the page background around the page.
- **AC-10**: A `Contents` button in the reader bar (in the place of the Highlights button, which is hidden for EPUB) opens a side panel (the same overlay style as the Highlights panel of spec 0002) with two tabs. **Contents** lists the book's table of contents with nesting, marks the current chapter, and jumps to a chapter on click. A book with no table of contents shows `This book has no table of contents.` and the panel opens on the Search tab. **Search** runs when Enter is pressed in its field (2 characters or more, letter case ignored), shows `Searching…` while it runs, lists each match under its chapter with a short excerpt (the match in bold), shows `No matches` when nothing is found, and jumps to the match on click with the match marked on the page. A new search cancels the running one; closing the panel clears the marks.
- **AC-11**: Fixed layout EPUBs (picture books, comics) open page by page and resume the same way; the font size buttons and the layout switch are hidden for them.
- **AC-12**: A damaged or copy protected EPUB opens to the message `This book can't be opened. It may be damaged or copy protected.` and Back keeps working. A book counts as copy protected when `META-INF/rights.xml` exists or `META-INF/encryption.xml` lists any encryption method other than the two font obfuscation methods (IDPF and Adobe).
- **AC-13**: An EPUB counts as Finished the first time its reading spot reaches 98% or more, or the last page of the book is on screen, and stays Finished when you move back, as PDFs do.
- **AC-14**: Links inside the book to other places in the book follow inside the reader. Links to `http`, `https` and `mailto` open in the system browser; other external links do nothing. Scripts inside an EPUB never run, and the reader never loads anything over the network. Audio and video inside a book are not played in v3.
- **AC-15**: While an EPUB is open, the reader blocks the browser context menu, `F5`, `Ctrl R`, `F12`, `Ctrl Shift I`, `Ctrl Shift J`, `Ctrl Shift C`, and dropped files, in the reader page and in every book section. The spec 0001 AC-17 blocks (`Ctrl O`, `Ctrl P`, `Ctrl S`, drag and drop) also hold. `Escape` closes the side panel first (also when focus is in the search field), then goes back to the shelf. The Highlights button is hidden for EPUBs, and the highlight commands refuse EPUB books. The PDF reader is not changed by this.

## Decision

**Chosen option**: foliate-js as the EPUB renderer, vendored at a pinned commit into `ui/vendor/foliate-js/` and used as its `<foliate-view>` element directly in the reader page (no viewer iframe). EPUB covers and metadata come from the existing background cover queue using foliate-js, so Rust gains no new dependency. Positions are EPUB CFIs (a standard pointer to an exact spot in an EPUB) with a fraction and a chapter label.

**Implementation skills**: none (Agent Skills and MCP servers were not searched, at the owner's choice).

## Rationale

Reasoning and options: see [rationale.md](rationale.md).

## Feature design

**Data model sketch** (only new or changed fields; everything else as in specs 0001 and 0002):

| Entity | Field | Type | Rule |
|---|---|---|---|
| Settings (`library.json`) | `epubFontSize` | int | one of 80, 90, 100, 115, 130, 160, 200; default 100 |
| Settings | `epubLayout` | `"paginated"` or `"scrolled"` | default `"paginated"` |
| Book | `format` | `"pdf"` or `"epub"` | from the file extension at scan time |
| Book | `metaAuthor` | string, nullable | new; from EPUB metadata; stays null for PDFs; not shown yet |
| Book | `coverChecked` | bool | new, default false; set true by `set_meta`; a book with it true is never probed again |
| Book | `pageCount` | int, nullable | stays null for EPUB |
| Book | `position` | tagged object | adds `{ kind: "epub", cfi, fraction, chapter }` |
| EpubPosition | `cfi` | string | starts with `epubcfi(`, at most 2,000 characters |
| EpubPosition | `fraction` | number | finite, 0 to 1 |
| EpubPosition | `chapter` | string, nullable | the table of contents label at the spot, trimmed, at most 200 characters |
| Book | `progress` | number | for EPUB equals `position.fraction` |
| Book | `finishedAt` | ISO, nullable | EPUB: set the first time `fraction >= 0.98` or `atEnd` is true |

Old `library.json` files load unchanged (new settings take their defaults, `metaAuthor` defaults to null, `coverChecked` to false). No highlights file is ever created for an EPUB in v3.

**Reader mechanism** (all in `ui/`):
1. Session: `reader.js` keeps one reader session per open book (the v1 `cur` session, send queue and flush). For `format: "epub"` it creates a `<foliate-view>` in `#viewer-host` instead of the PDF.js iframe. The session gains a `ready` flag, false until resume (step 2) has finished; `relocate` events before that are ignored, so the open itself never saves a position. `send()` checks `ready` for EPUB instead of `isInitialViewSet`, and sends no page count. `openReader` and `closeReader` skip `startHighlights` and `endHighlights` for EPUB, and the highlights Escape handler is not involved (it focuses `session.frame`, which an EPUB session does not have). Every async step checks again that the session is still current.
2. Open and resume: read the file with `fetch(convertFileSrc(path))`, wrap it as `new File([blob], fileName)`, and pass it to `view.open(file)`. Run the protection check (step 10). Apply flow, columns and styles (steps 4 and 5). Then, when a position exists: a bad CFI does not reliably throw in foliate-js (an unparsable one silently opens the first section, an unknown spine item gives a blank view), so first check `view.resolveNavigation(position.cfi)`; when it is falsy or its `index < 0`, skip the CFI. Otherwise `await view.init({ lastLocation: position.cfi })` inside a try/catch. If the CFI was skipped, `init` threw, or `view.lastLocation` is still empty afterwards, `await view.goToFraction(position.fraction)`. With no position, `await view.init({})`, which opens the first linear section (the cover is not skipped). Then set `ready`. There is no timer.
3. Saving: on the view's `relocate` event (once `ready`), take `detail.cfi`, `detail.fraction`, `detail.tocItem?.label`, and `atEnd`, which is `view.renderer.atEnd === true` (exact in Pages mode, about one screen early in Scroll mode; the fixed layout renderer has none, and there `fraction` reaches 1 on the last spread, so the 98% rule covers it). Keep them as the pending position and save through the session's send queue 1 second after the last `relocate`. Flush on Back, Escape back, theme switch and window close (the v1 hooks).
4. Layout and size: `view.renderer.setAttribute("flow", layout === "scrolled" ? "scrolled" : "paginated")`; `max-column-count` comes from `columnCount(width)` in `ui/lib/epub.js` (2 when the `#viewer-host` width is over 1000 px, else 1), set on open and on every resize, followed by `view.renderer.render()` because changing the attribute alone does not lay out again (foliate-js also forces one column when the area is taller than wide; that is accepted), with `max-inline-size` fixed at 720 px so the column count is the only thing deciding two columns. The side panel is an overlay, so it does not change the width. The font size is `html { font-size: <n>% !important }` in the styles given to `view.renderer.setStyles`. Changing size or layout saves the settings through `set_epub_settings`, applies them to the open view, then calls `view.goTo` with the CFI held before the change.
5. Themes: `themeStyles(theme, palette, fontSize)` in `ui/lib/epub.js` builds the style text. Original gives only the font size rule. Paper and Night add, all `!important`: `html, body { color: <text>; background: <bg> }`, `*:not(img, svg, video, image) { color: inherit; background-color: transparent; border-color: currentColor }`, `a, a * { color: <text> }`, and for Night `img, svg { background: #fff }`. The palette values are written in as literal colors (the section documents can't see the app's CSS variables). The reader page background uses the same palette. A theme switch calls `setStyles` again (no reload) and flushes the position, as for PDF. `switchTheme` gets an EPUB branch for this; it never calls `loadViewer` for EPUB.
6. Fixed layout: when `view.book.rendition?.layout === "pre-paginated"`, hide `A−`, `A+` and the layout switch for this session, do not call `setStyles` (the fixed layout renderer has none), and theme only the page background of `#viewer-host`. That renderer also has no `atEnd`, fires one `load` per page frame, and draws no search marks (search still jumps to the match). For Original, `#viewer-host` has its own white background.
7. Keys: one `keydown` handler on the reader page `document` while an EPUB is open, plus the same handler in the capture phase on every section document the view loads (its `load` event gives `detail.doc`), so keys work wherever focus is. It ignores events whose target is an `input` or `textarea`, except Escape. It handles page turns with `view.goLeft()` and `view.goRight()` for the arrows (they follow the book's direction) and `view.next()` and `view.prev()` for Page Down, Space and Page Up; `Ctrl` font keys (AC-8); Escape (AC-15); and the lockdown keys (AC-15). The lockdown also adds `contextmenu` and `drop` listeners with `preventDefault` on the reader page and every section document.
8. Edge clicks (Pages mode only): a `click` listener on each section document. In Pages mode the section iframe is wider than the screen and scrolled, so the click's screen position is `frame.getBoundingClientRect().left + e.clientX * (rect.width / frame.offsetWidth)` with `frame = doc.defaultView.frameElement` (the scale factor is 1 in the paginator; fixed layout frames are scaled with CSS). When that is inside the left or right fifth of the `<foliate-view>` rectangle, the target is not inside an `a` element and no text is selected, call `view.goLeft()` or `view.goRight()`.
9. Links: listen to the view's `external-link` event and always call `preventDefault()` (otherwise foliate-js calls `globalThis.open` with any href, `javascript:` included). Its detail is `{ a, href_ }`, where `href_` is the raw attribute; pass it to the v1 `openExternal` only when it starts with `http:`, `https:` or `mailto:` (anything else is dropped). Also set `window.open` on each section document's window to the same handler, as `lockDown` does for PDFs. Internal links (`link` event) are left to foliate-js.
10. Protection check: after `view.open` (and in the probe after `makeBook`), read `await book.loadText("META-INF/rights.xml")` and `await book.loadText("META-INF/encryption.xml")` (public on the book; each gives null when the entry is missing). If the book is protected by the AC-12 rule, treat it as an open error. foliate-js only warns on unknown encryption, so this check is what decides.
11. Errors: if `open`, `init` with the fraction fallback, or the protection check throws, show the AC-12 message in `#viewer-host` (as the PDF reader does) and add the book id to the probe's `failed` set. Any throw counts; do not match on error classes (a non zip file fails with a plain TypeError because the other format loaders are not vendored).
12. Closing: on Back, wrap `view.close()` in a try/catch (it throws when no section ever loaded) and then call `view.book?.destroy?.()`, before removing the element.

**Contents panel** (new `ui/toc.js`, EPUB only):
- A new `<aside id="toc-panel" class="hl-panel">` in `ui/index.html`, reusing the Highlights panel's overlay styles; it does not touch `#hl-panel` or `highlights.js`. A `Contents` button `#toc-toggle` sits in the reader bar where `#hl-toggle` is; for EPUB `#hl-toggle` is hidden and `#toc-toggle` shown, for PDF the other way round.
- Two tab buttons, `Contents` and `Search`. Contents renders `view.book.toc` as a nested list, marks the item matching the last `relocate` `tocItem`, and calls `view.goTo(item.href)` on click. An empty or missing toc shows `This book has no table of contents.` and the panel opens on Search.
- Search has one text field. Enter with 2 or more characters stops any running search, shows `Searching…`, and iterates `view.search({ query, matchCase: false })` (which clears the old marks itself, and also ignores accents). Items with `subitems` are a chapter (`label`) and its matches (`cfi`, `excerpt: { pre, match, post }`); `{ progress }` items are ignored; `"done"` ends it. Each match is shown as `pre`, `match` in bold, `post`. No matches shows `No matches`. A click calls `view.goTo(match.cfi)`.
- Escape (handled in reader step 7) closes the panel first. Closing the panel stops the running search and calls `view.clearSearch()`.

**Cover and metadata probe** (in `ui/covers.js`, the existing queue, one book at a time, paused while reading):
- `needsProbe` in `ui/lib/covers.js` becomes `!book.coverPath && !book.coverChecked`.
- For an EPUB: fetch the file, `makeBook(new File([blob], fileName))` (no view), run the protection check, read `pickString(book.metadata.title)` and the first author through `pickString`. `pickString` in `ui/lib/epub.js` takes a string, an object with `name`, a language map like `{ en: "..." }` (first value), or an array (first item), recursing until it reaches a string (an author's `name` can itself be a language map), and returns a trimmed string or null.
- `await book.getCover()` in its own try/catch (it throws when the cover file is missing from the zip; that counts as no cover). A cover blob is drawn through an `<img>` (so SVG covers work) to a canvas 240 px wide, keeping its proportions, and saved with `save_cover` as PNG. If the cover is missing or fails to decode, skip it.
- Then call `set_meta` with title, author and no page count; Rust sets `coverChecked`. A book with no usable cover shows the title tile and is never probed again.
- The whole probe has the same 60 second timeout as the PDF probe. A thrown error, a timeout or a protected book puts the id in `failed` (the `Can't preview` tile) and does not call `set_meta`, so it is tried again on the next launch, as for PDFs.

**Security**: EPUB sections are shown by foliate-js in iframes made from `blob:` URLs. In WebView2 these do **not** get the app's CSP (the dev build ran a test book's inline script, 2026-11-19), so the reader also hooks foliate-js's loader: every HTML page gets `<meta http-equiv="Content-Security-Policy" content="script-src 'none'; object-src 'none'">` first in its head, SVG pages have their scripts and `on*` attributes cut out (`blockScripts` in `ui/lib/epub.js`), and script files in the book are never loaded (`load` event, `allow = false`). The CSP in `tauri.conf.json` changes to:
- `script-src 'self' 'wasm-unsafe-eval'` stays, so no script inside an EPUB can run;
- `style-src 'self' 'unsafe-inline' blob:` (new; EPUB chapters use inline styles and the book's style sheets, and foliate-js injects `<style>` itself);
- `font-src 'self' blob: data:` (new; embedded fonts);
- `frame-src 'self' blob:` (new; the section iframes);
- `img-src` and `connect-src` stay as they are (they already allow `blob:` and the asset protocol; no remote hosts);
- `media-src` is not added, so book audio and video do not play (AC-14).

Tauri adds a nonce to `style-src` only when a served HTML file has an inline `<style>`, which would turn `'unsafe-inline'` off. `ui/index.html` has none. Build step 2 checks the real CSP response header once in the dev build; only if a nonce or hash shows up in `style-src` does it add `"dangerousDisableAssetCspModification": ["style-src"]` under `app.security`. The asset protocol scope does not change (the books folder is already allowed).

**API surface** (Rust; logic in `scan.rs`, `derive.rs` and `ops.rs`, thin wrappers in `lib.rs`, new command allowed in `capabilities/default.json`):

| Command | Change | Inputs | Output | Key errors |
|---|---|---|---|---|
| (scan and watcher) | `.epub` files are books with `format: "epub"`; the count used for the empty state counts both formats | | | |
| `display_title` (derive) | strips `.epub` as well as `.pdf` from the file name fallback | | | |
| `save_position` | accepts `{ kind: "epub", cfi, fraction, chapter }` plus `atEnd` for EPUB books; `pageCount` becomes optional and is required only for PDF; sets `progress = fraction`; sets `finishedAt` the first time `fraction >= 0.98 \|\| atEnd` | `id`, `position`, `pageCount` (PDF only), `atEnd` | as today | `unknown_book`, `bad_position` (wrong kind for the book's format, a field breaking its rule, or a PDF save with no page count) |
| `set_meta` | gains `metaAuthor` (nullable); `pageCount` becomes optional (null for EPUB); sets `coverChecked = true` | `id`, `metaTitle`, `metaAuthor`, `pageCount` | as today | `unknown_book` |
| `set_epub_settings` | new | `fontSize` (one of the 7 values), `layout` (`"paginated"` or `"scrolled"`) | nothing | `bad_setting` |
| `get_state` | settings now include `epubFontSize` and `epubLayout`; books include `metaAuthor` and `coverChecked` | | | |
| highlight commands (spec 0002) | refuse EPUB books | | | `not_supported` |

**Value sourcing**:

| Action | Value | Source |
|---|---|---|
| shelf card | chapter | `position.chapter` (saved from the `relocate` event's `tocItem.label`), in its own ellipsized span |
| shelf card | percent | `round(progress * 100)`, the same rounding as `ui/lib/format.js` |
| shelf card | title | `displayTitle` (spec 0001 rule), fed by the EPUB `metaTitle`, else the file name without `.epub` |
| save_position | `cfi`, `fraction`, `chapter` | the `relocate` event detail |
| save_position | `atEnd` | `view.renderer.atEnd === true` (false when the renderer has none) |
| finished | 98% or end | `fraction >= 0.98 \|\| atEnd` in Rust |
| resume | where to start | `view.init({ lastLocation: cfi })` when `resolveNavigation(cfi)` finds it, else `goToFraction(fraction)`, else `view.init({})` |
| reader | font size, layout | `settings.epubFontSize`, `settings.epubLayout` from `get_state` |
| reader | theme colors | `settings.pageTheme` and the palette values, written into `themeStyles` |
| reader | column count | `columnCount(#viewer-host width)`: 2 above 1000 px, else 1 |
| reader | edge click position | section frame's left edge plus `e.clientX`, against the `<foliate-view>` rectangle |
| cover probe | title, author | `pickString(book.metadata.title)`, `pickString` of the first `book.metadata.author` |
| cover probe | cover | `book.getCover()`, drawn through an `<img>` |
| cover probe | probed already | `book.coverChecked`, set by `set_meta` |
| protection | protected | `META-INF/rights.xml` exists, or `encryption.xml` names a method other than the IDPF and Adobe font obfuscation URIs |
| search | chapter, excerpt | each result's `label` and `excerpt { pre, match, post }` from foliate-js |

**Key invariants**:
- EPUB files are only read, never written; book ids never change because of reading.
- No script from an EPUB runs, and nothing is fetched from the network.
- A position's `kind` always matches the book's `format`.
- PDF behavior from specs 0001 and 0002 does not change (the new lockdown keys apply only while an EPUB is open).

**Configuration required**: none. foliate-js is vendored at a pinned commit with its MIT license file; `ui/vendor/foliate-js/` is never edited, only replaced on purpose (the same rule as PDF.js). The foliate-js event and method names above (`relocate`, `load`, `external-link`, `init`, `goToFraction`, `goLeft`, `goRight`, `search`, `clearSearch`, `makeBook`, `getCover`) are checked against the pinned commit in build step 2; any difference is fixed in this spec before building on it. Pinned: commit `78914aef4466eb960965702401634c2cb348e9b1` (2026-05-01), checked 2026-11-19; the corrections from that check are already in the steps above. Checked in the dev build: blob section documents do not inherit the CSP, hence the per page meta CSP above. Still unchecked: `window.isSecureContext` (font deobfuscation needs `crypto.subtle`; only matters for books with obfuscated fonts).

**Critical test scenarios**:
- Happy path: put two EPUBs (one with a cover, one without) in the folder; both appear with cover or title tile and their metadata title. Restart; the coverless one is not probed again. Open one, read into chapter 2, Back; the card shows `<chapter 2 name> · N%`; restart and reopen; the same text is at the top. Verifies **AC-1**, **AC-2**, **AC-3**, **AC-4**, **AC-5**.
- Size and layout: change the font size twice and switch to Scroll and back; the spot is kept each time; reopen; settings are kept for another EPUB too. Verifies **AC-7**, **AC-8**.
- Pages: keys and edge clicks turn pages, also after clicking A+ (focus in the reader page); a click on a link follows it; two columns appear on a wide window; a right to left book turns the right way. Verifies **AC-6**, **AC-14**.
- Themes: Original keeps the book's colors; Paper and Night recolor text, including colored headings, with 4.5:1 contrast; images unchanged. Verifies **AC-9**.
- Contents and search: jump to a chapter; search a word, click the third result; the page shows it marked; Escape in the search field closes the panel and clears marks; a book with no toc opens on Search. Verifies **AC-10**.
- Fixed layout book opens and resumes; size buttons hidden. Verifies **AC-11**.
- Damaged file (a renamed text file) and a DRM book (with `encryption.xml` naming a non font method): `Can't preview` tile and the open message; Back works. Verifies **AC-2**, **AC-12**.
- Finished: jump to 99%, and separately to the last page of a book with one long last chapter; the book moves to Finished; move back; it stays Finished. Verifies **AC-13**.
- Safety: an EPUB with a `<script>`, a remote image and an `ftp:` link shows no script effect, makes no network request, and the link does nothing; F5, Ctrl R, F12 and right click do nothing inside the book and in the reader bar; the Highlights button is hidden. Verifies **AC-14**, **AC-15**.
- Unit tests (`cargo test`): `.epub` detection and format, `display_title` stripping `.epub`, EPUB position validation, kind and format mismatch, PDF save without page count refused, progress from fraction, the finished rule for 0.98 and for `atEnd`, `set_meta` with author and no page count setting `coverChecked`, `set_epub_settings` validation, old `library.json` loading with defaults, highlight commands refusing EPUB. Node tests: `needsProbe` with `coverChecked`, the EPUB card text parts, font size stepping, `columnCount`, `pickString`, `themeStyles`, the protection rule on sample `encryption.xml` text.

## Build plan

Tracer Bullet: open one EPUB from the shelf and resume it first, then thicken.

1. Rust: `.epub` in scan and watcher, `format`, `metaAuthor`, `coverChecked`, `display_title` for `.epub`, EPUB position with validation and optional page count, progress and the finished rule with `atEnd`, `set_meta` changes, `set_epub_settings` and the two settings, highlight commands refusing EPUB, empty state count; unit tests. Satisfies **AC-1**, **AC-13**, **AC-15**.
2. Vendor foliate-js at a pinned commit with its license and check the API names this spec uses against it. Change the CSP and check the real header for a style nonce. Open an EPUB in the reader with `<foliate-view>`: the session `ready` flag, open, resume by CFI with the fraction fallback, `relocate` and `atEnd`, save through the send queue, flush hooks, highlights skipped for EPUB. **The thin path works end to end here.** Satisfies **AC-4**, **AC-5**.
3. Shelf: the EPUB card text in two spans and the empty state wording, `needsProbe` with `coverChecked`, and the cover and metadata probe for EPUB in `covers.js` with `pickString`, the `<img>` drawing and the timeout. Satisfies **AC-1**, **AC-2**, **AC-3**.
4. Reading comfort: `themeStyles` and the `switchTheme` EPUB branch, font size buttons and keys, the Pages and Scroll switch, `columnCount`, fixed layout handling. Satisfies **AC-7**, **AC-8**, **AC-9**, **AC-11**.
5. Keys on the reader page and section documents, edge clicks with the frame offset, external links, Escape order and the EPUB lockdown; swap `#hl-toggle` for `#toc-toggle`. Satisfies **AC-6**, **AC-14**, **AC-15**.
6. Contents panel (`ui/toc.js`, `#toc-panel`) with the Contents and Search tabs. Satisfies **AC-10**.
7. Protection check and error handling for damaged and protected books, in the reader and the probe. Satisfies **AC-2**, **AC-12**.
8. Check every criterion in the real app with test EPUBs: one with a cover, one without, one fixed layout, one right to left, one with a long last chapter, one damaged, one with DRM style `encryption.xml`, one with a script, a remote image and an `ftp:` link. Satisfies **AC-1** to **AC-15**.

## Consequences

**Positive**:
- One shelf for PDF and EPUB, with the same exact resume.
- No bundler, no npm and no new Rust dependency; foliate-js sits beside PDF.js under the same pinned vendor rule.
- CFI positions survive font size, layout and window changes, which page numbers could not.

**Negative / tradeoffs**:
- foliate-js has no tagged releases, so updates are a deliberate commit bump and a manual check.
- The CSP now allows inline styles (needed for EPUB chapters). Scripts stay blocked, so the risk is limited to how a book is styled.
- No highlights or notes in EPUBs until the follow-up.
- Copy protected store books can't be read; audio and video in books don't play.
- Paper and Night override all book colors, so books that use color for meaning lose it in those themes (Original keeps it).
- Search runs through the whole book on each query, which can take a few seconds on very long books.

**Neutral**:
- Two new settings and two new book fields; old data loads with defaults.

## Follow-up

- [ ] EPUB highlights and notes with the Obsidian export (a new issue; anchors as CFI ranges instead of PDF boxes).
- [ ] Apply the extra lockdown keys (context menu, reload, devtools) to the PDF reader too (a new issue; v3 leaves PDF unchanged).
- [ ] Show the author on cards or in the export, if wanted.
- [ ] More reading settings (font choice, line spacing, page width), if missed in use.
- [ ] Update `AGENTS.md` and `ui/AGENTS.md` for foliate-js and the CSP change (`/sync` after the build).
