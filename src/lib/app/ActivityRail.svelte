<script lang="ts">
  import { Settings } from "@lucide/svelte";

  let {
    active = "P",
    onSelectMode = () => {},
    onOpenSettings = () => {},
    settingsAttention = false,
  }: {
    active?: string;
    onSelectMode?: (mode: string) => void;
    onOpenSettings?: () => void;
    settingsAttention?: boolean;
  } = $props();

  // RFC 0086 R3.1: GRAPH and ASK are gone. A graph is a view of the collection
  // and belongs in PROJECTS; asking already happens from the reader and, per
  // RFC 0087, from the vault inspector. Neither had a destination, and a mode
  // is a place you go. STUDY stays because RFC 0093 gives it one.
  // R3.3: the letter under each label is now the shortcut it always looked like.
  const modes = [
    { key: "P", label: "PROJECTS", reason: "" },
    { key: "F", label: "FIND", reason: "" },
    { key: "R", label: "READ", reason: "" },
    // R3.4: disabled with its reason, not inert. It comes alive with RFC 0093.
    { key: "S", label: "STUDY", reason: "Study is not built yet (RFC 0093)" },
  ];
</script>

<nav class="activity-rail hair-r" aria-label="Primary">
  <div class="logo hair-b">i0i</div>

  {#each modes as mode}
    <button
      class:active={mode.key === active}
      type="button"
      title={mode.reason || mode.label}
      disabled={Boolean(mode.reason)}
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
    <Settings size={18} strokeWidth={1.75} aria-hidden="true" />
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

  button:disabled {
    cursor: default;
    opacity: 0.42;
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
