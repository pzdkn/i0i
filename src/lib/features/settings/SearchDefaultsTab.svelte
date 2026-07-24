<script lang="ts">
  import { saveSetting, getRerankerStatus, type RerankerStatus } from "$lib/bridge/settings";
  import TogglePref from "./TogglePref.svelte";

  let {
    prefs,
    onChanged,
  }: {
    prefs: Record<string, string>;
    onChanged: () => Promise<void> | void;
  } = $props();

  let reranker = $state<RerankerStatus | null>(null);

  $effect(() => {
    void getRerankerStatus()
      .then((status) => (reranker = status))
      .catch(() => (reranker = null));
  });

  const rerankerStatusText = $derived(
    !reranker
      ? "…"
      : !reranker.featureBuilt
        ? "not built in this build"
        : reranker.ready
          ? "model ready"
          : "model not loaded",
  );

  // Defaults: expansion + reranker default on, viewable-filter off.
  const rerankerEnabled = $derived(prefs["search.reranker_enabled"] !== "false");
  const defaultExpand = $derived(prefs["search.default_expand"] !== "false");
  const defaultOnlyViewable = $derived(prefs["search.default_only_viewable"] === "true");
  const resultLimit = $derived(prefs["search.default_result_limit"] ?? "25");

  async function saveLimit(event: Event) {
    const value = (event.currentTarget as HTMLSelectElement).value;
    await saveSetting("search.default_result_limit", value);
    await onChanged();
  }
</script>

<div class="tab col">
  <p class="intro">
    Defaults for new searches. The per-search bar still overrides these for any one search.
  </p>

  <div class="group hair-b">
    <TogglePref
      settingKey="search.reranker_enabled"
      label="Rank results by meaning (embedding reranker)"
      hint={`Local semantic ranking · ${rerankerStatusText}`}
      checked={rerankerEnabled}
      disabled={!reranker?.featureBuilt}
      onSaved={onChanged}
    />
  </div>

  <TogglePref
    settingKey="search.default_expand"
    label="Expand my search by default"
    hint="Runs a cheap LLM to widen recall — costs an extra call + provider fan-out per search."
    checked={defaultExpand}
    onSaved={onChanged}
  />
  <TogglePref
    settingKey="search.default_only_viewable"
    label="Only show results I can open by default"
    hint="Hides results with no obtainable PDF/HTML."
    checked={defaultOnlyViewable}
    onSaved={onChanged}
  />

  <label class="limit row">
    <span class="col text">
      <span class="label">Default result limit</span>
      <span class="hint">How many candidates a quick search keeps.</span>
    </span>
    <select value={resultLimit} onchange={saveLimit}>
      <option value="10">10</option>
      <option value="25">25</option>
      <option value="50">50</option>
    </select>
  </label>
</div>

<style>
  .intro {
    margin: 0;
    padding: 14px 16px;
    font-size: 11px;
    line-height: 1.5;
    color: var(--fg-3, #888);
  }

  .limit {
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 12px 16px;
  }

  .text {
    gap: 3px;
  }

  .label {
    font-size: 12px;
    font-weight: 700;
    color: var(--fg-1, #ddd);
  }

  .hint {
    font-size: 11px;
    color: var(--fg-3, #888);
  }

  select {
    background: var(--bg-0, #0b0b0b);
    border: 1px solid var(--fg-3, #444);
    color: var(--fg-1, #ddd);
    padding: 5px 8px;
    font-size: 12px;
    font-family: inherit;
  }
</style>
