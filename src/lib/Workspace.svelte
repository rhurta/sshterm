<script lang="ts">
  import { tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { hasMessage, t } from "$lib/i18n/index.svelte";
  import ConnectionSidebar from "./ConnectionSidebar.svelte";
  import ConnectionEditor from "./ConnectionEditor.svelte";
  import TerminalPane from "./TerminalPane.svelte";
  import LanguageSelect from "./LanguageSelect.svelte";
  import type { Connection, ConnectionSummary, Tab } from "$lib/types";

  let { dir, onlock }: { dir: string; onlock: () => void } = $props();

  let tabs = $state<Tab[]>([]);
  let activeId = $state<string | null>(null);
  let editor = $state<{ conn: Connection; isNew: boolean; parentId: string | null } | null>(null);
  let picker = $state(false);
  let pickerList = $state<ConnectionSummary[]>([]);
  let pickerDialog = $state<HTMLDivElement | null>(null);
  let newTabButton = $state<HTMLButtonElement | null>(null);
  let refreshToken = $state(0);
  let error = $state("");

  let openIds = $derived(tabs.map((tab) => tab.connId));

  /** The backend sends short codes; here they are converted to messages. */
  function errorText(code: string): string {
    const key = `error.${code}`;
    return hasMessage(key) ? t(key) : t("lock.error.unexpected", { detail: code });
  }

  function resolveJumpNames(connection: ConnectionSummary, all: ConnectionSummary[]): string[] {
    const byId = new Map(all.map((item) => [item.id, item]));
    const active = new Set<string>();
    const visited = new Set<string>();
    const names: string[] = [];

    function append(id: string) {
      if (active.has(id) || visited.has(id)) return;
      const hop = byId.get(id);
      if (!hop) return;
      active.add(id);
      for (const nested of hop.jump ?? []) append(nested);
      active.delete(id);
      visited.add(id);
      names.push(hop.name);
    }

    for (const id of connection.jump ?? []) append(id);
    return names;
  }

  function blank(): Connection {
    return {
      id: crypto.randomUUID(),
      name: "",
      host: "",
      port: 22,
      user: "",
      auth: { type: "agent" },
      keepalive_secs: 30,
      colors: null,
      protected: false,
      jump: [],
      tunnels: [],
      sftp: null,
      highlight: [],
    };
  }

  async function edit(c: ConnectionSummary) {
    try {
      const full = await invoke<Connection | null>("conn_get", { id: c.id });
      if (full) editor = { conn: full, isNew: false, parentId: null };
    } catch (e) {
      error = errorText(String(e));
    }
  }

  async function save(c: Connection, parentId: string | null): Promise<string | null> {
    try {
      await invoke("conn_upsert", { conn: c, parentId });
      editor = null;
      error = "";
      refreshToken += 1;
      return null;
    } catch (e) {
      return String(e);
    }
  }

  async function connect(c: ConnectionSummary) {
    error = "";
    try {
      const jumpNames = c.jump?.length
        ? resolveJumpNames(c, await invoke<ConnectionSummary[]>("conn_list"))
        : [];
      // 80x24 is provisional: TerminalPane adjusts to the real size on open.
      const sessionId = await invoke<string>("session_open", {
        connId: c.id,
        cols: 80,
        rows: 24,
      });
      tabs = [...tabs, { sessionId, connId: c.id, name: c.name, jumpNames }];
      // The new pane mounts active; TerminalPane focuses xterm as soon as it
      // is visible, so typing starts in the terminal right away.
      activeId = sessionId;
      picker = false;
    } catch (e) {
      error = errorText(String(e));
    }
  }

  async function openPicker() {
    try {
      pickerList = await invoke<ConnectionSummary[]>("conn_list");
      picker = true;
      // Move keyboard focus out of xterm, otherwise Escape would be sent to
      // the remote host instead of dismissing the dialog.
      await tick();
      pickerDialog?.focus();
    } catch (e) {
      error = errorText(String(e));
    }
  }

  function closePicker() {
    picker = false;
    newTabButton?.focus();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && picker) {
      e.preventDefault();
      closePicker();
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

<svelte:window onkeydown={onKeydown} />

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

  <div class="body">
    <ConnectionSidebar
      {openIds}
      {refreshToken}
      onopen={connect}
      onedit={edit}
      onnewconn={(parentId) => (editor = { conn: blank(), isNew: true, parentId })}
    />

    <main>
      {#if tabs.length > 0}
        <div class="tabs">
          {#each tabs as tab (tab.sessionId)}
            <div class="tab" class:active={activeId === tab.sessionId}>
              <button
                class="tab-name"
                onclick={() => (activeId = tab.sessionId)}
                title={tab.jumpNames.length ? `${tab.jumpNames.join(" → ")} → ${tab.name}` : tab.name}
              >
                <span>{tab.name}</span>
                {#if tab.jumpNames.length > 0}
                  <small class="tab-chain">{tab.jumpNames.join(" → ")} → {tab.name}</small>
                {/if}
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
          <button
            class="new-tab"
            bind:this={newTabButton}
            onclick={openPicker}
            title={t("workspace.newTab")}
          >+</button>
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

  {#if picker}
    <div class="backdrop">
      <button
        class="scrim"
        tabindex="-1"
        aria-label={t("workspace.close")}
        onclick={closePicker}
      ></button>
      <div
        class="dialog"
        role="dialog"
        aria-modal="true"
        aria-label={t("workspace.picker")}
        tabindex="-1"
        bind:this={pickerDialog}
      >
        <div class="dialog-head">
          <h2>{t("workspace.picker")}</h2>
          <button
            class="close"
            onclick={closePicker}
            title={t("workspace.close")}
            aria-label={t("workspace.close")}
          >&times;</button>
        </div>
        {#if pickerList.length === 0}
          <p class="hint">{t("workspace.pickerEmpty")}</p>
        {:else}
          <div class="picker-list">
            {#each pickerList as c (c.id)}
              <button onclick={() => connect(c)}>
                <span class="name">{c.name}</span>
                <span class="detail">{c.user}@{c.host}:{c.port}</span>
              </button>
            {/each}
          </div>
        {/if}
        <button class="ghost" onclick={closePicker}>{t("editor.cancel")}</button>
      </div>
    </div>
  {/if}

  {#if editor}
    <ConnectionEditor
      conn={editor.conn}
      isNew={editor.isNew}
      onsave={(c) => save(c, editor!.parentId)}
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
  .tab .tab-name {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.12rem;
    min-width: 0;
  }
  .tab-chain {
    max-width: 24rem;
    overflow: hidden;
    text-overflow: ellipsis;
    color: #8d97a8;
    font-size: 0.62rem;
    font-weight: 400;
    white-space: nowrap;
  }
  .tab-close {
    color: #8d97a8 !important;
  }
  .new-tab {
    font: inherit;
    font-size: 0.9rem;
    color: #b6bfcd;
    background: transparent;
    border: 1px solid #2f3847;
    border-radius: 4px;
    padding: 0.2rem 0.6rem;
    cursor: pointer;
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
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(8, 11, 16, 0.7);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 25;
  }
  .scrim {
    position: absolute;
    inset: 0;
    border: 0;
    padding: 0;
    background: transparent;
    cursor: default;
  }
  .dialog {
    position: relative;
    z-index: 1;
    width: min(24rem, 92vw);
    max-height: 80vh;
    /* Only the list scrolls, so the close button stays reachable. */
    overflow: hidden;
    background: #1f2530;
    border: 1px solid #2f3847;
    border-radius: 8px;
    padding: 1.1rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    box-sizing: border-box;
  }
  .dialog:focus {
    outline: none;
  }
  .dialog-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }
  .dialog h2 {
    margin: 0;
    font-size: 1rem;
    font-weight: 500;
  }
  button.close {
    font: inherit;
    font-size: 1.15rem;
    line-height: 1;
    width: 1.65rem;
    height: 1.65rem;
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #b6bfcd;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 4px;
    cursor: pointer;
  }
  button.close:hover {
    color: #dfe4ec;
    background: #222a36;
    border-color: #2f3847;
  }
  .picker-list {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    max-height: 55vh;
    overflow-y: auto;
    padding-right: 0.3rem;
  }
  .picker-list button {
    font: inherit;
    font-size: 0.82rem;
    text-align: left;
    color: #dfe4ec;
    background: #222a36;
    border: 1px solid #2f3847;
    border-radius: 4px;
    padding: 0.45rem 0.6rem;
    cursor: pointer;
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }
  .picker-list button:hover {
    border-color: #8fb4e8;
  }
  .detail {
    font-size: 0.72rem;
    color: #8d97a8;
  }
  .dialog button.ghost {
    font: inherit;
    font-size: 0.8rem;
    align-self: flex-start;
    color: #b6bfcd;
    background: transparent;
    border: 1px solid #2f3847;
    border-radius: 4px;
    padding: 0.35rem 0.7rem;
    cursor: pointer;
  }
</style>
