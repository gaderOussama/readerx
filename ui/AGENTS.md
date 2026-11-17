# Screen (ui)

## Overview

The shelf and the reader, as plain HTML, CSS and JavaScript modules with no bundler. The reader embeds the pinned PDF.js prebuilt viewer in an iframe; covers are made in the background with the PDF.js core library.

## Key files

| File | Owns |
|---|---|
| `app.js` | shelf: states, Continue card, sections, card menu, `library-changed` refresh |
| `reader.js` | reader: iframe viewer, saved position, flush on Back, Escape and close, theme switch, lockdown |
| `covers.js` | background cover probe queue (one book at a time, paused while reading) |
| `lib/*.js` + `tests/*.test.mjs` | pure logic with node tests (formatting, which books need a cover) |
| `viewer-theme.css` | recolors the PDF.js toolbar with the palette |
| `vendor/pdfjs/` | PDF.js 6.5.438, pinned; never edit, upgrade only on purpose |

## Commands

```bash
node --test "ui/tests/*.test.mjs"   # from the repo root; keep the quoted glob
```

## Conventions

- Tauri APIs come from `window.__TAURI__` (no imports from npm). Turn every file path into a URL with `convertFileSrc`.
- Build DOM with the small `h()` helper in `app.js`; colors only from the palette CSS variables (Paper and Night themes).
- Any async step in the reader re-checks that its session is still current (`cur`), because Back or a theme switch can replace it mid flight.

## Gotchas

- Set viewer options inside the `webviewerloaded` event, which fires on the parent `document` with `detail.source` = the iframe window.
- Open books with `PDFViewerApplication.open({ url })` after `initializedPromise`. Never use `?file=`: the viewer rejects a file from another origin.
- Save positions only after `isInitialViewSet` is true, debounced 1 s from `updateviewarea`, and flush on Back, Escape (listener on the iframe document) and `onCloseRequested`.
- Page colors are read only at viewer start, so a theme switch flushes and reloads the iframe.
- This PDF.js build only logs load errors; `reader.js` shows its own message for a damaged file.
- The CSP is strict (`tauri.conf.json`): no inline scripts, no remote anything. The cover probe uses `disableFontFace` because of it.
- `requestAnimationFrame` stalls while the window is minimized, so cover probes pause then too.

## Related specs

- [docs/specs/0001-readerx-v1-shelf-resume/index.md](../docs/specs/0001-readerx-v1-shelf-resume/index.md), "Reader mechanism" steps 1 to 8

_Drafted by /audit from the repo, worth a quick human pass. Edit freely: once a line stops matching this draft, later runs treat it as curated and will flag rather than overwrite it._
