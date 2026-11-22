<script lang="ts">
  import { onMount, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { hasMessage, t, type MessageKey } from "$lib/i18n/index.svelte";
  import PasswordInput from "./PasswordInput.svelte";
  import { findScriptNode, folderOptions, parentFolderId, suggestScriptName, targetFolderId, visibleRows } from "./scriptsTree";
  import type { ScriptDetail, ScriptsStatus, ScriptSummary } from "$lib/types";

  let {
    mode,
    draft,
    loaded,
    onclose,
    onload,
    onsaved,
  }: {
    mode: "save" | "load";
    /** Text currently in the command editor (what Save stores). */
    draft: string;
    /** Script the editor text came from, if any (Save can update it). */
    loaded: { id: string; name: string } | null;
    onclose: () => void;
    onload: (script: ScriptDetail, how: "replace" | "append") => void;
    onsaved: (script: { id: string; name: string }) => void;
  } = $props();

  let status = $state<ScriptsStatus | null>(null);
  let tree = $state<ScriptSummary[]>([]);
  let expanded = $state<Set<string>>(new Set());
  let selectedId = $state<string | null>(null);
  let query = $state("");
  let matchIds = $state<Set<string>>(new Set());
  let preview = $state<ScriptDetail | null>(null);
  let name = $state("");
  let error = $state("");
  let busy = $state(false);
  let unlockPassword = $state("");
  // Save mode: mark the saved script as a favorite (a button under the console).
  let favoriteChecked = $state(false);

  type Edit = { kind: "newFolder" | "rename" | "move"; value: string };
  let edit = $state<Edit | null>(null);
  type Confirm = { kind: "delete"; id: string } | { kind: "overwrite"; asNew: boolean } | { kind: "draft" };
  let confirm = $state<Confirm | null>(null);

  let searchEl: HTMLInputElement | undefined = $state();
  let nameEl: HTMLInputElement | undefined = $state();
  let editEl: HTMLInputElement | HTMLSelectElement | undefined = $state();
  let unlockEl: { focus: () => void } | undefined = $state();

  const rows = $derived(visibleRows(tree, expanded, query, matchIds));
  const selected = $derived(selectedId ? findScriptNode(tree, selectedId) : null);
  const targetFolder = $derived(targetFolderId(tree, selectedId));
  const draftEmpty = $derived(draft.trim().length === 0);
  const noScripts = $derived(tree.length === 0);

  function message(raw: string): string {
    const code = raw.split(":")[0];
    const key = `scripts.error.${code}`;
    return hasMessage(key) ? t(key as MessageKey) : t("scripts.error.unknown", { detail: raw });
  }

  async function refresh() {
    status = await invoke<ScriptsStatus>("scripts_status");
    if (status.state === "ready") {
      tree = await invoke<ScriptSummary[]>("scripts_tree");
      if (selectedId && !findScriptNode(tree, selectedId)) {
        selectedId = null;
        preview = null;
      }
    }
  }

  function expandAncestors(id: string) {
    const next = new Set(expanded);
    let parent = parentFolderId(tree, id);
    while (parent) {
      next.add(parent);
      parent = parentFolderId(tree, parent) ?? null;
    }
    expanded = next;
  }

  onMount(async () => {
    try {
      await refresh();
    } catch (e) {
      error = message(String(e));
    }
    if (status?.state === "ready" && loaded && findScriptNode(tree, loaded.id)) {
      selectedId = loaded.id;
      expandAncestors(loaded.id);
      void loadPreview(loaded.id);
    }
    name = mode === "save" ? (loaded?.name ?? suggestScriptName(draft)) : "";
    const loadedNode = loaded ? findScriptNode(tree, loaded.id) : null;
    favoriteChecked = loadedNode?.type === "script" ? loadedNode.favorite : false;
    await tick();
    // Save mode focuses the name (the main thing to type); load mode focuses the search.
    if (status?.state === "needs_password") unlockEl?.focus();
    else if (mode === "save") {
      nameEl?.focus();
      nameEl?.select();
    } else searchEl?.focus();
  });

  // Content search (names and bodies) runs on the backend; folder names match locally.
  $effect(() => {
    const q = query.trim();
    if (!q || status?.state !== "ready") {
      matchIds = new Set();
      return;
    }
    const timer = setTimeout(async () => {
      try {
        matchIds = new Set(await invoke<string[]>("scripts_search", { query: q }));
      } catch {
        matchIds = new Set();
      }
    }, 150);
    return () => clearTimeout(timer);
  });

  async function loadPreview(id: string) {
    try {
      preview = await invoke<ScriptDetail>("scripts_get", { id });
    } catch {
      preview = null;
    }
  }

  async function select(id: string) {
    selectedId = id;
    error = "";
    const node = findScriptNode(tree, id);
    if (node?.type === "script") {
      await loadPreview(id);
      if (mode === "save") {
        name = node.name; // like a file dialog: picking a file fills the name
        favoriteChecked = node.favorite;
      }
    } else {
      preview = null;
    }
  }

  function toggle(id: string) {
    const next = new Set(expanded);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    expanded = next;
  }

  async function focusRow(id: string) {
    await tick();
    document.querySelector<HTMLElement>(`[data-row-id="${id}"]`)?.focus();
  }

  function onTreeKeydown(event: KeyboardEvent) {
    if (edit || confirm) return;
    const at = rows.findIndex((row) => row.node.id === selectedId);
    const go = (index: number) => {
      const row = rows[Math.max(0, Math.min(rows.length - 1, index))];
      if (row) {
        void select(row.node.id);
        void focusRow(row.node.id);
      }
    };
    const row = at >= 0 ? rows[at] : undefined;
    switch (event.key) {
      case "ArrowDown": event.preventDefault(); go(at < 0 ? 0 : at + 1); break;
      case "ArrowUp": event.preventDefault(); go(at < 0 ? 0 : at - 1); break;
      case "Home": event.preventDefault(); go(0); break;
      case "End": event.preventDefault(); go(rows.length - 1); break;
      case "ArrowRight":
        if (row?.node.type === "folder") {
          event.preventDefault();
          if (!row.open) toggle(row.node.id);
          else go(at + 1);
        }
        break;
      case "ArrowLeft":
        if (row?.node.type === "folder" && row.open && !query.trim()) {
          event.preventDefault();
          toggle(row.node.id);
        } else if (row) {
          const parent = parentFolderId(tree, row.node.id);
          if (parent) {
            event.preventDefault();
            void select(parent);
            void focusRow(parent);
          }
        }
        break;
      case "Enter":
        if (!row) break;
        event.preventDefault();
        if (row.node.type === "folder") toggle(row.node.id);
        else if (mode === "load") requestLoad();
        else nameEl?.focus();
        break;
      case "F2": if (row) { event.preventDefault(); startEdit("rename"); } break;
      case "Delete": if (row) { event.preventDefault(); askDelete(); } break;
    }
  }

  function onSearchKeydown(event: KeyboardEvent) {
    if (event.key === "ArrowDown" && rows.length > 0) {
      event.preventDefault();
      const id = selectedId && rows.some((row) => row.node.id === selectedId) ? selectedId : rows[0].node.id;
      void select(id);
      void focusRow(id);
    }
  }

  function onDialogKeydown(event: KeyboardEvent) {
    if (event.key !== "Escape") return;
    // Escape peels one layer at a time: inline edit, confirmation, search, then the dialog.
    event.preventDefault();
    event.stopPropagation();
    if (edit) edit = null;
    else if (confirm) confirm = null;
    else if (query) query = "";
    else onclose();
  }

  // --- tree management ------------------------------------------------------
  async function run(action: () => Promise<void>) {
    busy = true;
    error = "";
    try {
      await action();
    } catch (e) {
      error = message(String(e));
    } finally {
      busy = false;
    }
  }

  async function startEdit(kind: Edit["kind"]) {
    if (kind !== "newFolder" && !selected) return;
    error = "";
    confirm = null;
    edit = {
      kind,
      value: kind === "rename" ? (selected?.name ?? "") : kind === "move" ? (parentFolderId(tree, selectedId ?? "") ?? "") : "",
    };
    await tick();
    editEl?.focus();
    if (editEl instanceof HTMLInputElement) editEl.select();
  }

  function applyEdit() {
    const current = edit;
    if (!current) return;
    void run(async () => {
      if (current.kind === "newFolder") {
        const id = await invoke<string>("scripts_create_folder", { parentId: targetFolder, name: current.value });
        await refresh();
        if (targetFolder) expanded = new Set(expanded).add(targetFolder);
        selectedId = id;
        preview = null;
      } else if (current.kind === "rename" && selectedId) {
        await invoke("scripts_rename", { id: selectedId, name: current.value });
        await refresh();
        if (mode === "save" && selected?.type === "script") name = current.value.trim();
      } else if (current.kind === "move" && selectedId) {
        await invoke("scripts_move", { id: selectedId, parentId: current.value || null });
        await refresh();
        expandAncestors(selectedId);
      }
      edit = null;
    });
  }

  const selectedScript = $derived(selected?.type === "script" ? selected : null);

  function toggleFavorite() {
    const script = selectedScript;
    if (!script) return;
    void run(async () => {
      await invoke("scripts_set_favorite", { id: script.id, favorite: !script.favorite });
      await refresh();
      if (mode === "save" && name.trim().toLowerCase() === script.name.toLowerCase()) favoriteChecked = !script.favorite;
    });
  }

  function askDelete() {
    if (!selectedId) return;
    edit = null;
    confirm = { kind: "delete", id: selectedId };
  }

  function doDelete(id: string) {
    void run(async () => {
      await invoke("scripts_delete", { id });
      confirm = null;
      selectedId = null;
      preview = null;
      await refresh();
    });
  }

  // --- load / save ----------------------------------------------------------
  function requestLoad() {
    if (!preview) return;
    if (draftEmpty) finishLoad("replace");
    else confirm = { kind: "draft" };
  }

  function finishLoad(how: "replace" | "append") {
    if (!preview) return;
    onload(preview, how);
    onclose();
  }

  function save(asNew: boolean, overwrite = false) {
    const trimmed = name.trim();
    if (!trimmed || draftEmpty) return;
    void run(async () => {
      const updating = !asNew && loaded !== null;
      try {
        const id = await invoke<string>("scripts_save", {
          // Fields of the request object keep their Rust (snake_case) names.
          request: { id: updating ? loaded!.id : null, folder_id: targetFolder, name: trimmed, body: draft, overwrite, favorite: favoriteChecked },
        });
        onsaved({ id, name: trimmed });
        onclose();
      } catch (e) {
        // A new script with an existing name asks before replacing it.
        if (String(e).split(":")[0] === "name_exists" && !updating && !overwrite) {
          confirm = { kind: "overwrite", asNew };
          return;
        }
        throw e;
      }
    });
  }

  function unlock() {
    if (!unlockPassword) return;
    void run(async () => {
      try {
        await invoke("scripts_unlock", { password: unlockPassword });
      } catch (e) {
        unlockPassword = "";
        throw e;
      }
      unlockPassword = "";
      await refresh();
    });
  }

  const moveOptions = $derived(folderOptions(tree, selected?.type === "folder" ? selected.id : null));
</script>

<div class="backdrop" role="presentation" onkeydown={onDialogKeydown}>
  <button class="scrim" tabindex="-1" aria-label={t("scripts.close")} onclick={onclose}></button>
  <div class="dialog" role="dialog" aria-modal="true" aria-label={mode === "save" ? t("scripts.titleSave") : t("scripts.titleLoad")}>
    <div class="head">
      <h2>{mode === "save" ? t("scripts.titleSave") : t("scripts.titleLoad")}</h2>
      <button class="close" onclick={onclose} title={t("scripts.close")} aria-label={t("scripts.close")}>&times;</button>
    </div>

    {#if status && status.state === "needs_password"}
      <div class="state">
        <p>{t("scripts.unlockHint")}</p>
        <label class="field">
          {t("scripts.unlockLabel")}
          <PasswordInput bind:value={unlockPassword} bind:this={unlockEl} disabled={busy} />
        </label>
        <div class="row">
          <button class="primary" disabled={busy || !unlockPassword} onclick={unlock}>{t("scripts.unlock")}</button>
        </div>
        {#if error}<p class="error" role="alert">{error}</p>{/if}
      </div>
    {:else if status && status.state !== "ready"}
      <div class="state">
        <p class="error" role="alert">
          {status.state === "failed" ? t("scripts.failed", { detail: status.detail ?? "?" }) : t("scripts.locked")}
        </p>
      </div>
    {:else}
      <input
        class="search"
        type="search"
        bind:this={searchEl}
        bind:value={query}
        onkeydown={onSearchKeydown}
        placeholder={t("scripts.search")}
        aria-label={t("scripts.searchLabel")}
        autocomplete="off"
        spellcheck="false"
      />

      <div class="cols">
        <div class="tree-panel">
          <div class="toolbar">
            <button class="ghost" disabled={busy} onclick={() => startEdit("newFolder")}>{t("scripts.newFolder")}</button>
            <button class="ghost" disabled={busy || !selected} onclick={() => startEdit("rename")}>{t("scripts.rename")}</button>
            <button class="ghost" disabled={busy || !selected} onclick={() => startEdit("move")}>{t("scripts.move")}</button>
            <button class="ghost" disabled={busy || !selected} onclick={askDelete}>{t("scripts.delete")}</button>
            <button class="ghost" disabled={busy || !selectedScript} aria-pressed={selectedScript?.favorite ?? false} onclick={toggleFavorite}>
              {selectedScript?.favorite ? `★ ${t("scripts.unfavorite")}` : `☆ ${t("scripts.favorite")}`}
            </button>
          </div>

          {#if edit}
            <div class="edit-row">
              {#if edit.kind === "move"}
                <select bind:this={editEl} bind:value={edit.value} aria-label={t("scripts.moveTo")}>
                  <option value="">{t("scripts.topLevel")}</option>
                  {#each moveOptions as option (option.id)}
                    <option value={option.id}>{"\u00a0\u00a0".repeat(option.depth)}{option.name}</option>
                  {/each}
                </select>
              {:else}
                <input
                  bind:this={editEl}
                  bind:value={edit.value}
                  placeholder={edit.kind === "newFolder" ? t("scripts.folderName") : t("scripts.newName")}
                  aria-label={edit.kind === "newFolder" ? t("scripts.folderName") : t("scripts.newName")}
                  autocomplete="off"
                  spellcheck="false"
                  onkeydown={(event) => { if (event.key === "Enter") { event.preventDefault(); applyEdit(); } }}
                />
              {/if}
              <button class="ghost" disabled={busy} onclick={applyEdit}>{t("scripts.ok")}</button>
              <button class="ghost" onclick={() => (edit = null)}>{t("scripts.cancel")}</button>
            </div>
          {/if}

          <div class="tree" role="tree" aria-label={t("scripts.tree")} tabindex="-1" onkeydown={onTreeKeydown}>
            {#each rows as row, index (row.node.id)}
              {@const isSelected = row.node.id === selectedId}
              <button
                class="row-item"
                class:selected={isSelected}
                role="treeitem"
                aria-level={row.depth + 1}
                aria-selected={isSelected}
                aria-expanded={row.node.type === "folder" ? row.open : undefined}
                tabindex={isSelected || (!selectedId && index === 0) ? 0 : -1}
                data-row-id={row.node.id}
                style:padding-left="{row.depth * 14 + 8}px"
                onclick={() => {
                  void select(row.node.id);
                  if (row.node.type === "folder" && !query.trim()) toggle(row.node.id);
                }}
                ondblclick={() => { if (row.node.type === "script" && mode === "load") requestLoad(); }}
              >
                {#if row.node.type === "folder"}
                  <span class="chevron" aria-hidden="true">{row.open ? "▾" : "▸"}</span>
                  <span class="folder-name">{row.node.name}</span>
                {:else}
                  <span class="chevron" aria-hidden="true">·</span>
                  <span>{row.node.name}</span>
                  {#if row.node.favorite}
                    <span class="star" role="img" aria-label={t("scripts.favoriteMark")} title={t("scripts.favoriteMark")}>★</span>
                  {/if}
                {/if}
              </button>
            {/each}
            {#if rows.length === 0}
              <p class="empty" role="status">
                {query.trim() ? t("scripts.noResults", { query: query.trim() }) : noScripts ? t("scripts.empty") : ""}
              </p>
            {/if}
          </div>
        </div>

        <div class="detail">
          {#if mode === "save"}
            <label class="field">
              {t("scripts.name")}
              <input
                type="text"
                bind:this={nameEl}
                bind:value={name}
                autocomplete="off"
                spellcheck="false"
                onkeydown={(event) => { if (event.key === "Enter") { event.preventDefault(); save(false); } }}
              />
            </label>
            <p class="meta">
              {t("scripts.folder")}:
              <strong>{targetFolder ? (findScriptNode(tree, targetFolder)?.name ?? "") : t("scripts.topLevel")}</strong>
            </p>
            <label class="check">
              <input type="checkbox" bind:checked={favoriteChecked} />
              {t("scripts.addFavorite")}
            </label>
            <p class="hint">{t("scripts.saveHint")}</p>
            <textarea readonly value={draft} aria-label={t("scripts.preview")} spellcheck="false"></textarea>
          {:else}
            <p class="meta">{t("scripts.preview")}: <strong>{preview?.name ?? ""}</strong></p>
            <textarea
              readonly
              value={preview?.body ?? ""}
              placeholder={t("scripts.previewEmpty")}
              aria-label={t("scripts.preview")}
              spellcheck="false"
            ></textarea>
          {/if}
        </div>
      </div>

      {#if error}<p class="error" role="alert">{error}</p>{/if}

      {#if confirm?.kind === "delete"}
        {@const target = findScriptNode(tree, confirm.id)}
        <div class="confirm" role="alert">
          <span>{target?.type === "folder" ? t("scripts.confirmDeleteFolder", { name: target.name }) : t("scripts.confirmDelete", { name: target?.name ?? "" })}</span>
          <button class="danger" disabled={busy} onclick={() => doDelete((confirm as { id: string }).id)}>{t("scripts.delete")}</button>
          <button class="ghost" onclick={() => (confirm = null)}>{t("scripts.cancel")}</button>
        </div>
      {:else if confirm?.kind === "overwrite"}
        {@const asNew = confirm.asNew}
        <div class="confirm" role="alert">
          <span>{t("scripts.overwrite", { name: name.trim() })}</span>
          <button class="danger" disabled={busy} onclick={() => { confirm = null; save(asNew, true); }}>{t("scripts.overwriteYes")}</button>
          <button class="ghost" onclick={() => (confirm = null)}>{t("scripts.cancel")}</button>
        </div>
      {:else if confirm?.kind === "draft"}
        <div class="confirm" role="alert">
          <span>{t("scripts.draftNotEmpty")}</span>
          <button class="primary" onclick={() => finishLoad("replace")}>{t("scripts.replace")}</button>
          <button class="ghost" onclick={() => finishLoad("append")}>{t("scripts.append")}</button>
          <button class="ghost" onclick={() => (confirm = null)}>{t("scripts.cancel")}</button>
        </div>
      {:else}
        <div class="actions">
          {#if mode === "load"}
            <button class="primary" disabled={!preview || busy} onclick={requestLoad}>{t("scripts.load")}</button>
          {:else if loaded}
            <button class="primary" disabled={busy || !name.trim() || draftEmpty} onclick={() => save(false)}>{t("scripts.update", { name: loaded.name })}</button>
            <button class="ghost" disabled={busy || !name.trim() || draftEmpty} onclick={() => save(true)}>{t("scripts.saveAsNew")}</button>
          {:else}
            <button class="primary" disabled={busy || !name.trim() || draftEmpty} onclick={() => save(false)}>{t("scripts.save")}</button>
          {/if}
          <button class="ghost" onclick={onclose}>{t("scripts.close")}</button>
        </div>
      {/if}
    {/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(8, 11, 16, 0.7);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 30;
  }
  .scrim {
    position: absolute;
    inset: 0;
    border: 0;
    padding: 0;
    background: transparent;
    cursor: default;
  }
  /* Fixed size, proportional to the window (see the UI guide): content never resizes it. */
  .dialog {
    position: relative;
    z-index: 1;
    width: clamp(560px, 64vw, 1000px);
    height: clamp(420px, 76vh, 840px);
    max-width: 96vw;
    max-height: 96vh;
    overflow: hidden;
    box-sizing: border-box;
    background: var(--surface-raised);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 1.1rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    color: var(--text);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex: 0 0 auto;
  }
  h2 {
    margin: 0;
    font-size: 1rem;
    font-weight: 500;
  }
  .search,
  .field input,
  .edit-row input,
  .edit-row select {
    font: inherit;
    font-size: 0.82rem;
    color: var(--text);
    background: var(--app-bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.4rem 0.55rem;
    min-width: 0;
  }
  .search {
    flex: 0 0 auto;
  }
  .cols {
    flex: 1 1 auto;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(200px, 38%) 1fr;
    gap: 0.8rem;
  }
  .tree-panel,
  .detail {
    min-height: 0;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
  }
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem;
    flex: 0 0 auto;
  }
  .edit-row {
    display: flex;
    gap: 0.3rem;
    flex: 0 0 auto;
  }
  .edit-row input,
  .edit-row select {
    flex: 1;
  }
  .tree {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    background: var(--surface-deep);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.25rem 0;
  }
  .row-item {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    width: 100%;
    font: inherit;
    font-size: 0.82rem;
    text-align: left;
    color: var(--text);
    background: transparent;
    border: 0;
    padding-top: 0.28rem;
    padding-bottom: 0.28rem;
    padding-right: 0.5rem;
    cursor: pointer;
    box-sizing: border-box;
  }
  .row-item:hover {
    background: var(--surface-hover);
  }
  .row-item.selected {
    background: color-mix(in srgb, var(--accent) 22%, transparent);
  }
  .chevron {
    width: 0.9rem;
    flex: 0 0 auto;
    color: var(--muted);
    text-align: center;
  }
  .star {
    color: var(--warning);
    font-size: 0.8rem;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.82rem;
    color: var(--text-secondary);
  }
  .check input {
    accent-color: var(--accent);
  }
  button[aria-pressed="true"] {
    color: var(--warning);
    border-color: var(--warning-border, var(--border));
  }
  .folder-name {
    font-weight: 600;
  }
  .detail textarea {
    flex: 1 1 auto;
    min-height: 0;
    resize: none;
    font-family: "JetBrains Mono", "Cascadia Mono", Consolas, monospace;
    font-size: 0.8rem;
    color: var(--text);
    background: var(--surface-deep);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.5rem;
    white-space: pre;
    overflow: auto;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    font-size: 0.82rem;
    color: var(--text-secondary);
  }
  .meta,
  .hint {
    margin: 0;
    font-size: 0.78rem;
    color: var(--text-secondary);
  }
  .hint {
    color: var(--muted);
  }
  .empty {
    margin: 0.8rem 0.6rem;
    font-size: 0.8rem;
    color: var(--muted);
    text-align: center;
  }
  .state {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    max-width: 24rem;
  }
  .state p {
    margin: 0;
  }
  .error {
    margin: 0;
    font-size: 0.78rem;
    color: var(--danger);
    flex: 0 0 auto;
  }
  .actions,
  .confirm,
  .row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    flex: 0 0 auto;
  }
  .confirm {
    padding: 0.45rem 0.6rem;
    background: var(--warning-bg, var(--surface));
    border: 1px solid var(--warning-border, var(--border));
    border-radius: 4px;
    font-size: 0.8rem;
    flex-wrap: wrap;
  }
  .confirm span {
    flex: 1 1 12rem;
  }
  button {
    font: inherit;
    font-size: 0.8rem;
    border-radius: 4px;
    padding: 0.35rem 0.7rem;
    cursor: pointer;
  }
  button.ghost {
    color: var(--text-secondary);
    background: transparent;
    border: 1px solid var(--border);
  }
  button.primary {
    color: var(--accent-contrast);
    background: var(--accent);
    border: 1px solid var(--accent);
    font-weight: 600;
  }
  button.danger {
    color: var(--danger);
    background: transparent;
    border: 1px solid var(--danger);
    font-weight: 600;
  }
  button.close {
    font-size: 1.15rem;
    line-height: 1;
    width: 1.65rem;
    height: 1.65rem;
    padding: 0;
    color: var(--text-secondary);
    background: transparent;
    border: 1px solid transparent;
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  button:focus-visible,
  input:focus-visible,
  select:focus-visible,
  textarea:focus-visible,
  .row-item:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
