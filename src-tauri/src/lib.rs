pub mod derive;
pub mod export;
pub mod highlights;
pub mod library;
pub mod ops;
pub mod scan;
pub mod watch;

use chrono::Utc;
use derive::{state_view, BookView, StateView};
use highlights::{Books, Ctx, Highlight, Item};
use library::{lock, Position, Store};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};

struct Shared(Mutex<Store>);

/// The running folder watcher. Replacing it drops, and so stops, the old one.
struct WatchSlot(Mutex<Option<watch::Watcher>>);

/// Book ids for the export worker, which writes the vault files in the background.
struct Exporter(Sender<String>);

impl Exporter {
    fn queue(&self, id: &str) {
        let _ = self.0.send(id.to_string());
    }
}

/// Watch `folder`; every rescan it makes ends in a `library-changed` event.
fn restart_watch(app: &AppHandle, folder: &str) {
    let (scan_app, emit_app) = (app.clone(), app.clone());
    let w = watch::start(
        PathBuf::from(folder),
        move || {
            let s = scan_app.state::<Shared>();
            let too_new = scan::run(&s.0, Utc::now(), &mut scan::fingerprint);
            let readable = lock(&s.0).folder_readable;
            (too_new, readable)
        },
        move || {
            let _ = emit_app.emit("library-changed", ());
        },
    );
    *app.state::<WatchSlot>().0.lock().unwrap_or_else(|e| e.into_inner()) = Some(w);
}

/// The asset scope is not remembered between runs, so every start and every
/// folder change re-allows the folders. Never `forbid_directory`: it sticks
/// until restart even if the folder is chosen again.
fn allow_dir(app: &AppHandle, dir: &Path) {
    if let Err(e) = app.asset_protocol_scope().allow_directory(dir, true) {
        eprintln!("readerx: asset scope for {}: {e}", dir.display());
    }
}

// `(async)` runs a command off the main thread: a scan walks the disk.

#[tauri::command(async)]
fn get_state(state: State<Shared>) -> Result<StateView, String> {
    scan::run(&state.0, Utc::now(), &mut scan::fingerprint);
    Ok(state_view(&lock(&state.0)))
}

#[tauri::command(async)]
fn choose_folder(app: AppHandle, state: State<Shared>, path: String) -> Result<StateView, String> {
    ops::set_folder(&mut lock(&state.0), &path)?;
    allow_dir(&app, Path::new(&path));
    restart_watch(&app, &path);
    scan::run(&state.0, Utc::now(), &mut scan::fingerprint);
    Ok(state_view(&lock(&state.0)))
}

#[tauri::command(async)]
fn dismiss_notice(state: State<Shared>) -> Result<(), String> {
    ops::dismiss_notice(&mut lock(&state.0));
    Ok(())
}

#[tauri::command(async)]
fn open_book(state: State<Shared>, id: String) -> Result<ops::Opened, String> {
    ops::open_book(&mut lock(&state.0), &id, Utc::now())
}

#[tauri::command(async)]
fn save_position(
    state: State<Shared>,
    id: String,
    position: Position,
    page_count: Option<i64>,
) -> Result<ops::Saved, String> {
    ops::save_position(&mut lock(&state.0), &id, position, page_count, Utc::now())
}

#[tauri::command(async)]
fn set_meta(
    state: State<Shared>,
    id: String,
    meta_title: Option<String>,
    meta_author: Option<String>,
    page_count: Option<i64>,
) -> Result<(), String> {
    ops::set_meta(&mut lock(&state.0), &id, meta_title, meta_author, page_count)
}

#[tauri::command(async)]
fn save_cover(state: State<Shared>, id: String, png: Vec<u8>) -> Result<(), String> {
    ops::save_cover(&lock(&state.0), &id, &png)
}

#[tauri::command(async)]
fn set_finished(state: State<Shared>, id: String, finished: bool) -> Result<BookView, String> {
    ops::set_finished(&mut lock(&state.0), &id, finished, Utc::now())
}

#[tauri::command(async)]
fn set_page_theme(state: State<Shared>, theme: String) -> Result<(), String> {
    ops::set_page_theme(&mut lock(&state.0), &theme)
}

#[tauri::command(async)]
fn set_epub_settings(state: State<Shared>, font_size: i64, layout: String) -> Result<(), String> {
    ops::set_epub_settings(&mut lock(&state.0), font_size, &layout)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HighlightsOut {
    highlights: Vec<Highlight>,
    export_folder: Option<String>,
    export_error: Option<String>,
    notice: Option<bool>,
}

#[derive(Serialize)]
struct HighlightList {
    highlights: Vec<Highlight>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportFolder {
    export_folder: String,
}

#[tauri::command(async)]
fn get_highlights(
    state: State<Shared>,
    books: State<Books>,
    exporter: State<Exporter>,
    id: String,
) -> Result<HighlightsOut, String> {
    let (dir, pages) = ops::highlight_book(&lock(&state.0), &id)?;
    let (export_folder, export_error) = {
        let s = lock(&state.0);
        (s.lib.settings.export_folder.clone(), s.lib.settings.export_errors.get(&id).cloned())
    };
    let listing = highlights::get(&Ctx::new(&dir, &id, pages)?, &books, Utc::now());
    if export_error.is_some() {
        exporter.queue(&id);
    }
    Ok(HighlightsOut {
        highlights: listing.highlights,
        export_folder,
        export_error,
        notice: listing.notice,
    })
}

#[tauri::command(async)]
fn add_highlights(
    state: State<Shared>,
    books: State<Books>,
    exporter: State<Exporter>,
    id: String,
    items: Vec<Item>,
) -> Result<HighlightList, String> {
    let (dir, pages) = ops::highlight_book(&lock(&state.0), &id)?;
    let highlights = highlights::add(&Ctx::new(&dir, &id, pages)?, &books, items, Utc::now())?;
    exporter.queue(&id);
    Ok(HighlightList { highlights })
}

#[tauri::command(async)]
fn set_note(
    state: State<Shared>,
    books: State<Books>,
    exporter: State<Exporter>,
    id: String,
    highlight_id: String,
    note: String,
) -> Result<HighlightList, String> {
    let (dir, pages) = ops::highlight_book(&lock(&state.0), &id)?;
    let ctx = Ctx::new(&dir, &id, pages)?;
    let highlights = highlights::set_note(&ctx, &books, &highlight_id, &note, Utc::now())?;
    exporter.queue(&id);
    Ok(HighlightList { highlights })
}

#[tauri::command(async)]
fn delete_highlight(
    state: State<Shared>,
    books: State<Books>,
    exporter: State<Exporter>,
    id: String,
    highlight_id: String,
) -> Result<HighlightList, String> {
    let (dir, pages) = ops::highlight_book(&lock(&state.0), &id)?;
    let ctx = Ctx::new(&dir, &id, pages)?;
    let highlights = highlights::delete(&ctx, &books, &highlight_id, Utc::now())?;
    exporter.queue(&id);
    Ok(HighlightList { highlights })
}

#[tauri::command(async)]
fn restore_highlight(
    state: State<Shared>,
    books: State<Books>,
    exporter: State<Exporter>,
    id: String,
) -> Result<HighlightList, String> {
    let (dir, pages) = ops::highlight_book(&lock(&state.0), &id)?;
    let highlights = highlights::restore(&Ctx::new(&dir, &id, pages)?, &books, Utc::now())?;
    exporter.queue(&id);
    Ok(HighlightList { highlights })
}

#[tauri::command(async)]
fn forget_undo(books: State<Books>, id: String) -> Result<(), String> {
    books.forget_undo(&id);
    Ok(())
}

#[tauri::command(async)]
fn set_export_folder(
    state: State<Shared>,
    books: State<Books>,
    exporter: State<Exporter>,
    path: String,
) -> Result<ExportFolder, String> {
    let ids = export::set_folder(&state.0, &books, &path, Utc::now())?;
    for id in &ids {
        exporter.queue(id);
    }
    Ok(ExportFolder { export_folder: path })
}

pub fn run() {
    tauri::Builder::default()
        // single-instance must be registered first
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(
            // VISIBLE excluded: the window starts hidden and the screen shows it after first paint
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::all()
                        - tauri_plugin_window_state::StateFlags::VISIBLE,
                )
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(dir.join("covers"))?;
            let store = Store::open(&dir, Utc::now());
            allow_dir(app.handle(), &store.covers_dir());
            let folder = store.lib.settings.books_folder.clone();
            if let Some(folder) = &folder {
                allow_dir(app.handle(), Path::new(folder));
            }
            app.manage(Shared(Mutex::new(store)));
            app.manage(Books::default());
            let job_app = app.handle().clone();
            app.manage(Exporter(export::spawn(move |id| {
                let (shared, books) = (job_app.state::<Shared>(), job_app.state::<Books>());
                if let Some(export_error) = export::export_book(&shared.0, &books, id, Utc::now()) {
                    let _ = job_app.emit(
                        "highlights-exported",
                        serde_json::json!({ "id": id, "exportError": export_error }),
                    );
                }
            })));
            app.manage(WatchSlot(Mutex::new(None)));
            if let Some(folder) = &folder {
                restart_watch(app.handle(), folder);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            choose_folder,
            dismiss_notice,
            open_book,
            save_position,
            set_meta,
            save_cover,
            set_finished,
            set_page_theme,
            set_epub_settings,
            get_highlights,
            add_highlights,
            set_note,
            delete_highlight,
            restore_highlight,
            forget_undo,
            set_export_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ReaderX");
}
