<script lang="ts">
  let {
    active = "V",
    onSelectMode = () => {},
    onOpenSettings = () => {},
    settingsAttention = false,
  }: {
    active?: string;
    onSelectMode?: (mode: string) => void;
    onOpenSettings?: () => void;
    settingsAttention?: boolean;
  } = $props();

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
      onclick={() => onSelectMode(mode.key)}
    >
      <span class="vertical">{mode.label}</span>
      <span class="key">{mode.key}</span>
    </button>
  {/each}

  <div class="spacer"></div>
  <button
    class="settings"
    type="button"
    title={settingsAttention ? "Settings — an API key is needed" : "Settings"}
    aria-label="Settings"
    onclick={onOpenSettings}
  >
    {#if settingsAttention}<span class="attention" aria-hidden="true"></span>{/if}
    <svg
      class="gear"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <circle cx="12" cy="12" r="3" />
      <path
        d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"
      />
    </svg>
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

  /* Settings is a utility action, not a mode: a compact gear rather than the
     tall vertical-text buttons above. */
  .settings {
    min-height: 44px;
    position: relative;
  }

  .gear {
    width: 18px;
    height: 18px;
  }

  /* First-run nudge: a dot when a required API key is unresolved (RFC 0055). */
  .attention {
    position: absolute;
    top: 8px;
    right: 10px;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--amber, #f2a93b);
  }

  .settings:hover {
    color: var(--amber);
  }
</style>
