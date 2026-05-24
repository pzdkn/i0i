<script lang="ts">
  import { onMount } from "svelte";
  import AppShell from "$lib/app/AppShell.svelte";
  import { getVaultStatus } from "$lib/bridge/tauri";
  import type { VaultStatus } from "$lib/domain/vault";
  import ReaderView from "$lib/features/reader/ReaderView.svelte";
  import VaultExplorer from "$lib/features/vault/VaultExplorer.svelte";
  import VaultHome from "$lib/features/vault/VaultHome.svelte";

  type ActiveView = "vault" | "reader";

  let vaultStatus = $state<VaultStatus | null>(null);
  let bridgeError = $state("");
  let activeView = $state<ActiveView>("vault");
  let selectedPaperId = $state("vaswani2017");

  onMount(async () => {
    try {
      vaultStatus = await getVaultStatus();
    } catch (error) {
      bridgeError = String(error);
    }
  });
</script>

<AppShell activeMode={activeView === "reader" ? "R" : "V"} {vaultStatus} {bridgeError}>
  <VaultExplorer />
  {#if activeView === "reader"}
    <ReaderView
      paperId={selectedPaperId}
      onBack={() => {
        activeView = "vault";
      }}
    />
  {:else}
    <VaultHome
      onOpenPaper={(paperId) => {
        selectedPaperId = paperId;
        activeView = "reader";
      }}
    />
  {/if}
</AppShell>
