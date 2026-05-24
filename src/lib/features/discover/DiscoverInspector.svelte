<aside class="discover-inspector col hair-l">
  <header class="panel-header hair-b">
    <div class="label hot">Search Console</div>
    <h2>ssl + dino run</h2>
  </header>

  <section class="panel-section col">
    <div class="row section-title">
      <span>Filters</span>
      <span class="mono-dim">editable later</span>
    </div>
    <div class="range-row row">
      <span>2020</span>
      <div class="gauge flex1"><i style="width: 82%"></i></div>
      <span>2026</span>
    </div>
    <div class="chips row">
      <span class="chip hot">ICLR</span>
      <span class="chip">CVPR</span>
      <span class="chip">ICML</span>
      <span class="chip">ECCV</span>
      <span class="chip">arXiv</span>
    </div>
  </section>

  <section class="panel-section col">
    <div class="row section-title">
      <span>Signal Mix</span>
      <span class="mono-dim">mock</span>
    </div>
    <div class="metric-row">
      <span>semantic</span>
      <div class="gauge"><i style="width: 45%"></i></div>
    </div>
    <div class="metric-row">
      <span>citation graph</span>
      <div class="gauge"><i style="width: 35%"></i></div>
    </div>
    <div class="metric-row">
      <span>novelty</span>
      <div class="gauge"><i style="width: 20%"></i></div>
    </div>
  </section>

  <section class="panel-section col">
    <div class="row section-title">
      <span>Agents</span>
      <span class="mono-dim">visible only</span>
    </div>
    <div class="agent-row row">
      <span class="key">A1</span>
      <span>citation scout</span>
      <strong>idle</strong>
    </div>
    <div class="agent-row row">
      <span class="key">A2</span>
      <span>author drift</span>
      <strong>idle</strong>
    </div>
    <div class="agent-row row">
      <span class="key">A3</span>
      <span>venue watch</span>
      <strong>idle</strong>
    </div>
  </section>

  <section class="panel-section col">
    <div class="row section-title">
      <span>Schedule</span>
      <span class="mono-dim">not active</span>
    </div>
    <div class="schedule-line row">
      <span>weekly sweep</span>
      <strong>off</strong>
    </div>
    <div class="schedule-line row">
      <span>alert threshold</span>
      <strong>0.88</strong>
    </div>
  </section>

  <div class="flex1"></div>

  <footer class="batch-actions row hair-t">
    <button class="btn" type="button">Queue</button>
    <button class="btn" type="button">Compare</button>
    <button class="btn primary" type="button">Add</button>
  </footer>
</aside>

<style>
  .discover-inspector {
    width: 300px;
    flex-shrink: 0;
    min-height: 0;
    background: var(--panel);
  }

  .panel-header {
    padding: 14px;
  }

  h2 {
    margin: 3px 0 0;
    color: var(--amber);
    font-size: 15px;
    font-weight: 600;
  }

  .panel-section {
    gap: 10px;
    padding: 14px;
    border-bottom: 1px solid var(--border);
  }

  .section-title {
    justify-content: space-between;
    color: var(--fg-2);
    font-size: 10px;
    text-transform: uppercase;
  }

  .range-row,
  .agent-row,
  .schedule-line {
    gap: 8px;
    color: var(--fg-3);
    font-size: 10px;
  }

  .chips {
    gap: 5px;
    flex-wrap: wrap;
  }

  .metric-row {
    display: grid;
    grid-template-columns: 92px 1fr;
    align-items: center;
    gap: 8px;
    color: var(--fg-3);
    font-size: 10px;
  }

  .agent-row span:nth-child(2),
  .schedule-line span {
    flex: 1;
    min-width: 0;
  }

  strong {
    color: var(--amber-mid);
    font-weight: 500;
  }

  .batch-actions {
    height: 42px;
    flex-shrink: 0;
    justify-content: flex-end;
    gap: 6px;
    padding: 0 10px;
    background: var(--bg-1);
  }
</style>
