//! Paper-scoped chat against an OpenRouter chat-completions provider.

pub mod agent_loop;
pub(crate) mod config;
mod context;
pub mod context_manager;
pub(crate) mod evidence_question;
mod research_tools;
mod service;

pub use context_manager::ContextManager;
pub(crate) use evidence_question::EvidenceQuestionService;
pub use research_tools::AppResearchToolbox;
pub use service::{AutoHighlightCategory, ChatService};
