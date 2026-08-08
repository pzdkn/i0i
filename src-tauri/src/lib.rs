mod commands;
mod domain;
mod html_ingestion;
mod pdf_extraction;
mod pdf_layout;
mod pdf_ingestion;
mod services;
mod shared;
mod storage;

use pdf_extraction::{PdfExtractionConfig, PdfExtractionManager};
use pdf_ingestion::{PdfDownloadManager, PdfIngestionConfig};
use services::chat::ChatService;
use services::metadata_enrichment::MetadataEnrichmentService;
use services::reader_service::ReaderService;
use services::research::manager::SearchManager;
use services::source_acquisition::SourceAcquisitionService;
use storage::library_store::LibraryStore;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Settings store first (RFC 0055): installed as the process-global
            // handle so every config/key resolver below — including the ones
            // that read model overrides at construction — sees user overrides.
            let settings_store = services::settings::SettingsStore::new(&app.handle())
                .map_err(std::io::Error::other)?;
            services::settings::init_global(settings_store.clone());

            let store = LibraryStore::new(&app.handle()).map_err(std::io::Error::other)?;
            store.init().map_err(std::io::Error::other)?;
            let highlight_service = services::highlight::HighlightService::new(store.clone());
            let extraction_config = PdfExtractionConfig::load(&app.handle());
            let pdf_extractions = PdfExtractionManager::new(
                app.handle().clone(),
                store.clone(),
                extraction_config.clone(),
            );
            // RFC 0051: reqwest's default has no request timeout at all, which
            // let a stalled publisher server hang PDF opens forever.
            let source_client = reqwest::Client::builder()
                .user_agent(concat!(
                    env!("CARGO_PKG_NAME"),
                    "/",
                    env!("CARGO_PKG_VERSION")
                ))
                .connect_timeout(std::time::Duration::from_secs(10))
                .timeout(std::time::Duration::from_secs(30))
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
            let metadata_enrichment = MetadataEnrichmentService::new(
                app.handle().clone(),
                store.clone(),
                extraction_config.clone(),
                discovery_providers.openalex.clone(),
                discovery_providers.arxiv.clone(),
            );
            // Embedding reranker (RFC 0054). Disabled unless the `embeddings`
            // feature is built in; ranking falls back to legacy weights. Built
            // before SearchManager so deep research can rank semantically too
            // (RFC 0057).
            let embedding_reranker = {
                let cache_dir = app
                    .path()
                    .app_data_dir()
                    .map(|dir| dir.join("models"))
                    .unwrap_or_else(|_| std::path::PathBuf::from("models"));
                services::embedding::build(cache_dir)
            };
            eprintln!(
                "[embedding] reranker ready={}",
                embedding_reranker.is_ready()
            );
            let search_manager = SearchManager::new(
                app.handle().clone(),
                store.clone(),
                embedding_reranker.clone(),
            );
            search_manager
                .recover_and_queue_startup_runs()
                .map_err(std::io::Error::other)?;
            // Query expansion (RFC 0054). Disabled without an OpenRouter key.
            let query_expander = services::query_expansion::QueryExpander::from_app_config();
            eprintln!("[query_expansion] ready={}", query_expander.is_ready());
            app.manage(store);
            app.manage(highlight_service);
            app.manage(pdf_downloads);
            app.manage(pdf_extractions);
            app.manage(source_acquisition);
            app.manage(reader_service);
            app.manage(chat_service);
            app.manage(metadata_enrichment);
            app.manage(discovery_providers);
            app.manage(search_manager);
            app.manage(embedding_reranker);
            app.manage(query_expander);
            app.manage(settings_store);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::vault::get_vault_status,
            commands::discovery::search_papers,
            commands::discovery::expand_search,
            commands::settings::get_settings,
            commands::settings::save_setting,
            commands::settings::clear_setting,
            commands::settings::test_provider_key,
            commands::settings::get_reranker_status,
            commands::library::get_library,
            commands::library::add_paper_to_vaults,
            commands::library::import_local_pdfs,
            commands::library::import_html_url,
            commands::library::autofill_paper_metadata,
            commands::library::apply_paper_metadata_candidate,
            commands::library::update_paper_metadata,
            commands::library::download_paper_pdf,
            commands::library::get_document_sources,
            commands::library::create_vault,
            commands::library::rename_vault,
            commands::library::delete_vault,
            commands::library::remove_paper_from_vault,
            commands::library::delete_paper_globally,
            commands::library::export_vault_bibtex,
            commands::reader::get_reader_document,
            commands::reader::get_discovery_reader_document,
            commands::reader::cancel_discovery_pdf_acquisition,
            commands::reader::probe_discovery_candidate_pdf,
            commands::reader::get_reader_pdf_bytes,
            commands::reader::open_html_document,
            commands::reader::get_reader_html,
            commands::reader::extract_paper_document,
            commands::chat::list_chat_threads,
            commands::chat::get_chat_thread,
            commands::chat::add_note_at_anchor,
            commands::chat::ask_at_anchor_streamed,
            commands::chat::annotate_streamed,
            commands::chat::auto_highlight,
            commands::chat::debug_log,
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
            commands::highlight::create_highlight,
            commands::highlight::recolor_highlight,
            commands::highlight::set_highlight_label,
            commands::highlight::set_highlight_note,
            commands::highlight::remove_highlight,
            commands::highlight::list_highlights,
            commands::highlight::create_agent_highlight,
            commands::highlight::list_agent_highlights,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
