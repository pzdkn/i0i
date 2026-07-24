<script lang="ts">
  import type { SecretStatus } from "$lib/bridge/settings";
  import ApiKeyRow from "./ApiKeyRow.svelte";

  let {
    secrets,
    onChanged,
  }: {
    secrets: SecretStatus[];
    onChanged: () => Promise<void> | void;
  } = $props();

  // Display metadata per logical secret name. Order here drives the layout.
  const META: Record<
    string,
    { label: string; unlocks: string; placeholder: string; isEmail?: boolean }
  > = {
    openrouter: {
      label: "OpenRouter",
      unlocks: "Chat, deep research, and query expansion",
      placeholder: "sk-or-…",
    },
    openalex: {
      label: "OpenAlex",
      unlocks: "Discovery search",
      placeholder: "API key",
    },
    core: {
      label: "CORE",
      unlocks: "The CORE provider — extra open-access results",
      placeholder: "API key",
    },
    email: {
      label: "Contact email",
      unlocks: "Better open-access PDF resolution (Unpaywall)",
      placeholder: "you@example.com",
      isEmail: true,
    },
  };

  const ordered = $derived(
    Object.keys(META)
      .map((name) => secrets.find((secret) => secret.name === name))
      .filter((secret): secret is SecretStatus => Boolean(secret)),
  );
</script>

<div class="tab col">
  <p class="intro">
    Keys are stored on this machine and used to reach each service. A value set here overrides an
    environment variable or <code>.env</code>. Values are never shown back — save a new one to
    replace it.
  </p>
  {#each ordered as secret (secret.name)}
    <ApiKeyRow
      {secret}
      label={META[secret.name].label}
      unlocks={META[secret.name].unlocks}
      placeholder={META[secret.name].placeholder}
      isEmail={META[secret.name].isEmail ?? false}
      {onChanged}
    />
  {/each}
</div>

<style>
  .intro {
    margin: 0;
    padding: 14px 16px;
    font-size: 11px;
    line-height: 1.5;
    color: var(--fg-3, #888);
  }

  code {
    font-family: inherit;
    color: var(--fg-2, #aaa);
  }
</style>
