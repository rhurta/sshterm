<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { hasMessage, t } from "$lib/i18n/index.svelte";
  import ConnectionSidebar from "./ConnectionSidebar.svelte";
  import ConnectionEditor from "./ConnectionEditor.svelte";
  import TerminalPane from "./TerminalPane.svelte";
  import LanguageSelect from "./LanguageSelect.svelte";
  import type { Connection, ConnectionSummary, Tab } from "$lib/types";

  let { dir, onlock }: { dir: string; onlock: () => void } = $props();

  let connections = $state<ConnectionSummary[]>([]);
  let tabs = $state<Tab[]>([]);
  let activeId = $state<string | null>(null);
  let editor = $state<{ conn: Connection; isNew: boolean } | null>(null);
  let pendingDelete = $state<ConnectionSummary | null>(null);
  let error = $state("");

  /** The backend sends short codes; here they are converted to messages. */
  function errorText(code: string): string {
    const key = `error.${code}`;
    return hasMessage(key) ? t(key) : t("lock.error.unexpected", { detail: code });
  }

  async function refresh() {
    try {
      connections = await invoke<ConnectionSummary[]>("conn_list");
    } catch (e) {
      error = errorText(String(e));
    }
  }
  void refresh();

  function blank(): Connection {
    return {
      id: crypto.randomUUID(),
      name: "",
      host: "",
      port: 22,
      user: "",
      password: "",
      keepalive_secs: 30,
    };
  }

  async function edit(c: ConnectionSummary) {
    try {
      const full = await invoke<Connection | null>("conn_get", { id: c.id });
      if (full) editor = { conn: { ...full, password: full.password ?? "" }, isNew: false };
    } catch (e) {
      error = errorText(String(e));
    }
  }

  async function save(c: Connection) {
    try {
      await invoke("conn_upsert", { conn: c });
      editor = null;
      error = "";
      await refresh();
    } catch (e) {
      error = errorText(String(e));
    }
  }

  // Native dialogs (confirm) are blocked in WebView2; an inline confirmation
  // is used instead.
  function remove(c: ConnectionSummary) {
    pendingDelete = c;
  }

  async function confirmDelete() {
    const c = pendingDelete;
    pendingDelete = null;
    if (!c) return;
    try {
      await invoke("conn_delete", { id: c.id });
      await refresh();
    } catch (e) {
      error = errorText(String(e));
    }
  }

  async function connect(c: ConnectionSummary) {
    error = "";
    try {
      // 80x24 is provisional: TerminalPane adjusts to the real size on open.
      const sessionId = await invoke<string>("session_open", {
        connId: c.id,
        cols: 80,
        rows: 24,
      });
      tabs = [...tabs, { sessionId, name: c.name }];
      activeId = sessionId;
    } catch (e) {
      error = errorText(String(e));
    }
  }

  function dropTab(sessionId: string) {
    tabs = tabs.filter((tab) => tab.sessionId !== sessionId);
    if (activeId === sessionId) activeId = tabs[tabs.length - 1]?.sessionId ?? null;
  }

  async function closeTab(sessionId: string) {
    try {
      await invoke("session_close", { id: sessionId });
    } catch {
      // the session already ended; drop the tab anyway
    }
    dropTab(sessionId);
  }
</script>

<div class="app">
  <header>
    <span class="brand">sshterm</span>
    <span class="dir">{dir}</span>
    <div class="header-right">
      <LanguageSelect />
      <button onclick={onlock}>{t("home.lock")}</button>
    </div>
  </header>

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}

  {#if pendingDelete}
    <div class="confirm">
      <span>{t("workspace.confirmDelete", { name: pendingDelete.name })}</span>
      <button onclick={confirmDelete}>{t("workspace.delete")}</button>
      <button class="ghost" onclick={() => (pendingDelete = null)}>
        {t("editor.cancel")}
      </button>
    </div>
  {/if}

  <div class="body">
    <ConnectionSidebar
      {connections}
      onnew={() => (editor = { conn: blank(), isNew: true })}
      onopen={connect}
      onedit={edit}
      ondelete={remove}
    />

    <main>
      {#if tabs.length > 0}
        <div class="tabs">
          {#each tabs as tab (tab.sessionId)}
            <div class="tab" class:active={activeId === tab.sessionId}>
              <button class="tab-name" onclick={() => (activeId = tab.sessionId)}>
                {tab.name}
              </button>
              <button
                class="tab-close"
                onclick={() => closeTab(tab.sessionId)}
                title={t("workspace.close")}
              >
                x
              </button>
            </div>
          {/each}
        </div>
        <div class="panes">
          {#each tabs as tab (tab.sessionId)}
            <TerminalPane
              sessionId={tab.sessionId}
              active={activeId === tab.sessionId}
              onclosed={dropTab}
            />
          {/each}
        </div>
      {:else}
        <div class="empty">
          <p class="hint">{t("workspace.terminals")}</p>
        </div>
      {/if}
    </main>
  </div>

  {#if editor}
    <ConnectionEditor
      conn={editor.conn}
      isNew={editor.isNew}
      onsave={save}
      oncancel={() => (editor = null)}
    />
  {/if}
</div>

<style>
  .app {
    height: 100vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  header {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 0.5rem 0.9rem;
    border-bottom: 1px solid #2f3847;
    background: #1a1f29;
  }
  .brand {
    font-weight: 600;
    color: #8fb4e8;
    white-space: nowrap;
  }
  .dir {
    flex: 1;
    font-size: 0.75rem;
    color: #8d97a8;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .header-right {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    flex: 0 0 auto;
  }
  header button {
    font: inherit;
    font-size: 0.8rem;
    color: #dfe4ec;
    background: #222a36;
    border: 1px solid #2f3847;
    border-radius: 4px;
    padding: 0.3rem 0.7rem;
    cursor: pointer;
    white-space: nowrap;
  }
  .error {
    margin: 0;
    padding: 0.5rem 0.9rem;
    background: #3a2226;
    color: #f08a7a;
    font-size: 0.85rem;
  }
  .confirm {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    padding: 0.45rem 0.9rem;
    background: #2a2630;
    font-size: 0.85rem;
  }
  .confirm span {
    flex: 1;
  }
  .confirm button {
    font: inherit;
    font-size: 0.78rem;
    color: #12161d;
    background: #f08a7a;
    border: 0;
    border-radius: 4px;
    padding: 0.3rem 0.65rem;
    cursor: pointer;
  }
  .confirm button.ghost {
    background: transparent;
    color: #b6bfcd;
    border: 1px solid #2f3847;
  }
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    background: #161a22;
  }
  .tabs {
    flex: 0 0 auto;
    display: flex;
    gap: 0.25rem;
    padding: 0.4rem;
    border-bottom: 1px solid #2f3847;
    background: #1a1f29;
  }
  .tab {
    display: flex;
    align-items: center;
    border: 1px solid #2f3847;
    border-radius: 4px;
    overflow: hidden;
  }
  .tab.active {
    border-color: #8fb4e8;
  }
  .tab button {
    font: inherit;
    font-size: 0.8rem;
    color: #dfe4ec;
    background: transparent;
    border: 0;
    padding: 0.3rem 0.55rem;
    cursor: pointer;
  }
  .tab-close {
    color: #8d97a8 !important;
  }
  .panes {
    flex: 1;
    min-height: 0;
  }
  .empty {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .hint {
    margin: 0;
    color: #8d97a8;
    font-size: 0.9rem;
  }
</style>
