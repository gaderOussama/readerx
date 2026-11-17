# 0002. ReaderX v2: decision record

Build spec: [index.md](index.md).

## Context

v1 (spec 0001, Accepted) gives one click resume and a shelf with progress. The reader now wants to keep what they read: mark passages, add a thought to some of them, and have those land in their Obsidian vault, where the rest of their notes live. Export is the main payoff; marks inside the app are what makes it possible.

The forces are the ones v1 set. A book's identity is a hash of the first 64 KiB of its file plus its size, so anything that changes the PDF file changes its id and loses its progress. The reader runs the pinned PDF.js 6.5.438 prebuilt viewer in an iframe, and v1 relies only on its public events and options so that an upgrade stays possible. Storage is plain files in the app data folder, saved atomically. The interface is calm and monochrome. Everything is local, with no network.

An Obsidian vault is a folder of Markdown files that the user also edits, and it may be synced to other devices. Whatever ReaderX writes there has to live alongside the user's own edits without destroying them.

If nothing is decided, the build would have to guess where highlights live, how they are drawn, and how the export treats files the user changed. Each of those is hard to change once highlights exist.

## Options considered

### Drawing and capturing highlights

**Option A: our own thin layer (chosen).** We read the selection from the PDF.js text layer, store boxes in PDF page units, and draw them in our own layer on every `pagerendered` event.
- Pros: uses only public events; we control the storage format, the look and the merge rule; small (about 150 lines).
- Cons: we own the selection to box math, including line merging and odd page rotations.

**Option B: the PDF.js built-in highlight editor.** PDF.js 6.5 ships a highlight tool with a toolbar, colors and free drawing.
- Pros: a polished editor for free, including drawn highlights.
- Cons: it is built to save into the PDF; reloading highlights from our own JSON goes through internal classes that can change between versions; its toolbar and colors clash with the calm, single color design.

### Where highlights are stored

**Option A: one JSON file per book in the app data folder (chosen).**
- Pros: keyed by book id, so renames and moves keep them; `library.json` stays small; one book's save never rewrites another's.
- Cons: one more folder to back up.

**Option B: inside `library.json`.**
- Pros: a single file holds everything.
- Cons: every highlight or note keystroke save rewrites the whole library; the file grows with every note.

**Option C: a sidecar file next to the PDF.**
- Pros: highlights travel with the book file.
- Cons: clutters the books folder; breaks on rename, which the book id model was built to survive.

**Option D: inside the PDF.**
- Pros: visible in every PDF reader.
- Cons: changes the file and so the book id, which loses progress; risks damaging the user's files.

### How the export handles user edits

**Option A: rewrite only a marked block and ReaderX's own properties (chosen).**
- Pros: the vault file can hold the user's own notes and properties next to the export.
- Cons: more logic (property merge, marker handling) and more tests.

**Option B: ReaderX owns the whole file.**
- Pros: the simplest export.
- Cons: any edit in Obsidian is silently lost on the next highlight.

## Rationale

The book id rule decides most of this. Since the id is a hash of the file, anything written into the PDF would orphan the book's progress. That rules out storing highlights in the PDF, and it makes the PDF.js editor a poor fit, because its natural save path is the PDF. Our own layer keeps to the same discipline v1 used for the reader (public events only, pinned version) and stays small. Storing per book files in the app data folder reuses the v1 atomic save and keys everything by the id that already survives renames and moves.

The export is where the user's real work ends up, so it has to be safe to run often. Rewriting only a marked block and ReaderX's own properties lets the vault file be the user's note too. It also lets the export run after every change, which is what keeps the vault current with no export button. A name clash is settled by the `readerx` property rather than the file name alone, so two books with the same title, or a note the user already had, are never overwritten.

The export runs on a background worker, after the highlight is already saved, because the vault may sit on a slow, synced or removable drive. Running it inside the command would let an unplugged drive freeze Back and window close. A separate export on leave was dropped: every change already exports, and the only values it would refresh (`progress` and `last-read`) are allowed to lag until the next change or the next open of the book.

The highlight tint is the one deliberate exception to the monochrome rule. A gray tint was considered, but on a printed page it reads as dirt or a scan shadow rather than a mark, and the owner asked for one soft color.

A cross check by a second model before acceptance found missing decisions (merge details, selection math, YAML and marker safety, file name edge cases, concurrency, key handling). They were settled in `index.md` with the owner's approval.
