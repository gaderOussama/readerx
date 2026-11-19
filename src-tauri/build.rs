fn main() {
    // Listing the commands makes each one need its own `allow-*` permission
    // in a capability, so the window can call only these.
    let app = tauri_build::AppManifest::new().commands(&[
        "get_state",
        "choose_folder",
        "dismiss_notice",
        "open_book",
        "save_position",
        "set_meta",
        "save_cover",
        "set_finished",
        "set_page_theme",
        "set_epub_settings",
        "set_dim_desktop",
        "dim_desktop",
        "get_highlights",
        "add_highlights",
        "set_note",
        "delete_highlight",
        "restore_highlight",
        "forget_undo",
        "set_export_folder",
    ]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(app))
        .expect("failed to run tauri-build");
}
