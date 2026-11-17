<script lang="ts">
  import { onMount, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { subscribeSessionErrors } from "$lib/sessionBus";
  import { hasMessage, t } from "$lib/i18n/index.svelte";
  import ConnectionSidebar from "./ConnectionSidebar.svelte";
  import ConnectionEditor from "./ConnectionEditor.svelte";
  import TerminalPane from "./TerminalPane.svelte";
  import LanguageSelect from "./LanguageSelect.svelte";
  import type { AppTheme, Connection, ConnectionSummary, Tab, TerminalSettings } from "$lib/types";
  import { DEFAULT_TERMINAL_SETTINGS } from "$lib/types";
  import { addSessionToPane, createLayoutTree, getLayoutGeometry, removeSession, setSplitRatioAtPath, splitPaneWithEmptyLeaf, type LayoutPreset, type LayoutRect, type SplitAxis, type SplitNode, type SplitPath } from "$lib/splitTree";

  let {
    dir,
    theme,
    settingsOpen = $bindable(false),
  }: {
    dir: string;
    theme: AppTheme;
    settingsOpen?: boolean;
  } = $props();

  let tabs = $state<Tab[]>([]);
  let activeId = $state<string | null>(null);
  let layoutPreset = $state<LayoutPreset>("single");
  let layoutTree = $state<SplitNode | null>(null);
  let layoutGeometry = $derived(getLayoutGeometry(layoutTree));
  let paneActiveIds = $state<Record<string, string>>({});
  let pendingPaneId = $state<string | null>(null);
  let visibleSessionIds = $derived(layoutGeometry.leaves.map((item) => item.sessionId));
  let panesElement = $state<HTMLDivElement | null>(null);
  let dragState = $state<{ path: SplitPath; axis: SplitAxis; region: LayoutRect } | null>(null);
  let editor = $state<{ conn: Connection; isNew: boolean; parentId: string | null } | null>(null);
  let picker = $state(false);
  let pickerList = $state<ConnectionSummary[]>([]);
  let pickerDialog = $state<HTMLDivElement | null>(null);
  let newTabButton = $state<HTMLButtonElement | null>(null);
  let refreshToken = $state(0);
  let error = $state("");
  let settings = $state<TerminalSettings>({ ...DEFAULT_TERMINAL_SETTINGS });
  let multilinePaste = $state<{ lineCount: number } | null>(null);
  let multilinePasteCancelButton = $state<HTMLButtonElement | null>(null);
  let multilinePasteResolver: ((confirmed: boolean) => void) | null = null;

  let openIds = $derived(tabs.map((tab) => tab.connId));

  /** The backend sends short codes; here they are converted to messages. */
  function errorText(value: string): string {
    const [code, ...detailParts] = value.split(":");
    const key = `error.${code.trim()}`;
    const base = hasMessage(key)
      ? t(key)
      : t("lock.error.unexpected", { detail: code.trim() });
    const detail = detailParts.join(":").trim();
    return detail ? `${base} — ${detail}` : base;
  }

  onMount(() => {
    void invoke<TerminalSettings>("config_get_terminal_settings")
      .then((value) => (settings = value))
      .catch(() => {});
    return subscribeSessionErrors((sessionId, message) => {
      const tab = tabs.find((item) => item.sessionId === sessionId);
      error = tab ? `${tab.name}: ${message}` : message;
    });
  });

  async function setTerminalSetting<K extends keyof TerminalSettings>(
    key: K,
    value: TerminalSettings[K],
  ) {
    const previous = settings;
    const next = { ...settings, [key]: value };
    settings = next;
    try {
      await invoke("config_set_terminal_settings", { settings: next });
    } catch (e) {
      settings = previous;
      error = String(e);
    }
  }

  /** Requests confirmation before a terminal receives multi-line clipboard data. */
  function requestMultilinePaste(text: string): Promise<boolean> {
    // Do not stack confirmation dialogs if another paste is triggered while
    // the first one is awaiting a decision.
    if (multilinePasteResolver) return Promise.resolve(false);
    return new Promise((resolve) => {
      multilinePasteResolver = resolve;
      multilinePaste = { lineCount: text.split(/\r\n|\r|\n/).length };
      void tick().then(() => multilinePasteCancelButton?.focus());
    });
  }

  function settleMultilinePaste(confirmed: boolean) {
    const resolve = multilinePasteResolver;
    multilinePasteResolver = null;
    multilinePaste = null;
    resolve?.(confirmed);
  }

  function resolveJumpNames(connection: ConnectionSummary, all: ConnectionSummary[]): string[] {
    const byId = new Map(all.map((item) => [item.id, item]));
    const active = new Set<string>();
    const visited = new Set<string>();
    const names: string[] = [];

    function append(id: string, disabled: string[] = []) {
      if (disabled.includes(id) || active.has(id) || visited.has(id)) return;
      const hop = byId.get(id);
      if (!hop) return;
      active.add(id);
      for (const nested of hop.jump ?? []) append(nested, hop.jump_disabled ?? []);
      active.delete(id);
      visited.add(id);
      names.push(hop.name);
    }

    for (const id of connection.jump ?? []) {
      append(id, connection.jump_disabled ?? []);
    }
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
      jump_disabled: [],
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
      tabs = [...tabs, { sessionId, connId: c.id, name: c.name, jumpNames, tunnelCount: c.tunnel_count ?? 0 }];
      const focusedPane = layoutGeometry.leaves.find((pane) => pane.sessionIds.includes(activeId ?? ""));
      const targetPaneId = pendingPaneId ?? focusedPane?.sessionId ?? layoutGeometry.leaves[0]?.sessionId;
      if (!layoutTree || !targetPaneId) {
        layoutTree = createLayoutTree([sessionId], "single", sessionId);
        paneActiveIds = { ...paneActiveIds, [sessionId]: sessionId };
      } else {
        layoutTree = addSessionToPane(layoutTree, targetPaneId, sessionId);
        paneActiveIds = { ...paneActiveIds, [targetPaneId]: sessionId };
      }
      pendingPaneId = null;
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


  async function openPickerForPane(paneId: string) {
    pendingPaneId = paneId;
    await openPicker();
  }

  function splitFocusedPane(axis: SplitAxis) {
    if (!layoutTree || !activeId) return;
    const pane = layoutGeometry.leaves.find((item) => item.sessionIds.includes(activeId!));
    if (!pane) return;
    const paneId = crypto.randomUUID();
    layoutTree = splitPaneWithEmptyLeaf(layoutTree, activeId, paneId, axis);
    pendingPaneId = paneId;
  }

  function closePicker() {
    picker = false;
    pendingPaneId = null;
    newTabButton?.focus();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && multilinePaste) {
      e.preventDefault();
      settleMultilinePaste(false);
    } else if (e.key === "Escape" && settingsOpen) {
      e.preventDefault();
      settingsOpen = false;
    } else if (e.key === "Escape" && picker) {
      e.preventDefault();
      closePicker();
    }
  }

  function activateSession(sessionId: string) {
    activeId = sessionId;
    const pane = layoutGeometry.leaves.find((item) => item.sessionIds.includes(sessionId));
    if (pane) paneActiveIds = { ...paneActiveIds, [pane.sessionId]: sessionId };
  }

  function chooseLayout(preset: LayoutPreset) {
    layoutPreset = preset;
    pendingPaneId = null;
    layoutTree = createLayoutTree(tabs.map((tab) => tab.sessionId), preset, activeId);
    const nextActive: Record<string, string> = {};
    for (const pane of getLayoutGeometry(layoutTree).leaves) {
      nextActive[pane.sessionId] = pane.sessionIds.includes(activeId ?? "") ? activeId! : pane.sessionIds[0];
    }
    paneActiveIds = nextActive;
  }

  function dropTab(sessionId: string) {
    const oldPane = layoutGeometry.leaves.find((pane) => pane.sessionIds.includes(sessionId));
    tabs = tabs.filter((tab) => tab.sessionId !== sessionId);
    layoutTree = removeSession(layoutTree, sessionId, true);
    if (oldPane && paneActiveIds[oldPane.sessionId] === sessionId) {
      const remaining = oldPane.sessionIds.filter((id) => id !== sessionId);
      if (remaining.length) paneActiveIds = { ...paneActiveIds, [oldPane.sessionId]: remaining[0] };
      else {
        const { [oldPane.sessionId]: _removed, ...rest } = paneActiveIds;
        paneActiveIds = rest;
      }
    }
    if (activeId === sessionId) activeId = tabs[tabs.length - 1]?.sessionId ?? null;
  }

  function dividerStyle(axis: SplitAxis, rect: LayoutRect): string {
    if (axis === "horizontal") {
      return `left: calc(${rect.x * 100}% - 4px); top: ${rect.y * 100}%; width: 8px; height: ${rect.height * 100}%;`;
    }
    return `left: ${rect.x * 100}%; top: calc(${rect.y * 100}% - 4px); width: ${rect.width * 100}%; height: 8px;`;
  }

  function updateDivider(event: PointerEvent, path: SplitPath, axis: SplitAxis, region: LayoutRect) {
    if (!panesElement) return;
    const bounds = panesElement.getBoundingClientRect();
    const ratio = axis === "horizontal"
      ? (event.clientX - bounds.left - region.x * bounds.width) / (region.width * bounds.width)
      : (event.clientY - bounds.top - region.y * bounds.height) / (region.height * bounds.height);
    layoutTree = setSplitRatioAtPath(layoutTree, path, ratio);
  }

  function startDividerDrag(event: PointerEvent, divider: { path: SplitPath; axis: SplitAxis; region: LayoutRect }) {
    event.preventDefault();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    dragState = { path: divider.path, axis: divider.axis, region: divider.region };
  }

  function moveDividerDrag(event: PointerEvent, divider: { path: SplitPath; axis: SplitAxis; region: LayoutRect }) {
    if (!dragState || dragState.path.join("/") !== divider.path.join("/")) return;
    updateDivider(event, divider.path, divider.axis, divider.region);
  }

  function endDividerDrag() {
    dragState = null;
  }

  function adjustDivider(event: KeyboardEvent, divider: { path: SplitPath; axis: SplitAxis; ratio: number }) {
    const negative = divider.axis === "horizontal" ? ["ArrowLeft"] : ["ArrowUp"];
    const positive = divider.axis === "horizontal" ? ["ArrowRight"] : ["ArrowDown"];
    let next = divider.ratio;
    if (negative.includes(event.key)) next -= 0.02;
    else if (positive.includes(event.key)) next += 0.02;
    else return;
    event.preventDefault();
    layoutTree = setSplitRatioAtPath(layoutTree, divider.path, next);
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
        <div class="workspace-layout">
          <div class="layout-tools" aria-label="Terminal layout">
            <button bind:this={newTabButton} class="tool-add" title={t("workspace.newTab")} onclick={openPicker}>+</button>
            <button class:chosen={layoutPreset === "single"} title="Single pane" aria-pressed={layoutPreset === "single"} onclick={() => chooseLayout("single")}>1</button>
            <button class:chosen={layoutPreset === "left-right"} title="Left and right" aria-pressed={layoutPreset === "left-right"} onclick={() => chooseLayout("left-right")}>▥</button>
            <button class:chosen={layoutPreset === "top-bottom"} title="Top and bottom" aria-pressed={layoutPreset === "top-bottom"} onclick={() => chooseLayout("top-bottom")}>▤</button>
            <button class:chosen={layoutPreset === "grid"} title="2 by 2 grid" aria-pressed={layoutPreset === "grid"} onclick={() => chooseLayout("grid")}>▦</button>
            <span class="tool-separator"></span>
            <button title="Split focused pane left/right" disabled={!activeId} onclick={() => splitFocusedPane("horizontal")}>↔</button>
            <button title="Split focused pane top/bottom" disabled={!activeId} onclick={() => splitFocusedPane("vertical")}>↕</button>
          </div>
          <div class="panes" bind:this={panesElement}>
            {#each layoutGeometry.leaves as pane (pane.sessionId)}
              <div class="pane-slot" style={`left: ${pane.rect.x * 100}%; top: ${pane.rect.y * 100}%; width: ${pane.rect.width * 100}%; height: ${pane.rect.height * 100}%;`}>
                <div class="pane-tabs" role="tablist" aria-label="Sessions in pane">
                  {#each pane.sessionIds as sessionId (sessionId)}
                    {@const tab = tabs.find((item) => item.sessionId === sessionId)}
                    {#if tab}
                      <div class="pane-tab" class:active={paneActiveIds[pane.sessionId] === sessionId}>
                        <button role="tab" aria-selected={paneActiveIds[pane.sessionId] === sessionId} title={tab.jumpNames.length ? `${tab.jumpNames.join(" → ")} → ${tab.name}` : tab.name} onclick={() => activateSession(sessionId)}>{tab.name}</button>
                        <button class="pane-tab-close" aria-label={`Close ${tab.name}`} title={t("workspace.close")} onclick={() => closeTab(sessionId)}>×</button>
                      </div>
                    {/if}
                  {/each}
                  <button class="pane-add" title="Open a connection in this pane" aria-label="Open a connection in this pane" onclick={() => openPickerForPane(pane.sessionId)}>+</button>
                </div>
                <div class="pane-terminals">
                  {#if pane.sessionIds.length === 0}
                    <div class="empty-pane"><button onclick={() => openPickerForPane(pane.sessionId)}>Open a connection</button></div>
                  {/if}
                  {#each pane.sessionIds as sessionId (sessionId)}
                    <TerminalPane sessionId={sessionId} active={paneActiveIds[pane.sessionId] === sessionId}
                      focused={activeId === sessionId} onactivate={activateSession} onclosed={dropTab}
                      onmultilinepaste={requestMultilinePaste} {settings} {theme} />
                  {/each}
                </div>
              </div>
            {/each}
            {#each layoutGeometry.dividers as divider}
              <div class="layout-divider" class:vertical={divider.axis === "horizontal"} class:horizontal={divider.axis === "vertical"}
                style={dividerStyle(divider.axis, divider.rect)} role="separator"
                aria-orientation={divider.axis === "horizontal" ? "vertical" : "horizontal"}
                aria-label="Resize terminal panes" aria-valuemin="15" aria-valuemax="85"
                aria-valuenow={Math.round(divider.ratio * 100)} tabindex="0"
                onpointerdown={(event) => startDividerDrag(event, divider)}
                onpointermove={(event) => moveDividerDrag(event, divider)}
                onpointerup={endDividerDrag} onpointercancel={endDividerDrag}
                onkeydown={(event) => adjustDivider(event, divider)}
              ></div>
            {/each}
          </div>
        </div>
      {:else}
        <div class="empty">
          <p class="hint">{t("workspace.terminals")}</p>
        </div>
      {/if}
    </main>
  </div>

  {#if settingsOpen}
    <div class="backdrop">
      <button class="scrim" tabindex="-1" aria-label={t("workspace.close")} onclick={() => (settingsOpen = false)}></button>
      <div class="dialog settings-dialog" role="dialog" aria-modal="true" aria-label={t("settings.title")} tabindex="-1">
        <div class="dialog-head">
          <h2>{t("settings.title")}</h2>
          <button class="close" onclick={() => (settingsOpen = false)} title={t("workspace.close")}>&times;</button>
        </div>
        <h3>{t("settings.language")}</h3>
        <LanguageSelect />
        <h3>{t("settings.terminal")}</h3>
        <label class="setting"><input type="checkbox" checked={settings.copy_on_selection} onchange={(e) => setTerminalSetting("copy_on_selection", e.currentTarget.checked)} />{t("settings.copyOnSelection")}</label>
        <label class="setting"><input type="checkbox" checked={settings.right_click_copy} onchange={(e) => setTerminalSetting("right_click_copy", e.currentTarget.checked)} />{t("settings.rightClickCopy")}</label>
        <label class="setting"><input type="checkbox" checked={settings.middle_click_copy} onchange={(e) => setTerminalSetting("middle_click_copy", e.currentTarget.checked)} />{t("settings.middleClickCopy")}</label>
        <label class="setting"><input type="checkbox" checked={settings.right_click_paste} onchange={(e) => setTerminalSetting("right_click_paste", e.currentTarget.checked)} />{t("settings.rightClickPaste")}</label>
        <label class="setting"><input type="checkbox" checked={settings.middle_click_paste} onchange={(e) => setTerminalSetting("middle_click_paste", e.currentTarget.checked)} />{t("settings.middleClickPaste")}</label>
        <label class="setting"><input type="checkbox" checked={settings.ctrl_c_copy} onchange={(e) => setTerminalSetting("ctrl_c_copy", e.currentTarget.checked)} />{t("settings.ctrlCCopy")}</label>
        <label class="setting"><input type="checkbox" checked={settings.confirm_multiline_paste} onchange={(e) => setTerminalSetting("confirm_multiline_paste", e.currentTarget.checked)} />{t("settings.confirmMultilinePaste")}</label>
        <button class="ghost" onclick={() => (settingsOpen = false)}>{t("editor.cancel")}</button>
      </div>
    </div>
  {/if}

  {#if multilinePaste}
    <div class="backdrop">
      <button
        class="scrim"
        tabindex="-1"
        aria-label={t("settings.multilinePasteCancel")}
        onclick={() => settleMultilinePaste(false)}
      ></button>
      <div
        class="dialog confirm-paste-dialog"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="multiline-paste-title"
        aria-describedby="multiline-paste-warning"
        tabindex="-1"
      >
        <div class="dialog-head">
          <h2 id="multiline-paste-title">{t("settings.multilinePasteTitle")}</h2>
        </div>
        <p id="multiline-paste-warning" class="hint">
          {t("settings.multilinePasteWarning", { count: multilinePaste.lineCount })}
        </p>
        <div class="confirm-actions">
          <button bind:this={multilinePasteCancelButton} class="ghost" onclick={() => settleMultilinePaste(false)}>
            {t("settings.multilinePasteCancel")}
          </button>
          <button class="confirm" onclick={() => settleMultilinePaste(true)}>
            {t("settings.multilinePasteConfirm")}
          </button>
        </div>
      </div>
    </div>
  {/if}

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
    height: 100%;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .error {
    margin: 0;
    padding: 0.5rem 0.9rem;
    background: var(--danger-bg);
    color: var(--danger);
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
    background: var(--app-bg);
  }
  .tabs {
    flex: 0 0 auto;
    display: flex;
    gap: 0.25rem;
    padding: 0.4rem;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }
  .tab {
    display: flex;
    align-items: center;
    border: 1px solid var(--border);
    border-radius: 4px;
    overflow: hidden;
  }
  .tab.active {
    border-color: var(--accent);
  }
  .tab button {
    font: inherit;
    font-size: 0.8rem;
    color: var(--text);
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
    color: var(--muted);
    font-size: 0.62rem;
    font-weight: 400;
    white-space: nowrap;
  }
  .tab-forwards {
    max-width: 24rem;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--success);
    font-size: 0.62rem;
    font-weight: 400;
    white-space: nowrap;
  }
  .tab-close {
    color: var(--muted) !important;
  }
  .new-tab {
    font: inherit;
    font-size: 0.9rem;
    color: var(--text-secondary);
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.2rem 0.6rem;
    cursor: pointer;
  }
  .workspace-layout { flex: 1; min-height: 0; min-width: 0; display: flex; overflow: hidden; }
  .panes { position: relative; flex: 1; min-height: 0; min-width: 0; overflow: hidden; background: var(--app-bg); }
  .pane-slot { position: absolute; display: flex; flex-direction: column; min-width: 0; min-height: 0; overflow: hidden; background: var(--surface); }
  .pane-tabs { display: flex; flex: 0 0 auto; min-height: 1.7rem; overflow-x: auto; gap: 2px; padding: 2px 3px; background: var(--surface); border-bottom: 1px solid var(--border); }
  .pane-tab { display: flex; align-items: center; min-width: 0; max-width: 11rem; border: 1px solid var(--border); border-radius: 3px; background: var(--surface-hover); }
  .pane-tab.active { border-color: var(--accent); background: var(--surface-raised); }
  .pane-tab button { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; padding: 0.18rem 0.35rem; color: var(--text-secondary); background: transparent; border: 0; font: inherit; font-size: 0.68rem; cursor: pointer; }
  .pane-tab.active button { color: var(--text); }
  .pane-tab .pane-tab-close { flex: 0 0 auto; padding-inline: 0.28rem; }
  .pane-add { flex: 0 0 auto; align-self: center; border: 0; background: transparent; color: var(--accent); font: inherit; cursor: pointer; }
  .empty-pane { display: flex; width: 100%; height: 100%; align-items: center; justify-content: center; }
  .empty-pane button { font: inherit; font-size: 0.78rem; color: var(--text-secondary); background: var(--surface-hover); border: 1px solid var(--border); border-radius: 4px; padding: 0.4rem 0.7rem; cursor: pointer; }
  .tool-separator { height: 1px; margin: 0.2rem 0; background: var(--border); }
  .pane-terminals { position: relative; flex: 1; min-height: 0; overflow: hidden; }
  .pane-terminals :global(.pane) { position: absolute; inset: 0; }

  .layout-divider {
    position: absolute;
    z-index: 4;
    background: var(--border);
    touch-action: none;
    outline: none;
  }
  .layout-divider.vertical { cursor: col-resize; }
  .layout-divider.horizontal { cursor: row-resize; }
  .layout-divider:hover,
  .layout-divider:focus-visible {
    background: var(--accent);
  }
  .layout-tools { display: flex; flex-direction: column; flex: 0 0 2.4rem; gap: 0.3rem; padding: 0.35rem 0.25rem; background: var(--surface); border-right: 1px solid var(--border); }
  .layout-tools button { min-width: 1.85rem; min-height: 1.85rem; padding: 0.2rem 0.35rem; color: var(--text-secondary); background: transparent; border: 1px solid var(--border); border-radius: 4px; cursor: pointer; }
  .layout-tools button.tool-add { color: var(--accent); font-weight: 700; }
  .layout-tools button:disabled { opacity: 0.4; cursor: default; }
  .layout-tools button.chosen { color: var(--accent); border-color: var(--accent); background: var(--surface-hover); }
  .empty {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .hint {
    margin: 0;
    color: var(--muted);
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
    background: var(--surface-raised);
    border: 1px solid var(--border);
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
  .settings-dialog {
    max-height: 85vh;
    overflow-y: auto;
  }
  .settings-dialog h3 {
    margin: 0.25rem 0;
    font-size: 0.85rem;
    color: var(--accent);
    font-weight: 600;
  }
  .setting {
    display: flex;
    align-items: center;
    gap: 0.65rem;
    padding: 0.35rem 0;
    color: var(--text);
    font-size: 0.82rem;
  }
  .setting input {
    accent-color: var(--accent);
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
    color: var(--text-secondary);
    background: transparent;
    border: 1px solid transparent;
    border-radius: 4px;
    cursor: pointer;
  }
  button.close:hover {
    color: var(--text);
    background: var(--surface-hover);
    border-color: var(--border);
  }
  .confirm-paste-dialog .hint {
    line-height: 1.5;
  }
  .confirm-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 0.35rem;
  }
  .dialog button.confirm {
    font: inherit;
    font-size: 0.8rem;
    color: var(--accent-contrast);
    background: var(--accent);
    border: 1px solid var(--accent);
    border-radius: 4px;
    padding: 0.35rem 0.7rem;
    cursor: pointer;
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
    color: var(--text);
    background: var(--surface-hover);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.45rem 0.6rem;
    cursor: pointer;
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }
  .picker-list button:hover {
    border-color: var(--accent);
  }
  .detail {
    font-size: 0.72rem;
    color: var(--muted);
  }
  .dialog button.ghost {
    font: inherit;
    font-size: 0.8rem;
    align-self: flex-start;
    color: var(--text-secondary);
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.35rem 0.7rem;
    cursor: pointer;
  }
</style>
