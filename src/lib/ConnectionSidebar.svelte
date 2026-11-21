<script lang="ts">
  import { untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { hasMessage, t } from "$lib/i18n/index.svelte";
  import ConnectionTree from "./ConnectionTree.svelte";
  import SearchBox from "./SearchBox.svelte";
  import ImportDialog from "./ImportDialog.svelte";
  import FolderHighlightEditor from "./FolderHighlightEditor.svelte";
  import type {
    Connection,
    ConnectionSummary,
    ImportReport,
    SearchHit,
    TreeNode,
  } from "$lib/types";

  let {
    openIds,
    refreshToken,
    onopen,
    onedit,
    onnewconn,
    ontunnels,
    onselectconnection,
  }: {
    openIds: string[];
    refreshToken: number;
    onopen: (c: ConnectionSummary) => void;
    onedit: (c: ConnectionSummary) => void;
    onnewconn: (parentId: string | null) => void;
    ontunnels: () => void;
    onselectconnection: (id: string | null) => void;
  } = $props();

  let tree = $state<TreeNode[]>([]);
  let summaries = $state<Record<string, ConnectionSummary>>({});
  let expanded = $state<Record<string, boolean>>({});
  let hits = $state<string[]>([]);
  let query = $state("");
  let selectedId = $state<string | null>(null);
  let autoRename = $state<string | null>(null);
  let pendingDelete = $state<TreeNode | null>(null);
  let folderHighlightTarget = $state<Extract<TreeNode, { type: "folder" }> | null>(null);
  let showImport = $state(false);
  let error = $state("");
  let searchTimer: ReturnType<typeof setTimeout> | undefined;

  function errText(code: string): string {
    const key = `error.${code}`;
    return hasMessage(key) ? t(key) : t("lock.error.unexpected", { detail: code });
  }

  async function guarded(action: () => Promise<unknown>) {
    try {
      await action();
      error = "";
    } catch (e) {
      error = errText(String(e));
    }
  }

  async function refresh() {
    await guarded(async () => {
      const [nodes, list] = await Promise.all([
        invoke<TreeNode[]>("tree_get"),
        invoke<ConnectionSummary[]>("conn_list"),
      ]);
      tree = nodes;
      summaries = Object.fromEntries(list.map((c) => [c.id, c]));
      // New folders start expanded; keep the user's own toggles.
      const next: Record<string, boolean> = {};
      const walk = (items: TreeNode[]) => {
        for (const item of items) {
          if (item.type === "folder") {
            next[item.id] = expanded[item.id] ?? true;
            walk(item.children);
          }
        }
      };
      walk(nodes);
      expanded = next;
    });
  }
  void refresh();

  // The connection editor lives in Workspace; when it saves, refresh the tree.
  let lastToken = 0;
  $effect(() => {
    if (refreshToken !== lastToken) {
      lastToken = refreshToken;
      untrack(() => refresh());
    }
  });

  function onQuery(value: string) {
    query = value;
    clearTimeout(searchTimer);
    searchTimer = setTimeout(async () => {
      if (!value.trim()) {
        hits = [];
        return;
      }
      await guarded(async () => {
        const result = await invoke<SearchHit[]>("search", { query: value });
        hits = result.map((hit) => hit.id);
      });
    }, 180);
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

  function findParent(nodes: TreeNode[], id: string, parent: string | null = null): string | null {
    for (const node of nodes) {
      if (node.id === id) return parent;
      if (node.type === "folder") {
        const found = findParent(node.children, id, node.id);
        if (found !== null) return found;
      }
    }
    return null;
  }

  function selectedFolder(): string | null {
    if (!selectedId) return null;
    const node = findNode(tree, selectedId);
    return node?.type === "folder" ? node.id : findParent(tree, selectedId);
  }

  function countItems(node: TreeNode): number {
    if (node.type !== "folder") return 0;
    return node.children.reduce(
      (total, child) => total + 1 + (child.type === "folder" ? countItems(child) : 0),
      0,
    );
  }

  async function createFolder(parentId: string | null) {
    await guarded(async () => {
      const id = await invoke<string>("folder_upsert", {
        id: null,
        parentId,
        name: t("tree.newFolder"),
      });
      await refresh();
      autoRename = id;
    });
  }

  async function rename(node: TreeNode, name: string) {
    await guarded(async () => {
      if (node.type === "folder") {
        await invoke("folder_upsert", { id: node.id, parentId: null, name });
      } else {
        const full = await invoke<Connection | null>("conn_get", { id: node.id });
        if (full) await invoke("conn_upsert", { conn: { ...full, name }, parentId: null });
      }
      await refresh();
    });
  }

  async function duplicate(c: ConnectionSummary) {
    await guarded(async () => {
      const full = await invoke<Connection | null>("conn_get", { id: c.id });
      if (!full) return;
      const copy: Connection = {
        ...full,
        id: crypto.randomUUID(),
        name: `${full.name} (copy)`,
      };
      await invoke("conn_upsert", { conn: copy, parentId: findParent(tree, c.id) });
      await refresh();
    });
  }

  async function moveNode(id: string, parentId: string | null, index: number) {
    await guarded(async () => {
      await invoke("move_node", { id, parentId, index });
      await refresh();
    });
  }

  async function confirmDelete() {
    const node = pendingDelete;
    pendingDelete = null;
    if (!node) return;
    await guarded(async () => {
      if (node.type === "folder") {
        await invoke("folder_delete", { id: node.id, cascade: true });
      } else {
        await invoke("conn_delete", { id: node.id });
      }
      await refresh();
    });
  }

  function deleteLabel(node: TreeNode): string {
    return node.type === "folder"
      ? t("tree.confirmFolder", { name: node.name, count: countItems(node) })
      : t("tree.confirmConnection", { name: summaries[node.id]?.name ?? node.id });
  }
</script>

<aside>
  <div class="head workspace-panel-header">
    <span class="title panel-title">{t("workspace.connections")}</span>
    <div class="head-actions"><button class="icon-action" onclick={ontunnels} title={t("tunnels.open")} aria-label={t("tunnels.open")}><svg viewBox="0 0 24 24"><path d="M4 8h11a4 4 0 0 1 0 8H9m0 0 3-3m-3 3 3 3M20 8l-3-3m3 3-3 3"/></svg></button><button class="icon-action" onclick={() => createFolder(selectedFolder())} title={t("tree.newFolder")} aria-label={t("tree.newFolder")}><svg viewBox="0 0 24 24"><path d="M3 6.5h7l2 2H21v10H3z"/></svg></button><button class="icon-action" onclick={() => onnewconn(selectedFolder())} title={t("tree.newConnection")} aria-label={t("tree.newConnection")}><svg viewBox="0 0 24 24"><circle cx="6.5" cy="7" r="2.5"/><circle cx="17.5" cy="6" r="2.5"/><circle cx="17.5" cy="18" r="2.5"/><path d="M9 7h6m-6 2 6 7"/></svg></button><button class="icon-action" onclick={() => (showImport = true)} title={t("tree.import")} aria-label={t("tree.import")}><svg viewBox="0 0 24 24"><path d="M12 3v12m-4-4 4 4 4-4M4 17v4h16v-4"/></svg></button></div>
  </div>

  <SearchBox value={query} count={hits.length} onchange={onQuery} autofocus />

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}

  {#if pendingDelete}
    <div class="confirm">
      <span>{deleteLabel(pendingDelete)}</span>
      <button onclick={confirmDelete}>{t("tree.delete")}</button>
      <button class="ghost" onclick={() => (pendingDelete = null)}>{t("editor.cancel")}</button>
    </div>
  {/if}

  <ConnectionTree
    {tree}
    {summaries}
    {expanded}
    {hits}
    {query}
    {openIds}
    {selectedId}
    {autoRename}
    {onopen}
    onedit={(c) => onedit(c)}
    onduplicate={(c) => duplicate(c)}
    ondelete={(node) => (pendingDelete = node)}
    onrename={(node, name) => rename(node, name)}
    onnewfolder={(parentId) => createFolder(parentId)}
    onnewconn={(parentId) => onnewconn(parentId)}
    onmove={(id, parentId, index) => moveNode(id, parentId, index)}
    onfolderhighlight={(folder) => (folderHighlightTarget = folder)}
    onselect={(node) => { selectedId = node.id; onselectconnection(node.type === "connection" ? node.id : null); }}
    ontoggle={(id) => (expanded = { ...expanded, [id]: !expanded[id] })}
  />

  <div class="foot"><span class="hint">{t("tree.hint")}</span></div>
</aside>

{#if folderHighlightTarget}
  <FolderHighlightEditor
    folder={folderHighlightTarget}
    oncancel={() => (folderHighlightTarget = null)}
    onsave={async (id, highlight) => {
      try {
        await invoke("folder_set_highlight", { id, highlight });
        folderHighlightTarget = null;
        await refresh();
        return null;
      } catch (e) {
        return errText(String(e));
      }
    }}
  />
{/if}

{#if showImport}
  <ImportDialog
    onclose={() => (showImport = false)}
    ondone={(_report: ImportReport) => refresh()}
  />
{/if}

<style>
  aside {
    width: 16.5rem;
    flex: 0 0 16.5rem;
    border-right: 1px solid var(--border);
    background: var(--surface);
    padding: 0;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow: hidden;
  }
  .head { display:flex; align-items:center; gap:.35rem; padding:0 12px; flex-wrap:nowrap; }
  .head-actions { display:flex; align-items:center; gap:3px; }
  .head .title { flex:1 1 auto; min-width:0; margin:0; }
  .head .icon-action { width:28px; height:28px; flex:0 0 28px; display:inline-flex; align-items:center; justify-content:center; padding:0; color:var(--muted); background:transparent; border:1px solid transparent; }
  .head .icon-action:hover { color:var(--accent); background:var(--surface-hover); border-color:var(--border); }
  .head .icon-action svg { width:16px; height:16px; fill:none; stroke:currentColor; stroke-width:1.7; stroke-linecap:round; stroke-linejoin:round; }
  .title {
    font-size: var(--ui-font-section);
    font-weight: 600;
    color: var(--text);
    letter-spacing: normal;
    text-transform: none;
    flex: 1 1 auto;
    min-width: 0;
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  button {
    font: inherit;
    font-size: var(--ui-font-dense);
    cursor: pointer;
    border: 1px solid var(--border);
    background: var(--surface-hover);
    color: var(--text);
    border-radius: 4px;
    padding: 0.3rem 0.45rem;
  }
  button:hover {
    border-color: var(--accent);
  }
  .error {
    margin: 0;
    padding: 0.3rem 0.65rem;
    background: var(--danger-bg);
    color: var(--danger);
    font-size: var(--ui-font-control);
  }
  .confirm {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.4rem 0.65rem;
    background: var(--danger-bg);
    font-size: var(--ui-font-control);
    flex-wrap: wrap;
  }
  .confirm span {
    flex: 1 0 100%;
  }
  .confirm button {
    font-size: var(--ui-font-dense);
    background: var(--danger);
    border: 0;
    color: var(--accent-contrast);
  }
  .confirm button.ghost {
    background: transparent;
    color: var(--text-secondary);
    border: 1px solid var(--border);
  }
  .foot {
    flex: 0 0 auto;
    border-top: 1px solid var(--border);
    padding: 0.5rem 0.65rem;
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .hint {
    font-size: var(--ui-font-meta);
    color: var(--muted);
    line-height: 1.3;
  }
</style>
