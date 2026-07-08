mod commands;
mod domain;
mod pdf_extraction;
mod pdf_ingestion;
mod services;
mod shared;
mod storage;

use pdf_extraction::{PdfExtractionConfig, PdfExtractionManager};
use pdf_ingestion::{PdfDownloadManager, PdfIngestionConfig};
use services::chat::ChatService;
use services::reader_service::ReaderService;
use services::research::manager::SearchManager;
use services::source_acquisition::SourceAcquisitionService;
use storage::library_store::LibraryStore;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let store = LibraryStore::new(&app.handle()).map_err(std::io::Error::other)?;
            store.init().map_err(std::io::Error::other)?;
            let extraction_config = PdfExtractionConfig::load(&app.handle());
            let pdf_extractions =
                PdfExtractionManager::new(app.handle().clone(), store.clone(), extraction_config);
            let source_client = reqwest::Client::builder()
                .user_agent(concat!(
                    env!("CARGO_PKG_NAME"),
                    "/",
                    env!("CARGO_PKG_VERSION")
                ))
                .build()
                .expect("reqwest client should build");
            let source_acquisition =
                SourceAcquisitionService::from_app_config(&app.handle(), source_client);
            let pdf_config = PdfIngestionConfig::load(&app.handle());
            let pdf_downloads = PdfDownloadManager::new(
                app.handle().clone(),
                store.clone(),
                pdf_config,
                pdf_extractions.clone(),
                source_acquisition.clone(),
            );
            pdf_downloads
                .recover_and_queue_startup_downloads()
                .map_err(std::io::Error::other)?;
            pdf_extractions
                .recover_and_queue_startup_extractions()
                .map_err(std::io::Error::other)?;
            let reader_service = ReaderService::new(
                app.handle().clone(),
                store.clone(),
                source_acquisition.clone(),
            );
            let chat_service = ChatService::from_app_config(
                app.handle().clone(),
                store.clone(),
                reader_service.clone(),
            )
            .map_err(std::io::Error::other)?;
            let discovery_providers = commands::discovery::DiscoveryProviders::from_app_config()
                .map_err(std::io::Error::other)?;
            let search_manager = SearchManager::new(app.handle().clone(), store.clone());
            search_manager
                .recover_and_queue_startup_runs()
                .map_err(std::io::Error::other)?;
            app.manage(store);
            app.manage(pdf_downloads);
            app.manage(pdf_extractions);
            app.manage(source_acquisition);
            app.manage(reader_service);
            app.manage(chat_service);
            app.manage(discovery_providers);
            app.manage(search_manager);
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
            commands::reader::get_reader_document,
            commands::reader::get_discovery_reader_document,
            commands::reader::get_reader_pdf_bytes,
            commands::reader::extract_paper_document,
            commands::chat::list_chat_threads,
            commands::chat::get_chat_thread,
            commands::chat::add_note_at_anchor,
            commands::chat::ask_at_anchor_streamed,
            commands::chat::add_chat_note,
            commands::chat::ask_chat_thread,
            commands::chat::ask_chat_thread_streamed,
            commands::chat::set_chat_entry_pinned,
            commands::chat::list_pinned_chat_entries,
            commands::chat::rename_chat_thread,
            commands::chat::delete_chat_thread,
            commands::research::create_search,
            commands::research::list_searches,
            commands::research::get_search,
            commands::research::list_search_candidates,
            commands::research::run_search,
            commands::research::cancel_search_run,
            commands::research::mark_search_candidate_saved,
            commands::research::mark_search_candidates_seen,
            commands::source_acquisition::debug_obscura_start,
            commands::source_acquisition::debug_obscura_fetch,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
