# ReaderX

A Windows PDF bookshelf that reopens every book where you left it. Built with Tauri 2 (Rust core in `src-tauri/`, plain HTML/CSS/JS in `ui/`).

## Prerequisites

- Rust (stable) and the Windows C++ build tools
- WebView2 runtime (already on Windows 10 and 11)
- Tauri CLI: `cargo install tauri-cli --version "^2"`

## Run

```
cd src-tauri
cargo tauri dev
```

## Build the installer

```
cd src-tauri
cargo tauri build
```

The per-user NSIS installer lands in `src-tauri/target/release/bundle/nsis/` as `ReaderX_<version>_x64-setup.exe`. It adds a Start menu shortcut and offers a desktop shortcut on the last page. The first build downloads the NSIS tooling.

## Tests

```
cd src-tauri && cargo test
node --test "ui/tests/*.test.mjs"
```
