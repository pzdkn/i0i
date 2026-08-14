<script lang="ts">
  import TextPref from "./TextPref.svelte";

  let {
    prefs,
    onChanged,
  }: {
    prefs: Record<string, string>;
    onChanged: () => Promise<void> | void;
  } = $props();

  // Curated suggestions; any OpenRouter slug can be typed. DeepSeek leads the
  // list since RFC 0079 R7 — same tool-calling and structured-output support at
  // a fraction of the price.
  const MODELS = [
    "deepseek/deepseek-v4-pro",
    "deepseek/deepseek-v3.2",
    "deepseek/deepseek-v4-flash",
    "deepseek/deepseek-v4-flash-0731",
    "anthropic/claude-sonnet-4.5",
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
  <!-- RFC 0079 R7.4: these two read preferences in the backend but had no field
       here, so the model on the retrieval critical path was unreachable. -->
  <TextPref
    settingKey="model.annotation"
    label="Annotation & retrieval"
    hint="Decides what to look up before an answer, and picks passages to highlight. Keep it fast."
    value={prefs["model.annotation"] ?? ""}
    placeholder="app default (cheap)"
    options={MODELS}
    restartNote
    onSaved={onChanged}
  />
  <!-- The 402 this came from: with no cap, OpenRouter reserves the model's full
       completion ceiling (32,000 tokens for deepseek-v4-pro) and refuses the
       request if the balance or the key's credit limit cannot cover that
       maximum — even when the answer itself would cost a fraction of a cent. -->
  <TextPref
    settingKey="model.max_tokens"
    label="Max answer tokens"
    hint="How much an answer may generate. Also what OpenRouter reserves against your balance — leave it low unless answers are being cut off."
    value={prefs["model.max_tokens"] ?? ""}
    placeholder="app default (2048)"
    restartNote
    onSaved={onChanged}
  />
  <TextPref
    settingKey="model.title"
    label="Thread titles"
    hint="Names a new conversation in 24 tokens. The cheapest model will do."
    value={prefs["model.title"] ?? ""}
    placeholder="app default (cheap)"
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
