pub mod derive;
pub mod library;
pub mod ops;
pub mod scan;

use chrono::Utc;
use derive::{state_view, BookView, StateView};
use library::{lock, Position, Store};
use std::path::Path;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};

struct Shared(Mutex<Store>);

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
    page_count: i64,
) -> Result<ops::Saved, String> {
    ops::save_position(&mut lock(&state.0), &id, position, page_count, Utc::now())
}

#[tauri::command(async)]
fn set_meta(
    state: State<Shared>,
    id: String,
    meta_title: Option<String>,
    page_count: i64,
) -> Result<(), String> {
    ops::set_meta(&mut lock(&state.0), &id, meta_title, page_count)
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
            if let Some(folder) = &store.lib.settings.books_folder {
                allow_dir(app.handle(), Path::new(folder));
            }
            app.manage(Shared(Mutex::new(store)));
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running ReaderX");
}
