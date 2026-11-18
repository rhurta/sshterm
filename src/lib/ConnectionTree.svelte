<script lang="ts">
  import { ContextMenu } from "bits-ui";
  import "$lib/ui/menu.css";
  import { t } from "$lib/i18n/index.svelte";
  import type { ConnectionSummary, TreeNode } from "$lib/types";

  type DropPosition = "before" | "after" | "into";
  type MenuEntry = { node: TreeNode; parentId: string | null; index: number };

  let {
    tree,
    summaries,
    expanded,
    hits,
    query,
    openIds,
    selectedId,
    autoRename,
    onopen,
    onselect,
    ontoggle,
    onrename,
    onnewfolder,
    onnewconn,
    onedit,
    onduplicate,
    ondelete,
    onmove,
    onfolderhighlight,
  }: {
    tree: TreeNode[];
    summaries: Record<string, ConnectionSummary>;
    expanded: Record<string, boolean>;
    hits: string[];
    query: string;
    openIds: string[];
    selectedId: string | null;
    autoRename: string | null;
    onopen: (c: ConnectionSummary) => void;
    onselect: (node: TreeNode) => void;
    ontoggle: (id: string) => void;
    onrename: (node: TreeNode, name: string) => void;
    onnewfolder: (parentId: string | null) => void;
    onnewconn: (parentId: string | null) => void;
    onedit: (c: ConnectionSummary) => void;
    onduplicate: (c: ConnectionSummary) => void;
    ondelete: (node: TreeNode) => void;
    onmove: (id: string, parentId: string | null, index: number) => void;
    onfolderhighlight: (folder: Extract<TreeNode, { type: "folder" }>) => void;
  } = $props();

  let menu = $state<MenuEntry | null>(null);
  let renaming = $state<string | null>(null);
  let renameText = $state("");
  let dragId = $state<string | null>(null);
  let drop = $state<{ id: string; position: DropPosition } | null>(null);
  let rootDrop = $state(false);

  function isOpen(node: TreeNode): boolean {
    return query.trim() ? true : !!expanded[node.id];
  }

  /** During a search, keep only folders that lead to a hit. */
  function visible(nodes: TreeNode[]): TreeNode[] {
    if (!query.trim()) return nodes;
    return nodes.flatMap((node): TreeNode[] => {
      if (node.type === "folder") {
        const children = visible(node.children);
        return children.length ? [{ ...node, children }] : [];
      }
      return hits.includes(node.id) ? [node] : [];
    });
  }

  /** Splits a label into matched / unmatched segments (no HTML injection). */
  function parts(text: string): { text: string; hit: boolean }[] {
    const needle = query.trim().toLowerCase();
    if (!needle) return [{ text, hit: false }];
    const result: { text: string; hit: boolean }[] = [];
    let cursor = 0;
    while (cursor <= text.length) {
      const at = text.toLowerCase().indexOf(needle, cursor);
      if (at < 0) {
        result.push({ text: text.slice(cursor), hit: false });
        break;
      }
      if (at > cursor) result.push({ text: text.slice(cursor, at), hit: false });
      result.push({ text: text.slice(at, at + needle.length), hit: true });
      cursor = at + needle.length;
    }
    return result;
  }

  function countItems(node: TreeNode): number {
    if (node.type !== "folder") return 0;
    return node.children.reduce(
      (total, child) => total + 1 + (child.type === "folder" ? countItems(child) : 0),
      0,
    );
  }

  function findNode(nodes: TreeNode[], id: string): TreeNode | null {
    for (const node of nodes) {
      if (node.id === id) return node;
      if (node.type === "folder") {
        const found = findNode(node.children, id);
        if (found) return found;
      }
    }
    return null;
  }

  function findLocation(
    nodes: TreeNode[],
    id: string,
    parentId: string | null = null,
  ): { parentId: string | null; index: number } | null {
    for (let index = 0; index < nodes.length; index++) {
      const node = nodes[index];
      if (node.id === id) return { parentId, index };
      if (node.type === "folder") {
        const found = findLocation(node.children, id, node.id);
        if (found) return found;
      }
    }
    return null;
  }

  function moveAt(id: string, parentId: string | null, index: number) {
    const source = findLocation(tree, id);
    const adjustedIndex =
      source?.parentId === parentId && source.index < index ? index - 1 : index;
    onmove(id, parentId, Math.max(0, adjustedIndex));
  }

  function focusOnMount(node: HTMLInputElement) {
    node.focus();
    node.select();
  }

  function startRename(node: TreeNode) {
    renaming = node.id;
    renameText = node.type === "folder" ? node.name : (summaries[node.id]?.name ?? "");
  }

  function commitRename() {
    const id = renaming;
    renaming = null;
    const name = renameText.trim();
    if (!id || !name) return;
    const node = findNode(tree, id);
    if (node) onrename(node, name);
  }

  // The menu itself is a Bits UI ContextMenu: it opens at the pointer, handles
  // keyboard navigation, focus, Escape and screen readers. It must not be
  // preventDefault()ed here, or Bits ignores the event.
  function showMenu(node: TreeNode, parentId: string | null, index: number) {
    onselect(node);
    menu = { node, parentId, index };
  }

  /**
   * Keyboard navigation: Up/Down/Home/End move focus between visible rows,
   * Right/Left expand/collapse the focused folder. Enter/Space activate the
   * focused row button, Shift+F10 or the Menu key open the context menu, F2
   * renames. Modified keys (Ctrl+Alt shortcuts) are left to the workspace.
   */
  function onTreeKeydown(event: KeyboardEvent) {
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    if (event.target instanceof HTMLInputElement) return; // rename field
    const rows = [...(event.currentTarget as HTMLElement).querySelectorAll<HTMLButtonElement>("button.row")];
    if (rows.length === 0) return;
    const index = rows.indexOf(document.activeElement as HTMLButtonElement);
    const focusRow = (i: number) => {
      event.preventDefault();
      rows[Math.max(0, Math.min(rows.length - 1, i))].focus();
    };
    if (event.key === "ArrowDown") focusRow(index < 0 ? 0 : index + 1);
    else if (event.key === "ArrowUp") focusRow(index < 0 ? 0 : index - 1);
    else if (event.key === "Home") focusRow(0);
    else if (event.key === "End") focusRow(rows.length - 1);
    else if ((event.key === "ArrowRight" || event.key === "ArrowLeft") && index >= 0) {
      const node = findNode(tree, rows[index].dataset.nodeId ?? "");
      if (node?.type !== "folder" || query.trim()) return;
      const open = isOpen(node);
      if ((event.key === "ArrowRight" && !open) || (event.key === "ArrowLeft" && open)) {
        event.preventDefault();
        ontoggle(node.id);
      }
    }
  }

  /** Right click on empty tree space: offer only the root-level actions. */
  function onTreeContextMenu(event: MouseEvent) {
    if (!(event.target as Element | null)?.closest(".line")) menu = null;
  }

  function menuParent(entry: MenuEntry): string | null {
    return entry.node.type === "folder" ? entry.node.id : entry.parentId;
  }

  function summary(node: TreeNode): ConnectionSummary | null {
    return summaries[node.id] ?? null;
  }

  function containsNode(node: TreeNode, id: string): boolean {
    return node.id === id || (node.type === "folder" && node.children.some((child) => containsNode(child, id)));
  }

  function onDragStart(event: DragEvent, node: TreeNode) {
    if (query.trim()) {
      event.preventDefault();
      return;
    }
    dragId = node.id;
    event.dataTransfer?.setData("text/plain", node.id);
    if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
  }

  function onDragOver(event: DragEvent, node: TreeNode) {
    if (query.trim() || !dragId) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
    const dragged = findNode(tree, dragId);
    if (!dragged || containsNode(dragged, node.id)) {
      drop = null;
      if (event.dataTransfer) event.dataTransfer.dropEffect = "none";
      return;
    }
    const bounds = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const offset = event.clientY - bounds.top;
    const edge = Math.min(10, bounds.height * 0.25);
    let position: DropPosition;
    if (node.type === "folder" && offset >= edge && offset <= bounds.height - edge) {
      position = "into";
    } else {
      position = offset < bounds.height / 2 ? "before" : "after";
    }
    drop = { id: node.id, position };
    rootDrop = false;
  }

  function onDrop(event: DragEvent, node: TreeNode, parentId: string | null, index: number) {
    event.preventDefault();
    event.stopPropagation();
    const id = event.dataTransfer?.getData("text/plain") || dragId;
    const position = drop?.position;
    drop = null;
    rootDrop = false;
    dragId = null;
    if (!id || !position) return;
    if (position === "into" && node.type === "folder") {
      moveAt(id, node.id, node.children.length);
    } else {
      moveAt(id, parentId, index + (position === "after" ? 1 : 0));
    }
  }

  function onRootDragOver(event: DragEvent) {
    if (query.trim() || !dragId) return;
    // Rows handle their own drop positions; only the blank tree area means root.
    if ((event.target as HTMLElement).closest(".line")) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
    drop = null;
    rootDrop = true;
  }

  function onRootDrop(event: DragEvent) {
    if ((event.target as HTMLElement).closest(".line")) return;
    event.preventDefault();
    event.stopPropagation();
    const id = event.dataTransfer?.getData("text/plain") || dragId;
    if (id && !query.trim()) moveAt(id, null, tree.length);
    clearDrag();
  }

  function clearDrag() {
    dragId = null;
    drop = null;
    rootDrop = false;
  }

  // A newly created folder is put straight into inline rename mode.
  let handledAutoRename = "";
  $effect(() => {
    if (autoRename && autoRename !== handledAutoRename) {
      handledAutoRename = autoRename;
      const node = findNode(tree, autoRename);
      if (node) startRename(node);
    }
  });
</script>

{#snippet row(node: TreeNode, parentId: string | null, index: number, depth: number)}
  {#if drop?.id === node.id && drop.position === "before"}
    <div class="drop-line" style="margin-left: {depth * 14 + 8}px"></div>
  {/if}
  <div
    class="line"
    role="treeitem"
    aria-selected={selectedId === node.id}
    tabindex="-1"
    style="padding-left: {depth * 14 + 8}px"
    draggable={!query.trim() && renaming !== node.id}
    ondragstart={(event) => onDragStart(event, node)}
    ondragover={(event) => onDragOver(event, node)}
    ondrop={(event) => onDrop(event, node, parentId, index)}
    ondragend={clearDrag}
  >
    {#if renaming === node.id}
      <input
        class="rename"
        bind:value={renameText}
        use:focusOnMount
        onclick={(event) => event.stopPropagation()}
        onkeydown={(event) => {
          if (event.key === "Enter") commitRename();
          if (event.key === "Escape") renaming = null;
        }}
        onblur={commitRename}
      />
    {:else}
      <button
        class="row"
        data-node-id={node.id}
        class:folder={node.type === "folder"}
        class:selected={selectedId === node.id}
        class:into={drop?.id === node.id && drop.position === "into"}
        onclick={() => {
          onselect(node);
          if (node.type === "folder") ontoggle(node.id);
          else if (summary(node)) onopen(summary(node)!);
        }}
        ondblclick={() => (node.type === "connection" && summary(node) ? onopen(summary(node)!) : startRename(node))}
        oncontextmenu={() => showMenu(node, parentId, index)}
        onkeydown={(event) => {
          if (event.key === "F2") startRename(node);
        }}
      >
        {#if node.type === "folder"}
          <span class="chevron">{isOpen(node) ? "▾" : "▸"}</span>
          <span class="folder-icon"></span>
          <span class="label">{#each parts(node.name) as part}<span class:hit={part.hit}>{part.text}</span>{/each}</span>
          <span class="spacer"></span>
          <span class="badge">{node.children.length}</span>
        {:else}
          <span class="conn-icon"></span>
          <span class="conn">
            <span class="label">
              {#each parts(summaries[node.id]?.name ?? node.id) as part}<span class:hit={part.hit}>{part.text}</span>{/each}
              {#if openIds.includes(node.id)}<span class="live"></span>{/if}
            </span>
            <span class="detail">{summaries[node.id]?.user ?? ""}@{summaries[node.id]?.host ?? ""}:{summaries[node.id]?.port ?? 22}</span>
          </span>
        {/if}
      </button>
    {/if}
  </div>
  {#if drop?.id === node.id && drop.position === "after"}
    <div class="drop-line" style="margin-left: {depth * 14 + 8}px"></div>
  {/if}
  {#if node.type === "folder" && isOpen(node)}
    {#each visible(node.children) as child, childIndex (child.id)}
      {@render row(child, node.id, childIndex, depth + 1)}
    {/each}
  {/if}
{/snippet}

<ContextMenu.Root>
  <ContextMenu.Trigger
    role="tree"
    tabindex={0}
    aria-label={t("workspace.connections")}
    ondragover={onRootDragOver}
    ondrop={onRootDrop}
    ondragleave={(event: DragEvent) => {
      if (!(event.currentTarget as Element).contains(event.relatedTarget as Node | null)) rootDrop = false;
    }}
    oncontextmenu={onTreeContextMenu}
    onkeydown={onTreeKeydown}
    data-nav="connection-tree"
  >
    {#snippet child({ props })}
      <div {...props} class="tree" class:root-drop={rootDrop}>
        {#each visible(tree) as node, index (node.id)}
          {@render row(node, null, index, 0)}
        {/each}
        {#if query.trim() && visible(tree).length === 0}
          <p class="empty">{t("tree.noMatches")}</p>
        {/if}
      </div>
    {/snippet}
  </ContextMenu.Trigger>
  <!-- Portaled into .window-shell, where the theme variables live. -->
  <ContextMenu.Portal to=".window-shell">
    <ContextMenu.Content class="ui-menu" collisionPadding={8}>
      {#if menu && menu.node.type === "connection" && summary(menu.node)}
        {@const c = summary(menu.node)!}
        <ContextMenu.Item class="ui-menu-item" onSelect={() => onopen(c)}>{t("tree.open")}</ContextMenu.Item>
        <ContextMenu.Item class="ui-menu-item" onSelect={() => onedit(c)}>{t("tree.edit")}</ContextMenu.Item>
        <ContextMenu.Item class="ui-menu-item" onSelect={() => onduplicate(c)}>{t("tree.duplicate")}</ContextMenu.Item>
      {/if}
      <ContextMenu.Item class="ui-menu-item" onSelect={() => onnewfolder(menu ? menuParent(menu) : null)}>
        {t("tree.newFolder")}
      </ContextMenu.Item>
      <ContextMenu.Item class="ui-menu-item" onSelect={() => onnewconn(menu ? menuParent(menu) : null)}>
        {t("tree.newConnection")}
      </ContextMenu.Item>
      {#if menu}
        {@const entry = menu}
        {#if entry.node.type === "folder"}
          {@const folder = entry.node}
          <ContextMenu.Item class="ui-menu-item" onSelect={() => onfolderhighlight(folder)}>
            {t("tree.highlightRules")}
          </ContextMenu.Item>
        {/if}
        <ContextMenu.Item class="ui-menu-item" onSelect={() => startRename(entry.node)}>{t("tree.rename")}</ContextMenu.Item>
        <ContextMenu.Separator class="ui-menu-sep" />
        <ContextMenu.Item class="ui-menu-item danger" onSelect={() => ondelete(entry.node)}>
          {entry.node.type === "folder"
            ? t("tree.confirmFolder", { name: entry.node.name, count: countItems(entry.node) })
            : t("tree.delete")}
        </ContextMenu.Item>
      {/if}
    </ContextMenu.Content>
  </ContextMenu.Portal>
</ContextMenu.Root>

<style>
  .tree {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding-bottom: 0.5rem;
    scrollbar-width: thin;
    scrollbar-color: var(--muted) transparent;
  }
  .tree::-webkit-scrollbar { width: 8px; }
  .tree::-webkit-scrollbar-track { background: transparent; }
  .tree::-webkit-scrollbar-thumb { background: var(--muted); border-radius: 6px; }
  .line {
    padding-right: 0.5rem;
    box-sizing: border-box;
  }
  .row {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 0.4rem;
    min-height: 1.7rem;
    font: inherit;
    font-size: 0.8rem;
    text-align: left;
    color: var(--text);
    background: transparent;
    border: 1px solid transparent;
    border-radius: 4px;
    padding: 0.15rem 0.3rem;
    cursor: pointer;
    box-sizing: border-box;
  }
  .row:hover {
    background: var(--surface-hover);
  }
  .row.selected {
    border-color: var(--accent);
    background: var(--surface-hover);
  }
  .row.into {
    border-color: var(--accent);
    border-style: dashed;
  }
  .row.folder {
    font-weight: 500;
  }
  .chevron {
    width: 0.75rem;
    color: var(--muted);
    flex: 0 0 0.75rem;
  }
  .folder-icon {
    width: 0.6rem;
    height: 0.5rem;
    border: 1px solid var(--accent);
    border-radius: 2px;
    flex: 0 0 0.6rem;
  }
  .conn-icon {
    width: 0.4rem;
    height: 0.4rem;
    border: 1px solid var(--muted);
    border-radius: 50%;
    flex: 0 0 0.4rem;
  }
  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .label :global(.hit) {
    background: var(--warning-highlight);
    color: var(--warning-highlight-text);
    border-radius: 2px;
  }
  .spacer {
    flex: 1;
  }
  .badge {
    font-size: 0.7rem;
    color: var(--muted);
  }
  .conn {
    display: flex;
    flex-direction: column;
    min-width: 0;
    line-height: 1.25;
  }
  .detail {
    font-size: 0.7rem;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .live {
    display: inline-block;
    width: 0.4rem;
    height: 0.4rem;
    margin-left: 0.35rem;
    border-radius: 50%;
    background: var(--success);
    vertical-align: middle;
  }
  .rename {
    width: 100%;
    font: inherit;
    font-size: 0.8rem;
    color: var(--text);
    background: var(--app-bg);
    border: 1px solid var(--accent);
    border-radius: 4px;
    padding: 0.2rem 0.35rem;
    box-sizing: border-box;
  }
  .drop-line {
    height: 2px;
    margin-right: 0.5rem;
    background: var(--accent);
  }
  .empty {
    color: var(--muted);
    font-size: 0.8rem;
    padding: 0.5rem 0.75rem;
    margin: 0;
  }
</style>
