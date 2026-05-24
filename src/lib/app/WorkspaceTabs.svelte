<script lang="ts">
  import type { WorkspaceTab } from "$lib/domain/workspace";

  let {
    tabs,
    activeTabId,
    onActivate,
    onClose,
  }: {
    tabs: WorkspaceTab[];
    activeTabId: string;
    onActivate: (tabId: string) => void;
    onClose: (tabId: string) => void;
  } = $props();

  function tabIcon(kind: WorkspaceTab["kind"]) {
    if (kind === "vault") {
      return "#";
    }

    if (kind === "discover") {
      return "find";
    }

    return "read";
  }
</script>

<div class="workspace-tabs row hair-b">
  {#each tabs as tab}
    <button
      class:active={tab.id === activeTabId}
      class="tab row"
      type="button"
      onclick={() => onActivate(tab.id)}
      title={tab.title}
    >
      <span class="icon">{tabIcon(tab.kind)}</span>
      <span class="title truncate">{tab.title}</span>
      <span
        class="close"
        role="button"
        tabindex="0"
        aria-label={`Close ${tab.title}`}
        onclick={(event) => {
          event.stopPropagation();
          onClose(tab.id);
        }}
        onkeydown={(event) => {
          if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            event.stopPropagation();
            onClose(tab.id);
          }
        }}
      >
        x
      </span>
    </button>
  {/each}
  <div class="flex1"></div>
  <div class="tab-actions row">
    <span>split</span>
    <span>layout</span>
  </div>
</div>

<style>
  .workspace-tabs {
    height: 26px;
    flex-shrink: 0;
    background: var(--bg-1);
  }

  .tab {
    height: 100%;
    max-width: 320px;
    min-width: 0;
    gap: 8px;
    padding: 0 10px 0 12px;
    border: 0;
    border-top: 1px solid transparent;
    border-right: 1px solid var(--border);
    background: transparent;
    color: var(--fg-2);
    font-size: 11px;
    cursor: pointer;
  }

  .tab.active {
    border-top-color: var(--amber);
    background: var(--bg);
    color: var(--amber);
  }

  .icon {
    flex-shrink: 0;
    color: var(--amber-mid);
    font-size: 9px;
  }

  .title {
    min-width: 0;
  }

  .close {
    flex-shrink: 0;
    padding: 0 2px;
    color: var(--fg-3);
    font-size: 11px;
  }

  .close:hover,
  .tab.active .close:hover {
    color: var(--amber);
  }

  .tab-actions {
    gap: 12px;
    padding: 0 10px;
    color: var(--fg-3);
    font-size: 10px;
  }
</style>
