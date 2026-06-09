mod commands;
mod domain;
mod pdf_ingestion;
mod services;
mod storage;

use pdf_ingestion::{PdfDownloadManager, PdfIngestionConfig};
use services::reader_service::ReaderService;
use storage::library_store::LibraryStore;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let store = LibraryStore::new(&app.handle()).map_err(std::io::Error::other)?;
            store.init().map_err(std::io::Error::other)?;
            let pdf_config = PdfIngestionConfig::load(&app.handle());
            let pdf_downloads =
                PdfDownloadManager::new(app.handle().clone(), store.clone(), pdf_config);
            pdf_downloads
                .recover_and_queue_startup_downloads()
                .map_err(std::io::Error::other)?;
            let reader_service = ReaderService::new(store.clone());
            let discovery_service = commands::discovery::AppDiscoveryService::new(
                commands::discovery::providers::openalex::OpenAlexProvider::from_app_config()
                    .map_err(std::io::Error::other)?,
            );
            app.manage(store);
            app.manage(pdf_downloads);
            app.manage(reader_service);
            app.manage(discovery_service);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::vault::get_vault_status,
            commands::discovery::search_papers,
            commands::library::get_library,
            commands::library::add_paper_to_vaults,
            commands::library::download_paper_pdf,
            commands::library::get_document_sources,
            commands::library::create_vault,
            commands::library::rename_vault,
            commands::library::delete_vault,
            commands::library::remove_paper_from_vault,
            commands::library::delete_paper_globally,
            commands::library::get_paper_notes,
            commands::library::create_paper_note,
            commands::library::delete_paper_note,
            commands::library::update_paper_note,
            commands::reader::get_reader_document,
            commands::reader::get_reader_pdf_bytes,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
