//! Paper-scoped chat against an OpenRouter chat-completions provider.

pub mod agent_loop;
pub(crate) mod config;
mod context;
pub mod context_manager;
mod service;

pub use context_manager::ContextManager;
pub use service::{AutoHighlightCategory, ChatService};
