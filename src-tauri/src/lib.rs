mod commands;
mod domain;
mod html_ingestion;
mod pdf_extraction;
mod pdf_ingestion;
mod pdf_layout;
mod services;
mod shared;
mod storage;

use pdf_extraction::{PdfExtractionConfig, PdfExtractionManager};
use pdf_ingestion::{PdfDownloadManager, PdfIngestionConfig};
use services::chat::ChatService;
use services::mcp::LocalMcpServer;
use services::metadata_enrichment::MetadataEnrichmentService;
use services::reader_service::ReaderService;
use services::research::controller::ProjectResearchController;
use services::research::manager::SearchManager;
use services::source_acquisition::SourceAcquisitionService;
use services::vault_suggestions::VaultSuggestionManager;
use storage::library_store::LibraryStore;
use tauri::{Listener, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
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
            // Recover parent Runs before SearchManager inspects child Search work.
            store
                .recover_interrupted_harness_runs()
                .map_err(std::io::Error::other)?;
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
            let mcp_server = tauri::async_runtime::block_on(LocalMcpServer::start(
                app.handle().clone(),
                store.clone(),
                pdf_downloads.clone(),
                pdf_extractions.clone(),
            ))
            .map_err(std::io::Error::other)?;
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
            //
            // One lazy handle is shared: the first semantic operation downloads
            // and initializes the ~130 MB model off the Tauri setup thread.
            // The reranker and chunk worker then reuse the same instance.
            let embedder = {
                let cache_dir = app
                    .path()
                    .app_data_dir()
                    .map(|dir| dir.join("models"))
                    .unwrap_or_else(|_| std::path::PathBuf::from("models"));
                services::embedding::load_embedder(cache_dir)
            };
            let embedding_reranker = services::embedding::build_reranker(embedder.clone());
            eprintln!(
                "[embedding] reranker ready={}",
                embedding_reranker.is_ready()
            );

            // Chunk embeddings (RFC 0075). The startup sweep re-chunks anything
            // below the current chunk version, then embeds whatever is missing.
            // Spawned rather than awaited: a first run over an existing library
            // is minutes of CPU, and the app is fully usable without it —
            // lexical search answers on its own.
            let embedder_for_search = embedder.clone();
            let chunk_embedder = services::embedding::chunk_worker::ChunkEmbeddingWorker::new(
                store.clone(),
                embedder,
            );
            eprintln!(
                "[embedding] chunk worker ready={}",
                chunk_embedder.is_ready()
            );

            // Search within our own papers (RFC 0076) — not deep-research
            // discovery, which is SearchManager above.
            let search_service =
                services::search::SearchService::new(store.clone(), embedder_for_search);

            // The agent's working memory (RFC 0077). Built after SearchService
            // because ContextManager is the only door to retrieval — ChatService
            // never calls search directly.
            let chat_config =
                services::chat::config::ChatConfig::load().map_err(std::io::Error::other)?;
            let evidence_questions = services::chat::EvidenceQuestionService::new(
                store.clone(),
                search_service.clone(),
                chat_config.clone(),
            );
            mcp_server.attach_evidence_questions(evidence_questions);
            let context_manager = services::chat::ContextManager::new(
                store.clone(),
                std::sync::Arc::new(search_service.clone()),
                chat_config.max_context_chars,
            );
            let search_manager = SearchManager::new(
                app.handle().clone(),
                store.clone(),
                embedding_reranker.clone(),
                source_acquisition.clone(),
            );
            mcp_server.attach_search_manager(search_manager.clone());
            search_manager
                .recover_and_queue_startup_runs()
                .map_err(std::io::Error::other)?;
            let research_controller = ProjectResearchController::new(
                app.handle().clone(),
                store.clone(),
                search_manager.clone(),
                mcp_server.clone(),
            );
            services::research::scheduler::HarnessScheduler::new(
                store.clone(),
                research_controller.clone(),
            )
            .start()
            .map_err(std::io::Error::other)?;
            let chat_browser_discovery =
                commands::discovery::browser::BrowserDiscoverySource::new_shared(
                    source_acquisition.clone(),
                    discovery_providers.openalex.clone(),
                    discovery_providers.arxiv.clone(),
                    commands::discovery::browser::BrowserDiscoveryConfig::load(&app.handle()),
                );
            let chat_research = services::chat::AppResearchToolbox::new(
                store.clone(),
                search_manager.clone(),
                chat_browser_discovery,
                source_acquisition.clone(),
            );
            let chat_service = ChatService::new(
                app.handle().clone(),
                chat_config,
                store.clone(),
                reader_service.clone(),
                context_manager.clone(),
                std::sync::Arc::new(chat_research),
            );
            {
                let worker = chunk_embedder.clone();
                tauri::async_runtime::spawn(async move {
                    let outcome = worker.recover_and_sweep().await;
                    eprintln!(
                        "[embedding] startup sweep embedded={} failed={}",
                        outcome.embedded, outcome.failed
                    );
                });

                // Then keep sweeping on demand. The startup sweep alone is not
                // enough: on the launch that re-extracts an upgraded library it
                // runs before the new chunks exist, so nothing would be embedded
                // until the launch after that.
                let worker = chunk_embedder.clone();
                tauri::async_runtime::spawn(worker.run_forever());

                // Extraction-ready is the signal. Listening to the event that
                // already exists keeps the extractor from having to know the
                // embedding worker exists at all.
                let worker = chunk_embedder.clone();
                app.handle()
                    .listen("document_extraction_updated", move |event| {
                        if event.payload().contains("\"status\":\"ready\"") {
                            worker.request_sweep();
                        }
                    });
            }
            let vault_suggestion_manager = VaultSuggestionManager::new(
                app.handle().clone(),
                store.clone(),
                embedding_reranker.clone(),
            );
            vault_suggestion_manager
                .recover_and_queue_startup_run()
                .map_err(std::io::Error::other)?;
            // Query expansion (RFC 0054). Disabled without an OpenRouter key.
            let query_expander = services::query_expansion::QueryExpander::from_app_config();
            eprintln!("[query_expansion] ready={}", query_expander.is_ready());
            let runtime_readiness = services::runtime_readiness::RuntimeReadinessService::new(
                pdf_extractions.clone(),
                source_acquisition.clone(),
                embedding_reranker.clone(),
            );
            app.manage(store);
            app.manage(mcp_server);
            app.manage(highlight_service);
            app.manage(pdf_downloads);
            app.manage(pdf_extractions);
            app.manage(source_acquisition.clone());
            app.manage(reader_service);
            app.manage(chat_service);
            app.manage(metadata_enrichment);
            app.manage(discovery_providers);
            app.manage(search_manager);
            app.manage(research_controller);
            app.manage(vault_suggestion_manager);
            app.manage(embedding_reranker);
            app.manage(chunk_embedder);
            app.manage(search_service);
            app.manage(context_manager);
            app.manage(query_expander);
            app.manage(settings_store);
            app.manage(runtime_readiness);
            // RFC 0100: pay browser startup latency while the rest of the app
            // initializes. Browser-backed commands still call ensure_ready(),
            // so they join this same single-flight attempt if it is in flight.
            let eager_browser = source_acquisition.clone();
            tauri::async_runtime::spawn(async move {
                let _ = eager_browser.ensure_browser_ready().await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::discovery::search_papers,
            commands::discovery::expand_search,
            commands::settings::get_settings,
            commands::settings::save_setting,
            commands::settings::clear_setting,
            commands::settings::test_provider_key,
            commands::settings::get_reranker_status,
            commands::settings::get_runtime_capabilities,
            commands::settings::retry_runtime_capability,
            commands::library::get_library,
            commands::search::search_chunks,
            commands::search::chunk_embedding_coverage,
            commands::context::add_chat_context,
            commands::context::delete_chat_context,
            commands::context::list_chat_context,
            commands::context::compact_chat_context,
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
            commands::library::create_project,
            commands::library::rename_project,
            commands::library::delete_project,
            commands::library::create_project_document,
            commands::library::get_project_document,
            commands::library::update_project_document,
            commands::library::delete_project_document,
            commands::library::remove_paper_from_vault,
            commands::library::delete_paper_globally,
            commands::library::export_vault_bibtex,
            commands::mcp::create_external_mcp_grant,
            commands::mcp::revoke_external_mcp_grant,
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
            commands::research::get_research_harness,
            commands::research::save_research_harness,
            commands::research::list_harness_configuration_versions,
            commands::research::clear_harness_configuration_history,
            commands::research::get_harness_run_instructions,
            commands::research::get_research_checkpoint,
            commands::research::list_research_checkpoints,
            commands::research::restore_research_checkpoint,
            commands::research::get_harness_change_set,
            commands::research::apply_harness_change_set,
            commands::research::reject_harness_change_set,
            commands::research::edit_harness_change_set,
            commands::research::run_project_research,
            commands::research::cancel_project_research,
            commands::research::pause_research_harness,
            commands::research::resume_research_harness,
            commands::research::stop_research_harness,
            commands::research_state::get_research_state,
            commands::research_state::get_research_entry,
            commands::research_state::list_research_evidence_candidates,
            commands::research_state::create_research_entry,
            commands::research_state::revise_research_entry,
            commands::research_state::set_research_entry_lifecycle,
            commands::research_document::create_document_from_research,
            commands::research_document::get_research_document_generation,
            commands::research_document::cancel_research_document_generation,
            commands::research_document::retry_research_document_generation,
            commands::harness_improvement::list_harness_improvements,
            commands::harness_improvement::get_harness_improvement,
            commands::harness_improvement::edit_harness_improvement,
            commands::harness_improvement::accept_harness_improvement,
            commands::harness_improvement::reject_harness_improvement,
            commands::vault_suggestions::get_vault_suggestions,
            commands::vault_suggestions::get_vault_suggestion_options,
            commands::vault_suggestions::save_vault_suggestion_options,
            commands::vault_suggestions::plan_vault_suggestion_queries,
            commands::vault_suggestions::run_vault_suggestions,
            commands::vault_suggestions::cancel_vault_suggestion_run,
            commands::vault_suggestions::dismiss_vault_suggestion,
            commands::vault_suggestions::undo_vault_suggestion_dismissal,
            commands::vault_suggestions::add_vault_suggestion,
            commands::source_acquisition::debug_obscura_start,
            commands::source_acquisition::debug_obscura_fetch,
            commands::source_acquisition::get_browser_runtime_status,
            commands::source_acquisition::retry_browser_runtime,
            commands::highlight::create_highlight,
            commands::highlight::recolor_highlight,
            commands::highlight::set_highlight_label,
            commands::highlight::set_highlight_note,
            commands::highlight::remove_highlight,
            commands::highlight::remove_annotation,
            commands::highlight::list_highlights,
            commands::highlight::create_agent_highlight,
            commands::highlight::list_agent_highlights,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");
    let mut shutdown_started = false;
    app.run(move |app_handle, event| {
        if let tauri::RunEvent::ExitRequested { code, api, .. } = event {
            if shutdown_started {
                return;
            }
            shutdown_started = true;
            api.prevent_exit();
            let app_handle = app_handle.clone();
            let controller = app_handle
                .state::<ProjectResearchController>()
                .inner()
                .clone();
            let mcp_server = app_handle.state::<LocalMcpServer>().inner().clone();
            tauri::async_runtime::spawn(async move {
                controller.shutdown().await;
                mcp_server.shutdown().await;
                app_handle.exit(code.unwrap_or(0));
            });
        }
    });
}
