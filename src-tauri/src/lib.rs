mod commands;
mod domain;
mod storage;

use storage::library_store::LibraryStore;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let store = LibraryStore::new(&app.handle()).map_err(std::io::Error::other)?;
            store.init().map_err(std::io::Error::other)?;
            app.manage(store);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::vault::get_vault_status,
            commands::library::get_library,
            commands::library::add_paper_to_vaults,
            commands::library::create_vault,
            commands::library::rename_vault,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
