import { invoke } from "@tauri-apps/api/core";
import type { VaultStatus } from "$lib/domain/vault";

export async function getVaultStatus(): Promise<VaultStatus> {
  return invoke<VaultStatus>("get_vault_status");
}
