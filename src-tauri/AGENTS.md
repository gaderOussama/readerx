# Rust core (src-tauri)

## Overview

Owns everything that touches the disk: the library file, book ids, the folder scan, the folder watcher and the commands the screen calls. Kept testable by putting logic in plain functions and keeping the Tauri commands thin.

## Key files

| File | Owns |
|---|---|
| `src/library.rs` | data model, `Store` (one mutex), atomic save, corrupt file set aside, `is_valid_id` |
| `src/scan.rs` | fingerprint (book id), hash cache, recursive walk, merge into the library |
| `src/derive.rs` | sections, display title, Continue choice, ordering, `StateView` for the screen |
| `src/ops.rs` | command logic as plain functions (finished rule, position checks, covers, meta) |
| `src/watch.rs` | folder watcher thread and its pure decision rules |
| `src/lib.rs` | thin `#[tauri::command(async)]` wrappers, asset scope, plugins, startup |
| `tauri.conf.json`, `capabilities/default.json` | window, CSP, installer, allowed commands |

## Conventions

- New command: logic in `ops.rs` with a unit test, a thin wrapper in `lib.rs`, and an `allow-*` entry in `capabilities/default.json`.
- Errors cross to the screen as short snake case codes (`unknown_book`, `bad_position`, `bad_image`, `not_a_folder`, `not_present`, `bad_theme`, `cover_write_failed`, `dim_failed`), never as prose.
- Take the store with `library::lock`; never hold it while walking or hashing files (`scan::run` snapshots, works unlocked, then merges).
- Tests use `tempfile` folders and fixed timestamps, never the real AppData or the real watcher.

## Gotchas

- Book id = sha256 of the first 64 KiB + `-` + file size, so renames and moves keep progress. An exact copy of a file is the same book.
- Check any id with `is_valid_id` before using it in a path (covers), so nothing escapes the covers folder.
- Saves go through `.tmp`, `sync_all`, then rename with retries; an unreadable `library.json` is renamed `library.corrupt-<UTC stamp>.json`, never deleted.
- A file modified less than 2 s ago is skipped and rescanned 1.6 s later (it may still be copying).
- A vanished file or folder never changes or deletes a book record; it only hides it.
- The asset protocol scope is not remembered: it is re-allowed at every start and on folder change. Never `forbid_directory`.
- Renaming the watched folder itself sends no event on Windows; the watcher notices it on its 10 s check.
- Builds are slow (a cold release build is about 14 min); reuse a warm `CARGO_TARGET_DIR` when you can.

## Related specs

- [docs/specs/0001-readerx-v1-shelf-resume/index.md](../docs/specs/0001-readerx-v1-shelf-resume/index.md)

_Drafted by /audit from the repo, worth a quick human pass. Edit freely: once a line stops matching this draft, later runs treat it as curated and will flag rather than overwrite it._
