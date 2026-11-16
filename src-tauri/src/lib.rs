pub mod derive;
pub mod library;
pub mod scan;

pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running ReaderX");
}
