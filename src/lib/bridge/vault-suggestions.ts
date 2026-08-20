import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { LibrarySnapshot } from "$lib/domain/library";
import type {
  VaultSuggestion,
  VaultSuggestionPreview,
  VaultSuggestionSnapshot,
  VaultSuggestionUpdated,
} from "$lib/domain/vault-suggestion";

export function getVaultSuggestions(vaultId: string): Promise<VaultSuggestionSnapshot> {
  return invoke("get_vault_suggestions", { vaultId });
}

export function runVaultSuggestions(vaultId: string): Promise<string> {
  return invoke("run_vault_suggestions", { vaultId });
}

export function cancelVaultSuggestionRun(runId: string): Promise<void> {
  return invoke("cancel_vault_suggestion_run", { runId });
}

export function dismissVaultSuggestion(suggestion: VaultSuggestion): Promise<void> {
  return invoke("dismiss_vault_suggestion", { suggestion });
}

export function undoVaultSuggestionDismissal(
  suggestionId: string,
): Promise<VaultSuggestionSnapshot> {
  return invoke("undo_vault_suggestion_dismissal", { suggestionId });
}

export function addVaultSuggestion(suggestion: VaultSuggestion): Promise<LibrarySnapshot> {
  return invoke("add_vault_suggestion", { suggestion });
}

export function listenVaultSuggestionUpdated(
  onEvent: (payload: VaultSuggestionUpdated) => void,
): Promise<UnlistenFn> {
  return listen("vault_suggestion_updated", (event) =>
    onEvent(event.payload as VaultSuggestionUpdated),
  );
}

export function listenVaultSuggestionPreview(
  onEvent: (payload: VaultSuggestionPreview) => void,
): Promise<UnlistenFn> {
  return listen("vault_suggestion_preview", (event) =>
    onEvent(event.payload as VaultSuggestionPreview),
  );
}
