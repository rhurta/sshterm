<script lang="ts">
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
    onsftp,
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
    onlocalopen,
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
    onsftp: (c: ConnectionSummary) => void;
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
    onlocalopen: (shell: "powershell" | "wsl") => void;
  } = $props();

  let menu = $state<MenuEntry | null>(null);
  let menuX = $state(0);
  let menuY = $state(0);
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
    menu = null;
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

  function showMenu(event: MouseEvent, node: TreeNode, parentId: string | null, index: number) {
    event.preventDefault();
    onselect(node);
    menu = { node, parentId, index };
    menuX = Math.min(event.clientX, window.innerWidth - 180);
    menuY = Math.min(event.clientY, window.innerHeight - 340);
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
    // Lets the workspace panes accept a connection dropped from the tree.
    if (node.type === "connection") event.dataTransfer?.setData("application/x-sshterm-conn", node.id);
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
    style="padding-left: {depth * 14 + 8}px; --tree-depth: {depth}"
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
        style={node.type === "folder" ? (node.color ? `--identity-color: ${node.color}` : undefined) : (summary(node)?.identity_color ? `--identity-color: ${summary(node)?.identity_color}` : undefined)}
        class:colored={node.type === "folder" ? !!node.color : !!summary(node)?.identity_color}
        class:folder={node.type === "folder"}
        class:selected={selectedId === node.id}
        class:into={drop?.id === node.id && drop.position === "into"}
        onclick={() => {
          onselect(node);
          if (node.type === "folder") ontoggle(node.id);
          else if (summary(node)) onopen(summary(node)!);
        }}
        ondblclick={() => (node.type === "connection" && summary(node) ? onopen(summary(node)!) : startRename(node))}
        oncontextmenu={(event) => showMenu(event, node, parentId, index)}
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

<div class="tree" role="tree" tabindex="0" aria-label={t("workspace.connections")} class:root-drop={rootDrop} ondragover={onRootDragOver} ondrop={onRootDrop} ondragleave={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) rootDrop = false; }}>
  <div class="line local-line" role="treeitem">
    <button class="row local-row" onclick={() => onlocalopen("powershell")}>
      <span class="conn-icon local-icon"></span>
      <span class="conn"><span class="label">{t("workspace.localPowerShell")}</span><span class="detail">{t("workspace.localTerminal")}</span></span>
      <span class="spacer"></span><span class="local-lock" title={t("workspace.builtInTerminal")}>●</span>
    </button>
  </div>
  <div class="line local-line" role="treeitem">
    <button class="row local-row" onclick={() => onlocalopen("wsl")}>
      <span class="conn-icon local-icon"></span>
      <span class="conn"><span class="label">{t("workspace.localWsl")}</span><span class="detail">{t("workspace.localTerminal")}</span></span>
      <span class="spacer"></span><span class="local-lock" title={t("workspace.builtInTerminal")}>●</span>
    </button>
  </div>
  {#each visible(tree) as node, index (node.id)}
    {@render row(node, null, index, 0)}
  {/each}
  {#if query.trim() && visible(tree).length === 0}
    <p class="empty">{t("tree.noMatches")}</p>
  {/if}
</div>

{#if menu}
  {@const entry = menu}
  <button class="menu-backdrop" aria-label={t("import.close")} onclick={() => (menu = null)}></button>
  <div class="menu" style="left: {menuX}px; top: {menuY}px">
    {#if entry.node.type === "connection" && summary(entry.node)}
      {@const c = summary(entry.node)!}
      <button onclick={() => { onopen(c); menu = null; }}>{t("tree.open")}</button>
      <button onclick={() => { onsftp(c); menu = null; }}>{t("tree.openSftp")}</button>
      <button onclick={() => { onedit(c); menu = null; }}>{t("tree.edit")}</button>
      <button onclick={() => { onduplicate(c); menu = null; }}>{t("tree.duplicate")}</button>
    {/if}
    <button onclick={() => { onnewfolder(menuParent(entry)); menu = null; }}>{t("tree.newFolder")}</button>
    <button onclick={() => { onnewconn(menuParent(entry)); menu = null; }}>{t("tree.newConnection")}</button>
    {#if entry.node.type === "folder"}
      <button onclick={() => { onfolderhighlight(entry.node as Extract<TreeNode, { type: "folder" }>); menu = null; }}>{t("tree.highlightRules")}</button>
    {/if}
    <button onclick={() => startRename(entry.node)}>{t("tree.rename")}</button>
    <div class="separator"></div>
    <button class="danger" onclick={() => { ondelete(entry.node); menu = null; }}>
      {entry.node.type === "folder"
        ? t("tree.confirmFolder", { name: entry.node.name, count: countItems(entry.node) })
        : t("tree.delete")}
    </button>
  </div>
{/if}

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
    position: relative;
    padding-right: 0.5rem;
    box-sizing: border-box;
  }
  .line::before {
    content: "";
    position: absolute;
    pointer-events: none;
    top: 0;
    bottom: 0;
    left: 8px;
    width: calc(var(--tree-depth, 0) * 14px);
    background: repeating-linear-gradient(
      to right,
      transparent 0 6px,
      color-mix(in srgb, var(--border) 85%, transparent) 6px 7px,
      transparent 7px 14px
    );
    opacity: .75;
  }
  .row {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 0.4rem;
    min-height: 1.85rem;
    font: inherit;
    font-size: var(--ui-font-body);
    text-align: left;
    color: var(--text);
    background: transparent;
    border: 1px solid transparent;
    border-radius: 5px;
    padding: 0.22rem 0.38rem;
    cursor: pointer;
    box-sizing: border-box;
  }
  .local-row { padding-left: 0.38rem; }
  .local-icon { border-color: var(--accent); background: color-mix(in srgb, var(--accent) 18%, transparent); }
  .local-lock { color: var(--muted); font-size: .55rem; padding: 0 .25rem; }
  .row.colored { background: color-mix(in srgb, var(--identity-color) 14%, transparent); }
  .row.colored:hover { background: color-mix(in srgb, var(--identity-color) 22%, var(--surface-hover)); }
  .row.colored.selected { background: color-mix(in srgb, var(--identity-color) 25%, var(--surface-raised)); }
  .row:hover {
    background: var(--surface-hover);
  }
  .row.selected {
    border-color: color-mix(in srgb, var(--accent) 55%, var(--border));
    background: color-mix(in srgb, var(--accent) 12%, var(--surface-raised));
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .row:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .row.into {
    border-color: var(--accent);
    border-style: dashed;
  }
  .row.folder {
    font-weight: 550;
    letter-spacing: .005em;
  }
  .chevron {
    width: 0.8rem;
    color: var(--muted);
    flex: 0 0 0.8rem;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: .9rem;
    transition: color .12s ease;
  }
  .row.folder:hover .chevron { color: var(--text); }
  .folder-icon {
    position: relative;
    width: .78rem;
    height: .55rem;
    flex: 0 0 .78rem;
    box-sizing: border-box;
    border: 1px solid color-mix(in srgb, var(--identity-color, var(--accent)) 75%, var(--text));
    border-radius: 2px;
    background: color-mix(in srgb, var(--identity-color, var(--accent)) 12%, transparent);
    margin-top: 2px;
  }
  .folder-icon::before {
    content: "";
    position: absolute;
    left: 1px;
    top: -3px;
    width: .32rem;
    height: 3px;
    border: 1px solid color-mix(in srgb, var(--identity-color, var(--accent)) 75%, var(--text));
    border-bottom: 0;
    border-radius: 2px 2px 0 0;
  }
  .conn-icon {
    width: .62rem;
    height: .62rem;
    box-sizing: border-box;
    border: 1px solid var(--muted);
    border-radius: 3px;
    flex: 0 0 .62rem;
    background: color-mix(in srgb, var(--muted) 10%, transparent);
    position: relative;
  }
  .conn-icon::after {
    content: "";
    position: absolute;
    width: 2px;
    height: 2px;
    left: 2px;
    top: 2px;
    border-radius: 50%;
    background: var(--muted);
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
    min-width: 1.1rem;
    padding: 0 .25rem;
    box-sizing: border-box;
    border-radius: 9px;
    background: var(--surface-deep);
    font-size: var(--ui-font-meta);
    line-height: 1.4;
    text-align: center;
    color: var(--muted);
  }
  .conn {
    display: flex;
    flex-direction: column;
    min-width: 0;
    line-height: 1.25;
  }
  .detail {
    font-size: var(--ui-font-meta);
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
    font-size: var(--ui-font-body);
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
    font-size: var(--ui-font-body);
    padding: 0.5rem 0.75rem;
    margin: 0;
  }
  .menu-backdrop {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: transparent;
    border: 0;
    cursor: default;
  }
  .menu {
    position: fixed;
    z-index: 41;
    width: 11rem;
    background: var(--surface-hover);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.25rem 0;
    display: flex;
    flex-direction: column;
  }
  .menu button {
    font: inherit;
    font-size: var(--ui-font-control);
    text-align: left;
    color: var(--text);
    background: transparent;
    border: 0;
    padding: 0.35rem 0.7rem;
    cursor: pointer;
    white-space: normal;
  }
  .menu button:hover {
    background: var(--border);
  }
  .menu button.danger {
    color: var(--danger);
  }
  .separator {
    height: 1px;
    background: var(--border);
    margin: 0.2rem 0;
  }
</style>
