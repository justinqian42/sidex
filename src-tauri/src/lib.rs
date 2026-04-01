mod commands;

use commands::storage::StorageDb;
use commands::terminal::TerminalStore;
use std::sync::Arc;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .manage(Arc::new(TerminalStore::new()))
        .setup(|app| {
            let app_data = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");
            std::fs::create_dir_all(&app_data).ok();
            let db_path = app_data.join("sidex_storage.db");
            let db = StorageDb::new(db_path.to_str().unwrap())
                .expect("failed to initialize storage database");
            app.manage(Arc::new(db));

            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // fs commands
            commands::read_file,
            commands::read_file_bytes,
            commands::write_file,
            commands::write_file_bytes,
            commands::read_dir,
            commands::stat,
            commands::mkdir,
            commands::remove,
            commands::rename,
            commands::exists,
            // terminal commands
            commands::terminal_spawn,
            commands::terminal_write,
            commands::terminal_resize,
            commands::terminal_kill,
            // search commands
            commands::search_files,
            commands::search_text,
            // window commands
            commands::create_window,
            commands::close_window,
            commands::set_window_title,
            commands::get_monitors,
            // os commands
            commands::get_os_info,
            commands::get_env,
            commands::get_shell,
            // storage commands
            commands::storage_get,
            commands::storage_set,
            commands::storage_delete,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
