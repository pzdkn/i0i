<script lang="ts">
  import { vaultTree } from "$lib/mock/vault-tree";
</script>

<aside class="explorer hair-r">
  <header class="row hair-b">
    <span class="label hot">Explorer</span>
    <div class="flex1"></div>
    <span class="mono-dim">+</span>
    <span class="mono-dim">import</span>
  </header>

  <div class="filter row">
    <span>/</span>
    <span class="truncate">filter vault...</span>
    <span class="key">/</span>
  </div>

  <div class="tree">
    {#each vaultTree as node}
      {#if node.kind === "section"}
        <div class="section label">{node.label}</div>
      {:else if node.kind === "item"}
        <div class:item-muted={node.muted} class="tree-row">
          <span class="glyph">-</span>
          <span class="truncate">{node.label}</span>
          {#if node.count !== undefined}
            <span class="count">{node.count}</span>
          {/if}
          {#if node.key}
            <span class="key">{node.key}</span>
          {/if}
        </div>
      {:else}
        <div class:item-muted={node.muted} class:active={node.active} class="tree-row folder">
          <span class="glyph">{node.open ? "v" : ">"}</span>
          <span class="folder-dot"></span>
          <span class="truncate">{node.label}</span>
          <span class="count">{node.count}</span>
        </div>
        {#if node.open && node.children}
          {#each node.children as child}
            <div class:active={child.active} class="tree-row child">
              <span class="folder-dot small"></span>
              <span class="truncate">{child.label}</span>
              {#if child.count !== undefined}
                <span class="count">{child.count}</span>
              {/if}
            </div>
          {/each}
        {/if}
      {/if}
    {/each}
  </div>
</aside>

<style>
  .explorer {
    width: 240px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--panel);
  }

  header {
    height: 26px;
    flex-shrink: 0;
    gap: 8px;
    padding: 0 10px;
    background: var(--bg-1);
  }

  .filter {
    height: 34px;
    flex-shrink: 0;
    gap: 6px;
    margin: 6px 8px;
    padding: 0 6px;
    border: 1px solid var(--border-2);
    color: var(--fg-3);
    background: var(--bg);
  }

  .filter span:first-child {
    color: var(--amber);
  }

  .tree {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 2px 0 12px;
  }

  .section {
    height: 30px;
    display: flex;
    align-items: flex-end;
    padding: 0 12px 5px;
    border-bottom: 1px solid var(--border);
  }

  .tree-row {
    height: 20px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 12px;
    color: var(--fg-1);
    font-size: 11px;
  }

  .tree-row.active {
    border-left: 2px solid var(--amber);
    background: rgba(242, 169, 59, 0.1);
    color: var(--amber);
  }

  .tree-row.child {
    padding-left: 31px;
  }

  .item-muted {
    color: var(--fg-3);
  }

  .glyph {
    width: 10px;
    color: var(--fg-3);
  }

  .folder-dot {
    width: 6px;
    height: 6px;
    flex-shrink: 0;
    background: var(--amber-mid);
  }

  .folder-dot.small {
    width: 5px;
    height: 5px;
    background: var(--amber-dim);
  }

  .count {
    margin-left: auto;
    color: var(--fg-3);
    font-size: 10px;
  }
</style>
