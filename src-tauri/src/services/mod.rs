pub mod bibtex;
pub mod chat;
pub mod codex_runtime;
pub mod embedding;
pub mod highlight;
pub mod llm;
pub mod mcp;
pub mod metadata_enrichment;
pub mod query_expansion;
pub mod reader_service;
pub mod research;
pub mod runtime_readiness;
/// Retrieval within papers we hold. Distinct from `research`, which is
/// deep-research discovery of papers we do not (RFC 0076).
pub mod search;
pub mod settings;
pub mod source_acquisition;
pub mod vault_suggestions;
