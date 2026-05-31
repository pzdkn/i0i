<script lang="ts">
  import { convertFileSrc } from "@tauri-apps/api/core";

  let {
    pdfUrl,
  }: {
    pdfUrl: string;
  } = $props();

  const safeUrl = $derived(convertFileSrc(pdfUrl));
</script>

{#snippet cover()}
  <div class="placeholder col">
    <div class="cover-label">PDF</div>
    <p>Loading viewer…</p>
  </div>
{/snippet}

<div class="pdf-page">
  <object
    data={safeUrl}
    type="application/pdf"
    title="Paper PDF"
  >
    {@render cover()}
  </object>
</div>

<style>
  .pdf-page {
    flex: 1;
    min-width: 0;
    min-height: 0;
    overflow: auto;
    display: flex;
    justify-content: center;
    padding: 20px 24px;
  }

  object {
    width: 100%;
    max-width: 900px;
    height: 100%;
    border: 1px solid rgba(0, 0, 0, 0.22);
    border-radius: 4px;
    background: #fff;
  }

  .placeholder {
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 48px;
    color: var(--fg-2);
  }

  .cover-label {
    font-size: 28px;
    font-weight: 700;
    letter-spacing: 0.04em;
    color: var(--amber);
  }
</style>
