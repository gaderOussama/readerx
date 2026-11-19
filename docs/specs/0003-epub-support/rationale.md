# 0003. ReaderX v3: decision record

Build spec: [index.md](index.md).

## Context

v1 (spec 0001) gives a shelf and exact resume for PDFs, and v2 (spec 0002) adds highlights and notes. v1 was designed so EPUB could fit in later: the book id is a hash of the file's content, `format` is a field, and `position` is a tagged object with an `epub` kind planned.

An EPUB is a zip of web pages (XHTML, CSS, images) plus a package file with metadata and a table of contents. Unlike a PDF, its text reflows: there are no fixed pages, so a page number changes with the font size and the window. The standard way to point at a spot is a CFI, a path through the book's structure that stays valid when the layout changes.

The forces are the ones the project set: plain JavaScript with no bundler and no npm, a Rust core with as few dependencies as possible, everything local with no network, a strict content security policy (CSP, the rules for what a page may load and run), and a calm monochrome interface where covers are the only color.

## Options considered

### Renderer

**Option A: foliate-js (chosen).** The engine of the Foliate reader, written as plain ES modules.
- Pros: loads with no bundler, like the vendored PDF.js; paginated and scrolled layouts, fixed layout books, table of contents, search and CFI built in; actively maintained.
- Cons: no tagged releases, so we pin a commit; its API is documented mostly by its source and its demo reader.

**Option B: epub.js.** The long standing EPUB library.
- Pros: well known, many examples.
- Cons: slow maintenance, an older design with known quirks on EPUB 3, and it needs its bundled build plus JSZip.

**Option C: Readium ts-toolkit.** The toolkit behind large reading apps.
- Pros: strong standards support.
- Cons: needs npm and a bundler, which the project avoids; far larger to integrate.

### Where cover and metadata are read

**Option A: the screen, in the existing cover queue (chosen).**
- Pros: reuses the probe queue and the `set_meta` and `save_cover` commands; one EPUB parser in the app; no new Rust dependency.
- Cons: a new EPUB shows a title tile until the background probe reaches it.

**Option B: Rust, with zip and XML crates during the scan.**
- Pros: metadata is ready at the first shelf paint.
- Cons: two new dependencies and a second EPUB parser to keep in step with the reader.

### Position for resume

**Option A: CFI plus fraction plus chapter label (chosen).**
- Pros: the CFI gives the exact spot across layouts; the fraction gives progress, the finished rule and a fallback when the CFI fails; the label lets the shelf show the chapter without opening the book.
- Cons: three fields to keep in step.

**Option B: CFI only.**
- Pros: one field.
- Cons: the shelf would have to open every EPUB to show progress or the chapter.

## Rationale

foliate-js is the only option that keeps the project's no bundler rule while covering what was asked: pages and scroll, fixed layout books, a table of contents and search. Using it directly as an element in the reader page (not inside a viewer iframe, as with PDF.js) keeps one session model, and its events (`relocate`, `load`, `link`) map onto the hooks v1 already has for saving and the lockdown.

Reading metadata in the screen keeps Rust free of EPUB parsing and reuses the cover queue that already handles failures and pausing. The cost, a title tile for a few seconds on a new book, is the same as for PDFs today.

The CSP has to allow inline styles, because EPUB chapters are styled web pages; without it most books lose their formatting. Scripts stay blocked, which is the part that matters for safety, and nothing remote can load because `connect-src`, `img-src` and `font-src` name no remote hosts.

The 98% finished rule was the owner's choice: many EPUBs end with notes, an index or ads, so reaching the last screen is a poor signal. A book is marked finished once and stays finished, as PDFs do.

A cross check against the code (2026-11-19) changed several details before the build: a stored `coverChecked` flag so coverless books are not probed forever; an explicit copy protection check, because foliate-js opens a DRM book as blank pages instead of failing; `view.init` with a fraction fallback instead of a timer; an `atEnd` signal beside 98%, because a long last chapter can stay under 0.98; one key handler on the reader page as well as in the book sections, because focus moves to the reader bar after a button click; and an exact list of lockdown keys for EPUB, because the v1 lockdown never covered reload, devtools or the context menu. PDFs keep their v1 lockdown; extending it is a separate follow up so v3 does not change PDF behavior.

Highlights for EPUB are left to a follow-up so v3 stays small. Their anchors (CFI ranges) and drawing differ from the PDF boxes of spec 0002, and doing both at once would roughly double v3.
