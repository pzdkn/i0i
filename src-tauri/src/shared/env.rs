//! Environment / `.env` resolution shared across features.
//!
//! Several features (discovery providers, chat) resolve secrets from an
//! environment variable first and fall back to a repo-local `.env`. Keeping the
//! `.env` reader here means no feature has to depend on another feature's
//! module just to read a key.

use std::{fs, path::Path};

/// Read a single key's value from `.env` files adjacent to the cargo manifest.
pub(crate) fn read_dotenv_value(key: &str) -> Option<String> {
    dotenv_paths()
        .into_iter()
        .filter_map(|path| fs::read_to_string(path).ok())
        .find_map(|contents| parse_dotenv_value(&contents, key))
}

fn dotenv_paths() -> [std::path::PathBuf; 2] {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    [manifest_dir.join(".env"), manifest_dir.join("../.env")]
}

pub(crate) fn parse_dotenv_value(contents: &str, key: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let (name, value) = line.split_once('=')?;
        if name.trim() != key {
            return None;
        }
        Some(value.trim().trim_matches(['"', '\'']).to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotenv_parser_reads_named_key() {
        assert_eq!(
            parse_dotenv_value(
                "OTHER=value\nOPENALEX_API_KEY='secret'\n",
                "OPENALEX_API_KEY"
            ),
            Some("secret".to_string())
        );
    }
}
