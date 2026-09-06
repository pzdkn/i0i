//! User settings store and secret resolution (RFC 0055).
//!
//! A small key/value store in the app data dir lets users configure API keys,
//! model choices, and search defaults from the UI instead of a repo-local
//! `.env`. Secrets and config resolve through one precedence chain:
//!
//! ```text
//! user settings store  ->  environment variable  ->  .env
//! ```
//!
//! The store is exposed to the config resolvers through a process-global handle
//! (like `read_dotenv_value` is a global read of env + repo file). The handle
//! holds only a DB path and opens a fresh connection per op, so a value saved
//! from the UI is picked up on the next resolve with no restart — for resolvers
//! that read their key lazily per request (chat, OpenAlex, CORE).

use std::path::PathBuf;
use std::sync::OnceLock;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::shared::env::read_dotenv_value;

/// Where a resolved secret/config value came from (for status display).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingSource {
    /// The user's settings store (highest priority).
    User,
    /// A process environment variable.
    Env,
    /// The repo-local `.env` (developer fallback).
    Dotenv,
}

/// Key/value settings persisted in `settings.sqlite` in the app data dir.
///
/// Cheap to clone (holds only a path); every operation opens a fresh
/// connection, matching `LibraryStore`.
#[derive(Debug, Clone)]
pub struct SettingsStore {
    db_path: PathBuf,
}

impl SettingsStore {
    pub fn new(app: &AppHandle) -> Result<Self, String> {
        let app_data_dir = app
            .path()
            .app_data_dir()
            .map_err(|error| error.to_string())?;
        std::fs::create_dir_all(&app_data_dir).map_err(|error| error.to_string())?;
        let store = Self {
            db_path: app_data_dir.join("settings.sqlite"),
        };
        store.init()?;
        Ok(store)
    }

    #[cfg(test)]
    fn for_test(db_path: PathBuf) -> Self {
        let store = Self { db_path };
        store.init().unwrap();
        store
    }

    fn init(&self) -> Result<(), String> {
        let conn = self.open()?;
        conn.execute_batch(
            "create table if not exists app_settings (
               key   text primary key,
               value text not null
             );",
        )
        .map_err(|error| error.to_string())?;
        // Stamp a schema version so a future key rename has a migration anchor
        // (RFC 0055). Written once; never overwritten.
        conn.execute(
            "insert or ignore into app_settings (key, value) values ('meta.schema_version', '1')",
            [],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
    }

    fn open(&self) -> Result<Connection, String> {
        let conn = Connection::open(&self.db_path).map_err(|error| error.to_string())?;
        // Fresh connection per op (like LibraryStore): WAL keeps a concurrent
        // read from blocking a write, and a busy timeout absorbs the brief lock
        // contention of several quick saves from a settings form instead of
        // erroring with SQLITE_BUSY (RFC 0055).
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|error| error.to_string())?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|error| error.to_string())?;
        Ok(conn)
    }

    pub fn get(&self, key: &str) -> Result<Option<String>, String> {
        let conn = self.open()?;
        conn.query_row(
            "select value from app_settings where key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| error.to_string())
    }

    /// Upsert a setting. An empty/whitespace value removes the row, so a cleared
    /// field falls back to env/`.env`.
    pub fn set(&self, key: &str, value: &str) -> Result<(), String> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return self.clear(key);
        }
        let conn = self.open()?;
        conn.execute(
            "insert into app_settings (key, value) values (?1, ?2)
             on conflict(key) do update set value = excluded.value",
            params![key, trimmed],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
    }

    pub fn clear(&self, key: &str) -> Result<(), String> {
        let conn = self.open()?;
        conn.execute("delete from app_settings where key = ?1", params![key])
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    /// All non-secret preference rows (`model.*`, `search.*`, …). Secret rows
    /// (`secret.*`) are never returned — their values must not reach the
    /// renderer (RFC 0055).
    pub fn prefs(&self) -> Result<Vec<(String, String)>, String> {
        let conn = self.open()?;
        let mut stmt = conn
            .prepare(
                "select key, value from app_settings
                 where key not like 'secret.%' and key not like 'meta.%'",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }
}

/// Process-global settings handle, set once at startup so the free-function
/// resolvers (which have no store handle) can consult it.
static GLOBAL: OnceLock<SettingsStore> = OnceLock::new();

/// Install the global settings handle. Idempotent; a second call is ignored.
pub fn init_global(store: SettingsStore) {
    let _ = GLOBAL.set(store);
}

fn global() -> Option<&'static SettingsStore> {
    GLOBAL.get()
}

/// Resolve a value through the precedence chain, also reporting its source.
/// Kept store-explicit (not reading the global) so it is unit-testable without
/// touching process-global state.
fn resolve_with(
    store: Option<&SettingsStore>,
    setting_key: &str,
    env_key: &str,
) -> (Option<String>, Option<SettingSource>) {
    if let Some(store) = store {
        if let Ok(Some(value)) = store.get(setting_key) {
            let value = value.trim().to_string();
            if !value.is_empty() {
                return (Some(value), Some(SettingSource::User));
            }
        }
    }
    if let Ok(value) = std::env::var(env_key) {
        let value = value.trim().to_string();
        if !value.is_empty() {
            return (Some(value), Some(SettingSource::Env));
        }
    }
    if let Some(value) = read_dotenv_value(env_key) {
        let value = value.trim().to_string();
        if !value.is_empty() {
            return (Some(value), Some(SettingSource::Dotenv));
        }
    }
    (None, None)
}

/// Resolve a value (and its source) using the global store. `get_settings` uses
/// the source; key consumers use `resolve_secret` which throws it away.
pub fn resolve_secret_with_source(
    setting_key: &str,
    env_key: &str,
) -> (Option<String>, Option<SettingSource>) {
    resolve_with(global(), setting_key, env_key)
}

/// Resolve a secret/config value: user store → env → `.env`. `None` when unset.
pub fn resolve_secret(setting_key: &str, env_key: &str) -> Option<String> {
    resolve_secret_with_source(setting_key, env_key).0
}

/// Read a non-secret preference (`model.*`, `search.*`, `research.*`) from the global store.
/// No env/`.env` fallback — callers supply their own default. `None` when unset.
pub fn preference(key: &str) -> Option<String> {
    global()
        .and_then(|store| store.get(key).ok().flatten())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// A boolean preference: `true`/`false` string, else the supplied default.
pub fn preference_bool(key: &str, default: bool) -> bool {
    match preference(key).as_deref() {
        Some("true") => true,
        Some("false") => false,
        _ => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> SettingsStore {
        let path = std::env::temp_dir().join(format!("i0i-settings-{}.sqlite", uid()));
        let _ = std::fs::remove_file(&path);
        SettingsStore::for_test(path)
    }

    fn uid() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    #[test]
    fn crud_round_trips_and_clear_removes() {
        let store = temp_store();
        assert_eq!(store.get("model.chat").unwrap(), None);
        store.set("model.chat", "anthropic/x").unwrap();
        assert_eq!(
            store.get("model.chat").unwrap().as_deref(),
            Some("anthropic/x")
        );
        store.set("model.chat", "anthropic/y").unwrap();
        assert_eq!(
            store.get("model.chat").unwrap().as_deref(),
            Some("anthropic/y")
        );
        // Empty value clears the row (falls back to defaults/env).
        store.set("model.chat", "  ").unwrap();
        assert_eq!(store.get("model.chat").unwrap(), None);
    }

    #[test]
    fn prefs_excludes_secret_rows() {
        let store = temp_store();
        store
            .set("secret.openrouter", "sk-should-not-leak")
            .unwrap();
        store.set("model.chat", "anthropic/x").unwrap();
        let prefs = store.prefs().unwrap();
        assert!(prefs.iter().any(|(k, _)| k == "model.chat"));
        assert!(
            !prefs.iter().any(|(k, _)| k.starts_with("secret.")),
            "secret rows must never appear in prefs: {prefs:?}"
        );
    }

    #[test]
    fn resolve_prefers_store_over_env_and_dotenv() {
        let store = temp_store();
        // A unique env key that isn't in the repo .env.
        let env_key = format!("I0I_TEST_SECRET_{}", uid());
        store.set("secret.test", "from-store").unwrap();
        std::env::set_var(&env_key, "from-env");

        let (value, source) = resolve_with(Some(&store), "secret.test", &env_key);
        assert_eq!(value.as_deref(), Some("from-store"));
        assert_eq!(source, Some(SettingSource::User));

        std::env::remove_var(&env_key);
    }

    #[test]
    fn resolve_falls_back_to_env_when_store_empty() {
        let store = temp_store();
        let env_key = format!("I0I_TEST_SECRET_{}", uid());
        std::env::set_var(&env_key, "from-env");

        let (value, source) = resolve_with(Some(&store), "secret.absent", &env_key);
        assert_eq!(value.as_deref(), Some("from-env"));
        assert_eq!(source, Some(SettingSource::Env));

        std::env::remove_var(&env_key);
    }

    #[test]
    fn resolve_returns_none_when_nothing_set() {
        let store = temp_store();
        let env_key = format!("I0I_TEST_ABSENT_{}", uid());
        let (value, source) = resolve_with(Some(&store), "secret.absent", &env_key);
        assert_eq!(value, None);
        assert_eq!(source, None);
    }
}
