pub mod http;
pub mod obscura;
pub mod service;
pub mod types;

pub use service::SourceAcquisitionService;
pub use types::{BrowserEndpoint, BrowserPageSnapshot};
