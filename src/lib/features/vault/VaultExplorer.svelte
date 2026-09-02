<script lang="ts">
  import { tick } from "svelte";
  import type { ProjectWorkspace } from "$lib/domain/library";

  let {
    activeProjectId,
    activeProjectView,
    projects,
    onOpenProject,
    onOpenProjectDocument,
    onCreateProject,
    onRenameProject,
    onRemoveProject,
  }: {
    activeProjectId: string;
    activeProjectView: "research" | "vault" | "documents";
    projects: ProjectWorkspace[];
    onOpenProject: (projectId: string, view: "research" | "vault" | "documents") => void;
    onOpenProjectDocument: (projectId: string, documentId: string) => void;
    onCreateProject: (title: string) => Promise<void>;
    onRenameProject: (projectId: string, title: string) => Promise<void>;
    onRemoveProject: (projectId: string) => Promise<void>;
  } = $props();

  let filterText = $state("");
  let createText = $state("");
  let isCreating = $state(false);
  let createInput = $state<HTMLInputElement | null>(null);
  let contextMenu = $state<{ x: number; y: number; projectId?: string } | null>(null);
  let renamingProjectId = $state("");
  let renameText = $state("");
  let renameInput = $state<HTMLInputElement | null>(null);
  const visibleProjects = $derived(
    projects.filter((project) => {
      const query = filterText.trim().toLowerCase();
      if (!query) {
        return true;
      }

      return (
        project.id.toLowerCase().includes(query) ||
        project.title.toLowerCase().includes(query) ||
        project.vault.path.toLowerCase().includes(query)
      );
    }),
  );

  async function submitCreate() {
    const title = createText.trim();
    if (!title) {
      return;
    }

    await onCreateProject(title);
    createText = "";
    isCreating = false;
  }

  async function startCreate() {
    closeContextMenu();
    isCreating = true;
    await tick();
    createInput?.focus();
  }

  function showContextMenu(event: MouseEvent, projectId?: string) {
    event.preventDefault();
    event.stopPropagation();
    contextMenu = {
      x: event.clientX,
      y: event.clientY,
      projectId,
    };
  }

  function closeContextMenu() {
    contextMenu = null;
  }

  async function startRename(project: ProjectWorkspace) {
    closeContextMenu();
    renamingProjectId = project.id;
    renameText = project.title;
    await tick();
    renameInput?.focus();
    renameInput?.select();
  }

  function cancelRename() {
    renamingProjectId = "";
    renameText = "";
  }

  async function submitRename(projectId: string) {
    const title = renameText.trim();
    if (!title) {
      return;
    }

    await onRenameProject(projectId, title);
    cancelRename();
  }

  async function removeProject(projectId: string) {
    closeContextMenu();
    await onRemoveProject(projectId);
  }

  function handleCreateKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      createText = "";
      isCreating = false;
      return;
    }

    if (event.key === "Enter") {
      event.preventDefault();
      void submitCreate();
    }
  }

  function handleRenameKeydown(event: KeyboardEvent, projectId: string) {
    if (event.key === "Escape") {
      cancelRename();
      return;
    }

    if (event.key === "Enter") {
      event.preventDefault();
      void submitRename(projectId);
    }
  }

  function handleWindowKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      closeContextMenu();
    }
  }
</script>

<svelte:window onclick={closeContextMenu} onkeydown={handleWindowKeydown} />

<aside class="explorer hair-r">
  <header class="row hair-b">
    <span class="label hot">Explorer</span>
    <div class="flex1"></div>
    <span class="mono-dim">import</span>
  </header>

  <label class="filter row">
    <span>/</span>
    <input bind:value={filterText} aria-label="Filter projects" placeholder="filter projects..." />
    <span class="key">/</span>
  </label>

  <div class="tree">
    <div class="section section-row row" role="presentation" oncontextmenu={(event) => showContextMenu(event)}>
      <span class="label">Projects</span>
      <div class="flex1"></div>
      <button class="section-action" type="button" title="Create Project" onclick={() => void startCreate()}>+</button>
    </div>

    {#if isCreating}
      <div class="tree-row create-item">
        <span class="glyph">=</span>
        <input
          bind:this={createInput}
          bind:value={createText}
          aria-label="New Project title"
          onkeydown={handleCreateKeydown}
          placeholder="new project..."
        />
      </div>
    {/if}

    {#if visibleProjects.length}
      {#each visibleProjects as project}
        {#if renamingProjectId === project.id}
          <div class="tree-row rename-item">
            <span class="glyph">=</span>
            <input
              bind:this={renameInput}
              bind:value={renameText}
              aria-label={`Rename ${project.title}`}
              onkeydown={(event) => handleRenameKeydown(event, project.id)}
            />
          </div>
        {:else}
          <button
            class:active={activeProjectId === project.id}
            class="tree-row folder"
            type="button"
            onclick={() => onOpenProject(project.id, "research")}
            oncontextmenu={(event) => showContextMenu(event, project.id)}
            title={`${project.title} · ${project.vault.path}`}
          >
            <span class="glyph">#</span>
            <span class="folder-dot"></span>
            <span class="truncate">{project.title}</span>
            <span class="count">{project.vault.papers.length}</span>
          </button>
          {#if activeProjectId === project.id}
            <div class="project-children">
              <button
                class:active-child={activeProjectView === "research"}
                class="tree-row child"
                type="button"
                onclick={() => onOpenProject(project.id, "research")}
              >
                <span class="glyph">*</span>
                <span class="truncate">Research</span>
              </button>
              <button
                class:active-child={activeProjectView === "vault"}
                class="tree-row child"
                type="button"
                onclick={() => onOpenProject(project.id, "vault")}
              >
                <span class="glyph">#</span>
                <span class="truncate">Vault</span>
                <span class="count">{project.vault.papers.length}</span>
              </button>
              <button
                class:active-child={activeProjectView === "documents"}
                class="tree-row child"
                type="button"
                onclick={() => onOpenProject(project.id, "documents")}
              >
                <span class="glyph">=</span>
                <span class="truncate">Documents</span>
                <span class="count">{project.documents.length}</span>
              </button>
              {#if activeProjectView === "documents"}
                {#each project.documents as document}
                  <button
                    class="tree-row document-child"
                    type="button"
                    onclick={() => onOpenProjectDocument(project.id, document.id)}
                    title={document.title}
                  >
                    <span class="glyph">.</span>
                    <span class="truncate">{document.title}</span>
                  </button>
                {/each}
              {/if}
            </div>
          {/if}
        {/if}
      {/each}
    {:else}
      <div class="empty-row" role="presentation" oncontextmenu={(event) => showContextMenu(event)}>No Projects</div>
    {/if}
  </div>

  {#if contextMenu}
    <div
      class="context-menu col"
      role="menu"
      style={`left: ${contextMenu.x}px; top: ${contextMenu.y}px;`}
      onclick={(event) => event.stopPropagation()}
      onkeydown={(event) => event.stopPropagation()}
      tabindex="-1"
    >
      <button role="menuitem" type="button" onclick={() => void startCreate()}>Create project</button>
      {#if contextMenu.projectId}
        {@const project = projects.find((candidate) => candidate.id === contextMenu?.projectId)}
        {#if project}
          <button role="menuitem" type="button" onclick={() => void startRename(project)}>Rename project</button>
          <button role="menuitem" class="danger" type="button" onclick={() => void removeProject(project.id)}>
            Remove project
          </button>
        {/if}
      {/if}
    </div>
  {/if}
</aside>

<style>
  .explorer {
    width: 100%;
    height: 100%;
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

  input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: none;
    background: transparent;
    color: var(--fg-2);
    font: inherit;
    font-size: 11px;
  }

  input::placeholder {
    color: var(--fg-3);
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

  .section-row {
    align-items: center;
    padding-bottom: 0;
  }

  .section-action {
    width: 22px;
    height: 22px;
    border: 1px solid var(--border-2);
    background: transparent;
    color: var(--amber-mid);
    font: inherit;
    font-size: 15px;
    line-height: 1;
    cursor: pointer;
  }

  .section-action:hover {
    border-color: var(--amber);
    color: var(--amber);
  }

  .tree-row {
    width: 100%;
    height: 20px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 12px;
    color: var(--fg-1);
    border: 0;
    border-left: 2px solid transparent;
    background: transparent;
    font-size: 11px;
    font-family: inherit;
    text-align: left;
    cursor: pointer;
  }

  .child {
    padding-left: 30px;
  }

  .document-child {
    padding-left: 48px;
    color: var(--fg-3);
  }

  .active-child {
    color: var(--amber);
    background: rgba(242, 169, 59, 0.05);
  }

  .tree-row.active {
    border-left: 2px solid var(--amber);
    background: rgba(242, 169, 59, 0.1);
    color: var(--amber);
  }

  .create-item {
    height: 24px;
    cursor: text;
  }

  .create-item input,
  .rename-item input {
    height: 18px;
    padding: 0 4px;
    border: 1px solid var(--cyan);
    background: var(--bg);
    color: var(--fg);
  }

  .rename-item {
    height: 24px;
    background: rgba(107, 160, 168, 0.05);
    cursor: text;
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

  .count {
    margin-left: auto;
    color: var(--fg-3);
    font-size: 10px;
  }

  .empty-row {
    height: 24px;
    display: flex;
    align-items: center;
    padding: 0 12px 0 24px;
    color: var(--fg-3);
    font-size: 11px;
  }

  .context-menu {
    position: fixed;
    z-index: 20;
    min-width: 144px;
    padding: 5px;
    border: 1px solid var(--border-2);
    border-radius: 3px;
    background: var(--bg-1);
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.38);
  }

  .context-menu button {
    width: 100%;
    height: 26px;
    padding: 0 9px;
    border: 0;
    border-radius: 2px;
    background: transparent;
    color: var(--fg-1);
    font: inherit;
    font-size: 11px;
    text-align: left;
    cursor: pointer;
  }

  .context-menu button:hover {
    background: rgba(242, 169, 59, 0.08);
    color: var(--amber);
  }

  .context-menu .danger {
    margin-top: 4px;
    border-top: 1px solid var(--border);
    border-radius: 0 0 2px 2px;
    color: var(--red);
  }
</style>
