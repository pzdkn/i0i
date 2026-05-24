<script lang="ts">
  import type { ReaderDocument } from "$lib/domain/reader";

  let { document }: { document: ReaderDocument } = $props();
</script>

<aside class="reader-inspector hair-l">
  <header class="hair-b">
    <div class="paper-name truncate">{document.title}</div>
    <div class="mono-dim">paper / selected / reader</div>
  </header>

  <section>
    <div class="row section-title">
      <span class="label hot">Lineage</span>
      <div class="flex1"></div>
      <span class="mono-dim">3 &lt;- / -&gt; 5</span>
    </div>

    <div class="lineage">
      <div class="label">Cites</div>
      <p><span>●</span> Bahdanau 2014 <em>soft alignment seed</em></p>
      <p><span>○</span> Luong 2015 <em>dot-product variant</em></p>
      <p><span>○</span> Cheng 2016 <em>intra-attention</em></p>

      <div class="label cited">Cited by</div>
      <p><span>●</span> Devlin 2019 <em>BERT</em></p>
      <p><span>●</span> Dosovitskiy 2021 <em>ViT</em></p>
      <p><span>○</span> Touvron 2023 <em>LLaMA</em></p>
    </div>

    <button class="btn ghost wide" type="button">Open graph</button>
  </section>

  <section>
    <div class="label hot">Ask this paper</div>
    <div class="ask-box">
      <div class="label">You</div>
      <p>Explain scaled dot-product attention vs additive attention.</p>
      <div class="answer">
        i0i will cite paragraph anchors here. For now this is a static Reader mock.
      </div>
      <div class="row ask-actions">
        <button class="btn primary" type="button">Run</button>
        <button class="btn ghost" type="button">Follow-up</button>
      </div>
    </div>
  </section>

  <div class="flex1"></div>

  <section class="metadata hair-t">
    <div class="label">Metadata</div>
    <div class="meta-grid">
      <span>id</span><strong>{document.identifier}</strong>
      <span>venue</span><strong>{document.venue} {document.year}</strong>
      <span>cite</span><strong>{document.citationKey}</strong>
      <span>marks</span><strong>{document.marks.length}</strong>
    </div>
  </section>
</aside>

<style>
  .reader-inspector {
    width: 320px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--panel);
  }

  header {
    flex-shrink: 0;
    padding: 10px 14px;
    background: var(--bg-1);
    font-size: 10px;
  }

  .paper-name {
    margin-bottom: 2px;
    color: var(--amber);
    font-size: 11px;
  }

  section {
    padding: 14px 14px 0;
  }

  .section-title {
    gap: 8px;
  }

  .lineage {
    margin-top: 8px;
    color: var(--fg-2);
    font-size: 10px;
    line-height: 1.6;
  }

  .lineage p {
    margin: 3px 0;
  }

  .lineage span {
    color: var(--amber);
  }

  .lineage em {
    margin-left: 4px;
    color: var(--fg-3);
    font-style: normal;
  }

  .cited {
    margin-top: 10px;
  }

  .wide {
    width: 100%;
    margin-top: 8px;
  }

  .ask-box {
    margin-top: 8px;
    padding: 8px;
    border: 1px solid var(--amber-dim);
    background: rgba(242, 169, 59, 0.04);
  }

  .ask-box p {
    margin: 5px 0;
    color: var(--fg-1);
    font-size: 11px;
    line-height: 1.45;
  }

  .answer {
    margin-top: 6px;
    color: var(--fg-2);
    font-size: 10.5px;
    line-height: 1.5;
  }

  .ask-actions {
    gap: 6px;
    margin-top: 8px;
  }

  .metadata {
    padding: 10px 14px;
    background: var(--bg-1);
  }

  .meta-grid {
    display: grid;
    grid-template-columns: 48px 1fr;
    gap: 4px 8px;
    margin-top: 6px;
    color: var(--fg-3);
    font-size: 10px;
  }

  .meta-grid strong {
    min-width: 0;
    color: var(--fg-2);
    font-weight: 400;
  }
</style>
