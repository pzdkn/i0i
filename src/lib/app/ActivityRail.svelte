<script lang="ts">
  let { active = "V" }: { active?: string } = $props();

  const modes = [
    { key: "V", label: "VAULT" },
    { key: "F", label: "FIND" },
    { key: "R", label: "READ" },
    { key: "G", label: "GRAPH" },
    { key: "A", label: "ASK" },
    { key: "S", label: "STUDY" },
  ];
</script>

<nav class="activity-rail hair-r" aria-label="Primary">
  <div class="logo hair-b">i0i</div>

  {#each modes as mode}
    <button
      class:active={mode.key === active}
      type="button"
      title={mode.label}
      aria-pressed={mode.key === active}
    >
      <span class="vertical">{mode.label}</span>
      <span class="key">{mode.key}</span>
    </button>
  {/each}

  <div class="spacer"></div>
  <button type="button" title="Settings">
    <span class="vertical">SETTINGS</span>
    <span class="key">?</span>
  </button>
</nav>

<style>
  .activity-rail {
    width: 44px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    background: var(--panel);
  }

  .logo {
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--amber);
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.05em;
  }

  button {
    min-height: 78px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    border: 0;
    border-left: 2px solid transparent;
    background: transparent;
    color: var(--fg-3);
    cursor: pointer;
  }

  button.active {
    border-left-color: var(--amber);
    background: rgba(242, 169, 59, 0.06);
    color: var(--amber);
  }

  .vertical {
    writing-mode: vertical-rl;
    transform: rotate(180deg);
    font-size: 9px;
    letter-spacing: 0.24em;
  }

  .spacer {
    flex: 1;
  }
</style>
