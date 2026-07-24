import { invoke } from "@tauri-apps/api/core";

/// Where a resolved secret/config value came from (RFC 0055).
export type SettingSource = "user" | "env" | "dotenv";

export type SecretStatus = {
  /** Logical name: openrouter | openalex | core | email. */
  name: string;
  /** Full setting key to write to, e.g. `secret.openrouter`. */
  settingKey: string;
  configured: boolean;
  source?: SettingSource;
  /** Resolved value — present only for non-sensitive entries (contact email);
   *  never present for API keys. */
  value?: string;
};

export type SettingsView = {
  secrets: SecretStatus[];
  /** Non-secret preferences (model.*, search.*) with values. */
  prefs: Record<string, string>;
};

export type RerankerStatus = { featureBuilt: boolean; ready: boolean };

export async function getSettings(): Promise<SettingsView> {
  return invoke<SettingsView>("get_settings");
}

export async function getRerankerStatus(): Promise<RerankerStatus> {
  return invoke<RerankerStatus>("get_reranker_status");
}

/// Write one setting. An empty value clears it (falls back to env/.env/default).
export async function saveSetting(key: string, value: string): Promise<void> {
  await invoke("save_setting", { key, value });
}

export async function clearSetting(key: string): Promise<void> {
  await invoke("clear_setting", { key });
}

/// Live-verify a provider's currently-resolved key. Resolves on success,
/// rejects with a message on failure. Call after saving (save-then-test).
export async function testProviderKey(name: string): Promise<void> {
  await invoke("test_provider_key", { name });
}
