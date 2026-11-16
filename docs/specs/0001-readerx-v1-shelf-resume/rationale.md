# 0001. ReaderX v1: decision record

Build spec: [index.md](index.md).

## Context

> ⚠️ Premise note: the core pain (reopening a PDF on the right page) is already solved by free readers. SumatraPDF remembers page and zoom per file and shows recent books on its start screen. Building ReaderX anyway is a conscious choice. It is a personal tool and a learning project, with a shelf that shows progress, and with notes (v2) and EPUB (v3) planned. The risk is building for weeks what a 10 MB install already does, so v1 is kept to exactly the shelf and resume, and anything else waits until real use shows it is missed.

The engineer reads books as PDFs on a Windows PC. Each session costs several steps: open File Explorer, find the folder, find the file, open it, then scroll to find the last page reached. The place is often lost, and the friction makes reading less likely to happen at all. The job of the app is to make "continue reading" one click and to make progress across books visible.

Forces: one user, one Windows PC, no sync, books gathered in one folder (with subfolders). The engineer wants to build it in Rust and wants a minimal monochrome look that is easy on the eyes during long reading. Node 22 is installed; the Rust toolchain is not. The design must not block two planned additions: highlights and notes (v2) and EPUB books (v3).

If nothing is decided, the first build will invent a book identity, a storage format and a reader approach on the fly, and each of those is hard to change once progress data exists.

## Options considered

### Option 1: Electron with the PDF.js viewer

A JavaScript desktop app (Chromium plus Node bundled together) with the same PDF.js viewer, and the library file handled in Node.

**Pros**:
- Fastest start: Node is already installed, one language everywhere.
- Huge community and examples for exactly this kind of app.

**Cons**:
- Not Rust, which the engineer explicitly asked for.
- Installer around 100 MB and higher memory use for a simple shelf.

### Option 2: Tauri 2 with the PDF.js viewer (chosen)

A Rust core owns files, scanning, watching and saving. The screen is plain HTML/CSS/JS in the system WebView2 and embeds the prebuilt PDF.js viewer.

**Pros**:
- Rust, as requested, for all the logic that matters (identity, scan, state rules), which is testable with `cargo test`.
- Small installer and low memory, because it uses the WebView2 already in Windows.
- Keeps the full PDF.js viewer (search, zoom, outline, text selection, highlight editor for v2) at no extra cost.

**Cons**:
- One time setup of Rust and the C++ Build Tools, plus slow first compiles.
- Two languages, and a command boundary between them to design and keep in sync.

### Option 3: Pure native Rust (egui or iced, with PDFium for rendering)

No web view at all. Pages are rendered by PDFium through `pdfium-render` and drawn in a native Rust interface.

**Pros**:
- One language, a fully custom look, no browser engine.

**Cons**:
- Zoom, smooth scrolling, text search, text selection and later highlights must all be built by hand. That is many times the v1 effort.
- A PDFium DLL must be shipped and kept updated.

### Option 4: Do not build; use SumatraPDF

Install the existing free reader, which already remembers positions and lists recent files.

**Pros**:
- Solves the main pain today, with zero code.

**Cons**:
- No real shelf with progress or sections, no path to in app notes, no custom look, and nothing learned from building it.

## Rationale

The engineer chose to build, and chose Rust. That rules out Option 4 by intent and Option 1 by language, although Option 1 would honestly be the quickest path, given that Node is already installed. Between the two Rust options the deciding force is the reader itself. A comfortable PDF reader (smooth zoom, search, text selection, and later highlights) is the hardest part of the whole app, and PDF.js already is one. Option 3 would spend most of the effort rebuilding it. Option 2 puts Rust where it adds value (the identity, scan, watch and save rules, all unit testable) and reuses a mature reader for the part users touch most. The cost is the toolchain setup and a two language boundary. That boundary is kept small: eight commands and one event.

Inside Option 2, every smaller choice leans to the fewest moving parts. One JSON file instead of SQLite, because a few hundred books is a tiny file and SQLite adds a native build step. A plain HTML/CSS screen with no framework and no bundler, because the shelf is one page. A full rescan on every folder change instead of incremental tracking, because hashing 64 KiB per file is cheap. Book identity is a hash of the file's first 64 KiB plus its size rather than the path, because the engineer wants renames and moves to keep progress, and a path based key cannot do that. A hash of the whole file would be more exact, but it reads entire books on every scan. The prebuilt PDF.js viewer is pinned to one version so the page recoloring and event names do not shift under the app.

The interface follows the page theme instead of a separate Windows light/dark setting. That keeps one switch and guarantees the shelf and the pages never clash (a bright shelf next to a dark page is exactly the eye strain the engineer wants to avoid). Covers stay in color because they are how books are recognized at a glance. Everything else is gray.
