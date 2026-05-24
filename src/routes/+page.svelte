<script lang="ts">
  import { onMount } from "svelte";
  import AppShell from "$lib/app/AppShell.svelte";
  import { getVaultStatus } from "$lib/bridge/tauri";
  import type { VaultStatus } from "$lib/domain/vault";
  import VaultExplorer from "$lib/features/vault/VaultExplorer.svelte";
  import VaultHome from "$lib/features/vault/VaultHome.svelte";

  let vaultStatus = $state<VaultStatus | null>(null);
  let bridgeError = $state("");

  onMount(async () => {
    try {
      vaultStatus = await getVaultStatus();
    } catch (error) {
      bridgeError = String(error);
    }
  });
</script>

<AppShell {vaultStatus} {bridgeError}>
  <VaultExplorer />
  <VaultHome />
</AppShell>
