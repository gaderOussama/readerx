# ReaderX

A small Windows app: a shelf of every PDF in one folder, and a reader that reopens each book at the exact page, scroll spot and zoom. Single user, local files only, no network.

## Stack

- **Language / Runtime**: Rust 2021 (core) and plain JavaScript ES modules (screen), in the system WebView2
- **Framework**: Tauri 2, with no frontend framework and no bundler (`app.withGlobalTauri: true`)
- **Key dependencies**: PDF.js 6.5.438 prebuilt viewer (vendored in `ui/vendor/pdfjs/`), `notify-debouncer-mini`, `sha2`, Tauri plugins dialog, opener, window-state, single-instance
- **Package manager**: cargo (no npm; Node is only used to run the UI tests)
- **Storage**: one `library.json` plus `covers/<id>.png` in `%APPDATA%\com.readerx.desktop`

## Build approach

Tracer Bullet: thin end to end slices through Rust and the screen, then thicken (from the build plan in spec 0001; no scope header yet).

## Commands

```bash
# Dev (from src-tauri/)
cargo tauri dev

# Build the NSIS installer (lands in src-tauri/target/release/bundle/nsis/)
cargo tauri build

# Test
cd src-tauri && cargo test
node --test "ui/tests/*.test.mjs"   # the glob is required; a folder path fails on Node 22
```

## Specs

Stored in `docs/specs/`. v1 is `docs/specs/0001-readerx-v1-shelf-resume/` (`index.md` to build from, `rationale.md` for why). Acceptance criteria are named AC-1 to AC-17.

## Rules

- Smallest change that works: no new dependency, framework or abstraction unless the spec asks for it.
- Rust owns files, ids, scanning, watching and saving; the screen only displays and calls commands.
- Logic worth testing lives in plain functions with unit tests (`src-tauri/src/*.rs` tests, `ui/lib/*.js` with `ui/tests/*.test.mjs`); write the test first.
- The interface is monochrome (palette CSS variables only); covers are the only color.
- Never send anything over the network; links in PDFs open in the system browser.

## Git

- integration: on, GitHub `gaderOussama/readerx`
- every task is a GitHub issue; branch `feat/<issue>-<name>` (or `chore/`, `docs/`, `spec/`) from `main`
- small commits, one change each, imperative subject; a PR per issue that says `Closes #N`
- `main` is protected: PR only, the CI check `test` must pass, no force push
- roadmap: pinned issue #12

## Agent skills

None installed. Declined: Agent Skills and MCP servers for Tauri and PDF.js (looked at and declined by the owner, 2026-11-16).

## Context files

- [src-tauri/AGENTS.md](src-tauri/AGENTS.md): Rust core, commands, library file, scan and watcher rules
- [ui/AGENTS.md](ui/AGENTS.md): shelf and reader screens, the PDF.js embedding and its gotchas

_Drafted by /audit from the repo, worth a quick human pass. Edit freely: once a line stops matching this draft, later runs treat it as curated and will flag rather than overwrite it._
