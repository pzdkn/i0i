//! Paper-scoped chat against an OpenRouter chat-completions provider.

pub(crate) mod config;
mod context;
mod service;

pub use service::ChatService;
