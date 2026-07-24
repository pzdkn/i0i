<script lang="ts">
  import TextPref from "./TextPref.svelte";

  let {
    prefs,
    onChanged,
  }: {
    prefs: Record<string, string>;
    onChanged: () => Promise<void> | void;
  } = $props();

  // Curated suggestions; any OpenRouter slug can be typed.
  const MODELS = [
    "anthropic/claude-sonnet-4.5",
    "anthropic/claude-opus-4.1",
    "anthropic/claude-haiku-4.5",
    "openai/gpt-4o-mini",
    "google/gemini-2.5-flash",
    "meta-llama/llama-3.3-70b-instruct",
  ];
</script>

<div class="tab col">
  <p class="intro">
    Which model powers each job. Cheaper, faster models are fine for expansion and titles — this is
    the main cost lever. Leave a field empty to use the app default. Changes apply on the next app
    start.
  </p>

  <TextPref
    settingKey="model.chat"
    label="Chat"
    hint="Paper-scoped chat and Ask."
    value={prefs["model.chat"] ?? ""}
    placeholder="app default"
    options={MODELS}
    restartNote
    onSaved={onChanged}
  />
  <TextPref
    settingKey="model.planner"
    label="Deep research planner"
    hint="Plans and refines queries during deep research."
    value={prefs["model.planner"] ?? ""}
    placeholder="follows chat model"
    options={MODELS}
    onSaved={onChanged}
  />
  <TextPref
    settingKey="model.expansion"
    label="Query expansion"
    hint="Cheap model that widens quick searches. Keep this one small."
    value={prefs["model.expansion"] ?? ""}
    placeholder="app default (cheap)"
    options={MODELS}
    restartNote
    onSaved={onChanged}
  />
</div>

<style>
  .intro {
    margin: 0;
    padding: 14px 16px;
    font-size: 11px;
    line-height: 1.5;
    color: var(--fg-3, #888);
  }
</style>
