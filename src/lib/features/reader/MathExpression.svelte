<script lang="ts">
  /** Safely render one parser-approved TeX expression with a literal fallback. */
  import { render as renderMath } from "katex";
  import "katex/dist/katex.min.css";

  let {
    tex,
    raw,
    display = false,
  }: {
    tex: string;
    raw: string;
    display?: boolean;
  } = $props();

  let container = $state<HTMLElement>();
  let failed = $state(false);

  $effect(() => {
    if (!container) return;

    container.replaceChildren();
    try {
      // KaTeX generates its own DOM. Model-authored HTML never enters this
      // component, and trust remains disabled for commands such as \href.
      renderMath(tex, container, {
        displayMode: display,
        output: "htmlAndMathml",
        throwOnError: true,
        trust: false,
        maxExpand: 1_000,
        maxSize: 20,
      });
      failed = false;
    } catch {
      failed = true;
      container.replaceChildren();
    }
  });
</script>

{#if display}
  <div class="math display" class:hidden={failed} bind:this={container}></div>
{:else}
  <span class="math inline" class:hidden={failed} bind:this={container}></span>
{/if}

{#if failed}
  {#if display}
    <pre class="fallback display-fallback">{raw}</pre>
  {:else}
    <code class="fallback">{raw}</code>
  {/if}
{/if}

<style>
  .math {
    max-width: 100%;
  }

  .inline {
    display: inline-block;
    vertical-align: -0.12em;
  }

  .display {
    overflow-x: auto;
    overflow-y: hidden;
    margin: 4px 0 8px;
    padding: 2px 0;
  }

  .hidden {
    display: none;
  }

  .fallback {
    font: inherit;
    white-space: pre-wrap;
  }

  .display-fallback {
    overflow-x: auto;
    margin: 4px 0 8px;
  }
</style>
