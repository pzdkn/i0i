//! Discovery command-side modules.
//!
//! Rust only compiles sibling files when the parent module declares them here.
//! This makes `service.rs`, `provider.rs`, and provider adapters discoverable
//! from `crate::commands::discovery`.

pub mod provider;
pub mod providers;
pub mod service;
pub mod error;
pub use service::DiscoveryService;
