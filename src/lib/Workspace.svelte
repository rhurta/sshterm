<script lang="ts">
  import { onMount, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { neighborPane, type Direction } from "$lib/paneNav";
  import { getLastPaneSize, rememberOpenSize, subscribeSessionErrors } from "$lib/sessionBus";
  import { hasMessage, t } from "$lib/i18n/index.svelte";
  import ConnectionSidebar from "./ConnectionSidebar.svelte";
  import ConnectionEditor from "./ConnectionEditor.svelte";
  import TerminalPane from "./TerminalPane.svelte";
  import CommandConsole from "./CommandConsole.svelte";
  import HighlightRulesPanel from "./HighlightRulesPanel.svelte";
  import type { BroadcastSession } from "$lib/broadcastTargets";
  import LanguageSelect from "./LanguageSelect.svelte";
  import type { AppTheme, CommandConsoleSettings, Connection, ConnectionSummary, Tab, TerminalSettings, TreeNode } from "$lib/types";
  import { DEFAULT_COMMAND_CONSOLE_SETTINGS, DEFAULT_TERMINAL_SETTINGS } from "$lib/types";
  import { addSessionToPane, createLayoutTree, getLayoutGeometry, removeEmptyPane, removeSession, setSplitRatioAtPath, splitPaneWithEmptyLeaf, type LayoutPreset, type LayoutRect, type SplitAxis, type SplitNode, type SplitPath } from "$lib/splitTree";

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
  let panesElement = $state<HTMLDivElement | null>(null);
  let dragState = $state<{ path: SplitPath; axis: SplitAxis; region: LayoutRect } | null>(null);
  let consoleVisible = $state(DEFAULT_COMMAND_CONSOLE_SETTINGS.visible);
  let consoleHeight = $state(DEFAULT_COMMAND_CONSOLE_SETTINGS.height);
  let consoleSettingsDirty = false;
  let consoleDrag = $state<{ startY: number; startHeight: number } | null>(null);
  let editor = $state<{ conn: Connection; isNew: boolean; parentId: string | null } | null>(null);
  let highlightPanelOpen = $state(false);
  let picker = $state(false);
  let pickerList = $state<ConnectionSummary[]>([]);
  let pickerQuery = $state("");
  let filteredPickerList = $derived.by(() => {
    const query = pickerQuery.trim().toLocaleLowerCase();
    if (!query) return pickerList;
    return pickerList.filter((c) => [c.name, c.host, c.user, `${c.user}@${c.host}:${c.port}`]
      .some((value) => value.toLocaleLowerCase().includes(query)));
  });
  let pickerDialog = $state<HTMLDivElement | null>(null);
  let pickerSearchInput = $state<HTMLInputElement | null>(null);
  let newTabButton = $state<HTMLButtonElement | null>(null);
  let refreshToken = $state(0);
  let error = $state("");
  let settings = $state<TerminalSettings>({ ...DEFAULT_TERMINAL_SETTINGS });
  let multilinePaste = $state<{ lineCount: number } | null>(null);
  let multilinePasteCancelButton = $state<HTMLButtonElement | null>(null);
  let multilinePasteResolver: ((confirmed: boolean) => void) | null = null;

  let openIds = $derived(tabs.filter((tab) => tab.connected).map((tab) => tab.connId));
  let consoleSessions = $derived<BroadcastSession[]>(tabs.map((tab) => ({
    sessionId: tab.sessionId,
    name: tab.name,
    protected: tab.protected,
    connected: tab.connected,
  })));

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

  function maxConsoleHeight(): number {
    return Math.max(160, Math.min(900, Math.floor(window.innerHeight * 0.65)));
  }

  function clampConsoleHeight(value: number): number {
    return Math.max(110, Math.min(maxConsoleHeight(), Math.round(value)));
  }

  async function persistConsoleSettings() {
    try {
      await invoke("config_set_command_console_settings", {
        settings: { visible: consoleVisible, height: Math.round(consoleHeight) },
      });
    } catch (e) {
      error = String(e);
    }
  }

  function toggleCommandConsole() {
    consoleSettingsDirty = true;
    consoleVisible = !consoleVisible;
    void persistConsoleSettings();
  }

  onMount(() => {
    void invoke<TerminalSettings>("config_get_terminal_settings")
      .then((value) => (settings = value))
      .catch(() => {});
    void invoke<CommandConsoleSettings>("config_get_command_console_settings")
      .then((value) => {
        if (!consoleSettingsDirty) {
          consoleVisible = value.visible;
          consoleHeight = clampConsoleHeight(value.height);
        }
      })
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
      const highlightSets = await invoke<string[]>("highlight_resolve", { connectionId: c.id });
      // Open with the size of the last fitted terminal so ssh starts at about
      // the right dimensions; the pane only resizes if it really differs.
      const size = getLastPaneSize();
      const sessionId = await invoke<string>("session_open", {
        connId: c.id,
        cols: size.cols,
        rows: size.rows,
      });
      rememberOpenSize(sessionId, size.cols, size.rows);
      tabs = [...tabs, {
        sessionId,
        connId: c.id,
        name: c.name,
        jumpNames,
        tunnelCount: c.tunnel_count ?? 0,
        protected: c.protected,
        connected: true,
        highlightSets,
      }];
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
      pickerQuery = "";
      picker = true;
      // Put focus directly in the searchable list whenever the picker opens.
      // This keeps both the + button and empty-pane entry points keyboard-first.
      await tick();
      pickerSearchInput?.focus();
      pickerSearchInput?.select();
      if (!pickerSearchInput) pickerDialog?.focus();
    } catch (e) {
      error = errorText(String(e));
    }
  }


  function onPickerSearchKeydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown") {
      const first = pickerDialog?.querySelector<HTMLButtonElement>(".picker-list button");
      if (first) {
        e.preventDefault();
        first.focus();
      }
    } else if (e.key === "Enter" && filteredPickerList.length > 0) {
      e.preventDefault();
      void connect(filteredPickerList[0]);
    }
  }

  function onPickerListKeydown(e: KeyboardEvent) {
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)) return;
    const items = [...(e.currentTarget as HTMLElement).querySelectorAll<HTMLButtonElement>("button")];
    const index = items.indexOf(document.activeElement as HTMLButtonElement);
    e.preventDefault();
    if ((e.key === "ArrowUp" && index <= 0)) {
      pickerSearchInput?.focus();
      return;
    }
    const next = e.key === "Home" ? 0 : e.key === "End" ? items.length - 1 : index + (e.key === "ArrowDown" ? 1 : -1);
    items[Math.max(0, Math.min(items.length - 1, next))]?.focus();
  }

  /** Ctrl+Alt+Arrows: move focus to the neighboring pane of a split layout. */
  function focusNeighborPane(direction: Direction) {
    const current = layoutGeometry.leaves.find((pane) => pane.sessionIds.includes(activeId ?? ""));
    if (!current) return;
    const next = neighborPane(layoutGeometry.leaves, current.sessionId, direction);
    if (!next) return;
    activateSession(paneActiveIds[next.sessionId] ?? next.sessionIds[0]);
  }

  /** Ctrl+Alt+PageUp/PageDown: previous/next tab inside the focused pane. */
  function cyclePaneTab(step: 1 | -1) {
    const current = layoutGeometry.leaves.find((pane) => pane.sessionIds.includes(activeId ?? ""));
    if (!current || current.sessionIds.length < 2) return;
    const index = current.sessionIds.indexOf(activeId ?? "");
    const count = current.sessionIds.length;
    activateSession(current.sessionIds[(index + step + count) % count]);
  }

  /** Ctrl+Alt+F: jump to the connection search box (also from a terminal). */
  function focusConnectionSearch() {
    const input = document.querySelector<HTMLInputElement>('[data-nav="connection-search"]');
    input?.focus();
    input?.select();
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
    if (
      !e.repeat
      && !e.getModifierState("AltGraph")
      && e.ctrlKey
      && e.altKey
      && !e.metaKey
      && !settingsOpen
      && !picker
      && !multilinePaste
    ) {
      const key = e.key.toLowerCase();
      if (key === "k" && tabs.length > 0) {
        e.preventDefault();
        toggleCommandConsole();
        return;
      }
      const directions: Record<string, Direction> = {
        arrowleft: "left",
        arrowright: "right",
        arrowup: "up",
        arrowdown: "down",
      };
      if (directions[key]) {
        e.preventDefault();
        focusNeighborPane(directions[key]);
        return;
      }
      if (key === "pageup" || key === "pagedown") {
        e.preventDefault();
        cyclePaneTab(key === "pagedown" ? 1 : -1);
        return;
      }
      if (key === "f") {
        e.preventDefault();
        focusConnectionSearch();
        return;
      }
      const presets: Record<string, LayoutPreset> = {
        "1": "single",
        "2": "left-right",
        "3": "top-bottom",
        "4": "grid",
      };
      const preset = presets[key];
      if (preset && tabs.length > 0) {
        e.preventDefault();
        chooseLayout(preset);
        return;
      }
    }

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

  function findConnectionParent(nodes: TreeNode[], connectionId: string, parentId: string | null = null): string | null | undefined {
    for (const node of nodes) {
      if (node.type === "connection" && node.id === connectionId) return parentId;
      if (node.type === "folder") {
        const found = findConnectionParent(node.children, connectionId, node.id);
        if (found !== undefined) return found;
      }
    }
    return undefined;
  }

  async function toggleSessionProtection(sessionId: string) {
    const tab = tabs.find((item) => item.sessionId === sessionId);
    if (!tab || !tab.connected) return;
    const protectedNext = !tab.protected;
    try {
      const [connection, tree] = await Promise.all([
        invoke<Connection | null>("conn_get", { id: tab.connId }),
        invoke<TreeNode[]>("tree_get"),
      ]);
      if (!connection) throw new Error("connection_not_found");
      const parentId = findConnectionParent(tree, tab.connId);
      if (parentId === undefined) throw new Error("connection_not_in_tree");
      await invoke("conn_upsert", { conn: { ...connection, protected: protectedNext }, parentId });
      tabs = tabs.map((item) => item.connId === tab.connId ? { ...item, protected: protectedNext } : item);
      refreshToken += 1;
      error = "";
    } catch (e) { error = errorText(String(e)); }
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

  function handleSessionClosed(sessionId: string) {
    if (!tabs.some((tab) => tab.sessionId === sessionId)) return;
    // The PTY has reached EOF. Remove the dead transport from the backend even
    // when the visible terminal is retained for error inspection.
    void invoke("session_close", { id: sessionId }).catch(() => {});
    if (settings.keep_disconnected_sessions) {
      tabs = tabs.map((tab) => tab.sessionId === sessionId
        ? { ...tab, connected: false }
        : tab);
    } else {
      dropTab(sessionId);
    }
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


  function removePane(paneId: string) {
    const pane = layoutGeometry.leaves.find((item) => item.sessionId === paneId);
    if (!pane || pane.sessionIds.length > 0 || layoutGeometry.leaves.length <= 1) return;
    layoutTree = removeEmptyPane(layoutTree, paneId);
    const { [paneId]: _removed, ...rest } = paneActiveIds;
    paneActiveIds = rest;
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


  function startConsoleResize(event: PointerEvent) {
    event.preventDefault();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    consoleSettingsDirty = true;
    consoleDrag = { startY: event.clientY, startHeight: consoleHeight };
  }

  function moveConsoleResize(event: PointerEvent) {
    if (!consoleDrag) return;
    consoleHeight = clampConsoleHeight(consoleDrag.startHeight + consoleDrag.startY - event.clientY);
  }

  function endConsoleResize() {
    if (!consoleDrag) return;
    consoleDrag = null;
    void persistConsoleSettings();
  }

  function adjustConsoleHeight(event: KeyboardEvent) {
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    event.preventDefault();
    consoleSettingsDirty = true;
    const delta = event.key === "ArrowUp" ? 12 : -12;
    consoleHeight = clampConsoleHeight(consoleHeight + delta);
  }

  function saveConsoleResizeAfterKeyboard(event: KeyboardEvent) {
    if (event.key === "ArrowUp" || event.key === "ArrowDown") {
      void persistConsoleSettings();
    }
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

<svelte:window onkeydown={onKeydown} onpointermove={moveConsoleResize} onpointerup={endConsoleResize} onpointercancel={endConsoleResize} />

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
          <div class="layout-tools" role="toolbar" aria-label={t("workspace.layout")}>
            <button bind:this={newTabButton} class="tool-add" title={t("workspace.newTab")} onclick={openPicker}>+</button>
            <button class:chosen={layoutPreset === "single"} title={t("workspace.layout.single")} aria-label={t("workspace.layout.single")} aria-pressed={layoutPreset === "single"} onclick={() => chooseLayout("single")}>1</button>
            <button class:chosen={layoutPreset === "left-right"} title={t("workspace.layout.leftRight")} aria-label={t("workspace.layout.leftRight")} aria-pressed={layoutPreset === "left-right"} onclick={() => chooseLayout("left-right")}>▥</button>
            <button class:chosen={layoutPreset === "top-bottom"} title={t("workspace.layout.topBottom")} aria-label={t("workspace.layout.topBottom")} aria-pressed={layoutPreset === "top-bottom"} onclick={() => chooseLayout("top-bottom")}>▤</button>
            <button class:chosen={layoutPreset === "grid"} title={t("workspace.layout.grid")} aria-label={t("workspace.layout.grid")} aria-pressed={layoutPreset === "grid"} onclick={() => chooseLayout("grid")}>▦</button>
            <span class="tool-separator"></span>
            <button title={t("workspace.splitLeftRight")} aria-label={t("workspace.splitLeftRight")} disabled={!activeId} onclick={() => splitFocusedPane("horizontal")}>↔</button>
            <button title={t("workspace.splitTopBottom")} aria-label={t("workspace.splitTopBottom")} disabled={!activeId} onclick={() => splitFocusedPane("vertical")}>↕</button>
            <span class="tool-separator"></span>
            <button class:chosen={consoleVisible} title={consoleVisible ? t("workspace.commandConsole.hide") : t("workspace.commandConsole.show")} aria-label={consoleVisible ? t("workspace.commandConsole.hide") : t("workspace.commandConsole.show")} aria-pressed={consoleVisible} onclick={toggleCommandConsole}>⌨</button>
          </div>
          <div class="panes" bind:this={panesElement}>
            {#each layoutGeometry.leaves as pane (pane.sessionId)}
              <div class="pane-slot" class:focused={pane.sessionIds.includes(activeId ?? "")} class:split-layout={layoutGeometry.leaves.length > 1} style={`left: ${pane.rect.x * 100}%; top: ${pane.rect.y * 100}%; width: ${pane.rect.width * 100}%; height: ${pane.rect.height * 100}%;`}>
                <div class="pane-tabs" role="tablist" aria-label={t("workspace.paneTabs")}>
                  {#each pane.sessionIds as sessionId (sessionId)}
                    {@const tab = tabs.find((item) => item.sessionId === sessionId)}
                    {#if tab}
                      <div class="pane-tab" class:active={paneActiveIds[pane.sessionId] === sessionId} class:disconnected={!tab.connected}>
                        <button role="tab" aria-selected={paneActiveIds[pane.sessionId] === sessionId}
                          aria-label={`${tab.name}${activeId === sessionId ? ", active pane" : ""}${!tab.connected ? `, ${t("workspace.disconnected")}` : ""}`}
                          title={tab.jumpNames.length ? `${tab.jumpNames.join(" → ")} → ${tab.name}` : tab.name}
                          onclick={() => activateSession(sessionId)}>
                          <span class="pane-tab-name">{tab.name}</span>
                          {#if tab.jumpNames.length > 0}
                            <span class="pane-tab-indicator jump" title={t("workspace.jumpVia", { route: tab.jumpNames.join(" → ") })}>
                              ↗ {tab.jumpNames[0]}{tab.jumpNames.length > 1 ? ` +${tab.jumpNames.length - 1}` : ""}
                            </span>
                          {/if}
                          {#if tab.tunnelCount > 0}
                            <span class="pane-tab-indicator tunnels" title={t("workspace.tunnelsConfigured", { count: tab.tunnelCount })}>
                              ⇄ {tab.tunnelCount}
                            </span>
                          {/if}
                          {#if !tab.connected}
                            <span class="pane-tab-indicator disconnected" title={t("workspace.disconnected")}>OFF</span>
                          {/if}
                          {#if activeId === sessionId}
                            <span class="pane-focus-dot" aria-hidden="true"></span>
                          {/if}
                        </button>
                        <button class="pane-tab-protect" class:protected={tab.protected}
                          aria-label={tab.protected ? t("workspace.session.unprotect") : t("workspace.session.protect")}
                          aria-pressed={tab.protected}
                          title={tab.protected ? t("workspace.session.unprotect") : t("workspace.session.protect")}
                          disabled={!tab.connected} onclick={() => toggleSessionProtection(sessionId)}
                        >{tab.protected ? "🔒" : "🔓"}</button>
                        <button class="pane-tab-close" aria-label={`Close ${tab.name}`} title={t("workspace.close")} onclick={() => closeTab(sessionId)}>×</button>
                      </div>
                    {/if}
                  {/each}
                  <button class="pane-add" title={t("workspace.openConnectionInPane")} aria-label={t("workspace.openConnectionInPane")} onclick={() => openPickerForPane(pane.sessionId)}>+</button>
                  {#if pane.sessionIds.length === 0 && layoutGeometry.leaves.length > 1}
                    <button class="pane-remove" title={t("workspace.removeEmptyPane")} aria-label={t("workspace.removeEmptyPane")} onclick={() => removePane(pane.sessionId)}>×</button>
                  {/if}
                </div>
                <div class="pane-terminals">
                  {#if pane.sessionIds.length === 0}
                    <div class="empty-pane"><button onclick={() => openPickerForPane(pane.sessionId)}>{t("workspace.openConnection")}</button></div>
                  {/if}
                  {#each pane.sessionIds as sessionId (sessionId)}
                    <TerminalPane sessionId={sessionId} active={paneActiveIds[pane.sessionId] === sessionId}
                      focused={activeId === sessionId} onactivate={activateSession} onclosed={handleSessionClosed}
                      onmultilinepaste={requestMultilinePaste} highlightSets={tabs.find((tab) => tab.sessionId === sessionId)?.highlightSets ?? []} {settings} {theme} />
                  {/each}
                </div>
              </div>
            {/each}
            {#each layoutGeometry.dividers as divider}
              <!-- svelte-ignore a11y_no_noninteractive_tabindex a11y_no_noninteractive_element_interactions -->
              <div class="layout-divider" class:vertical={divider.axis === "horizontal"} class:horizontal={divider.axis === "vertical"}
                style={dividerStyle(divider.axis, divider.rect)} role="separator"
                aria-orientation={divider.axis === "horizontal" ? "vertical" : "horizontal"}
                aria-label={t("workspace.resizePanes")} aria-valuemin="15" aria-valuemax="85"
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
      {#if consoleVisible && tabs.length > 0}
        <!-- svelte-ignore a11y_no_noninteractive_tabindex a11y_no_noninteractive_element_interactions -->
        <div class="console-resizer" role="separator" aria-orientation="horizontal"
          aria-label={t("workspace.resizeConsole")} aria-valuemin="110" aria-valuemax={maxConsoleHeight()}
          aria-valuenow={consoleHeight} tabindex="0" onpointerdown={startConsoleResize}
          onkeydown={adjustConsoleHeight} onkeyup={saveConsoleResizeAfterKeyboard} title={t("workspace.resizeConsole")}></div>
      {/if}
      <CommandConsole visible={consoleVisible && tabs.length > 0} sessions={consoleSessions} activeSessionId={activeId} height={consoleHeight} />
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
        <h3>{t("workspace.keyboardShortcuts")}</h3>
        <p class="hint">{t("workspace.shortcutConsoleHelp")}</p>
        <p class="hint">{t("workspace.shortcutLayoutHelp")}</p>
        <p class="hint">{t("console.shortcutSendHelp")}</p>
        <h3>{t("settings.terminal")}</h3>
        <button class="ghost" onclick={() => (highlightPanelOpen = true)}>{t("settings.highlightRules")}</button>
        <label class="setting"><input type="checkbox" checked={settings.copy_on_selection} onchange={(e) => setTerminalSetting("copy_on_selection", e.currentTarget.checked)} />{t("settings.copyOnSelection")}</label>
        <label class="setting"><input type="checkbox" checked={settings.right_click_copy} onchange={(e) => setTerminalSetting("right_click_copy", e.currentTarget.checked)} />{t("settings.rightClickCopy")}</label>
        <label class="setting"><input type="checkbox" checked={settings.middle_click_copy} onchange={(e) => setTerminalSetting("middle_click_copy", e.currentTarget.checked)} />{t("settings.middleClickCopy")}</label>
        <label class="setting"><input type="checkbox" checked={settings.right_click_paste} onchange={(e) => setTerminalSetting("right_click_paste", e.currentTarget.checked)} />{t("settings.rightClickPaste")}</label>
        <label class="setting"><input type="checkbox" checked={settings.middle_click_paste} onchange={(e) => setTerminalSetting("middle_click_paste", e.currentTarget.checked)} />{t("settings.middleClickPaste")}</label>
        <label class="setting"><input type="checkbox" checked={settings.ctrl_c_copy} onchange={(e) => setTerminalSetting("ctrl_c_copy", e.currentTarget.checked)} />{t("settings.ctrlCCopy")}</label>
        <label class="setting"><input type="checkbox" checked={settings.confirm_multiline_paste} onchange={(e) => setTerminalSetting("confirm_multiline_paste", e.currentTarget.checked)} />{t("settings.confirmMultilinePaste")}</label>
        <label class="setting"><input type="checkbox" checked={settings.keep_disconnected_sessions} onchange={(e) => setTerminalSetting("keep_disconnected_sessions", e.currentTarget.checked)} />{t("settings.keepDisconnectedSessions")}</label>
        <button class="ghost" onclick={() => (settingsOpen = false)}>{t("editor.cancel")}</button>
      </div>
    </div>
  {/if}

  {#if highlightPanelOpen}
    <HighlightRulesPanel onclose={() => (highlightPanelOpen = false)} />
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
        <input
          class="picker-search"
          bind:this={pickerSearchInput}
          type="search"
          bind:value={pickerQuery}
          onkeydown={onPickerSearchKeydown}
          placeholder={t("tree.search")}
          aria-label={t("tree.search")}
          autocomplete="off"
          spellcheck="false"
        />
        {#if pickerList.length === 0}
          <p class="hint">{t("workspace.pickerEmpty")}</p>
        {:else if filteredPickerList.length === 0}
          <p class="hint">{t("tree.noMatches")}</p>
        {:else}
          <div class="picker-list" role="presentation" onkeydown={onPickerListKeydown}>
            {#each filteredPickerList as c (c.id)}
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
  .workspace-layout { flex: 1; min-height: 0; min-width: 0; display: flex; overflow: hidden; }
  .panes { position: relative; flex: 1; min-height: 0; min-width: 0; overflow: hidden; background: var(--app-bg); }
  .pane-slot { position: absolute; display: flex; flex-direction: column; min-width: 0; min-height: 0; overflow: hidden; background: var(--surface); }
  .pane-slot.focused { outline: 2px solid var(--accent); outline-offset: -2px; z-index: 2; }
  .pane-slot.focused.split-layout { outline-offset: -6px; }
  .pane-tabs { display: flex; flex: 0 0 auto; min-height: 1.7rem; overflow-x: auto; gap: 2px; padding: 2px 3px; background: var(--surface); border-bottom: 1px solid var(--border); }
  .pane-tab { display: flex; align-items: center; min-width: 0; max-width: 11rem; border: 1px solid var(--border); border-radius: 3px; background: var(--surface-hover); }
  .pane-tab.active { border-color: var(--accent); background: var(--surface-raised); }
  .pane-tab button { min-width: 0; display: flex; align-items: center; gap: 0.35rem; overflow: hidden; padding: 0.18rem 0.35rem; color: var(--text-secondary); background: transparent; border: 0; font: inherit; font-size: 0.68rem; cursor: pointer; }
  .pane-tab-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pane-tab-indicator {
    flex: 0 0 auto;
    max-width: 4.5rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding: 0.04rem 0.22rem;
    border-radius: 3px;
    font-size: 0.58rem;
    line-height: 1.35;
  }
  .pane-tab-indicator.jump { color: var(--accent); background: var(--surface-hover); }
  .pane-tab-indicator.tunnels { color: #78bfa5; background: var(--surface-hover); }
  .pane-tab-indicator.disconnected { color: var(--danger); background: var(--danger-bg); font-weight: 700; }
  .pane-tab.disconnected { border-style: dashed; }
  .pane-tab.disconnected button { color: var(--text-secondary); }
  .pane-focus-dot { flex: 0 0 7px; width: 7px; height: 7px; border-radius: 50%; background: var(--accent); box-shadow: 0 0 5px var(--accent); }
  .pane-tab.active button { color: var(--text); }
  .pane-tab .pane-tab-protect { flex: 0 0 auto; padding: 0.12rem 0.22rem; color: var(--muted); background: transparent; border: 0; font-size: 0.65rem; line-height: 1; cursor: pointer; }
  .pane-tab .pane-tab-protect.protected { color: var(--warning, #d6a85f); }
  .pane-tab .pane-tab-protect:disabled { opacity: 0.35; cursor: default; }
  .pane-tab .pane-tab-close { flex: 0 0 auto; padding-inline: 0.28rem; }
  .pane-add, .pane-remove { flex: 0 0 auto; align-self: center; border: 0; background: transparent; font: inherit; cursor: pointer; padding: 0 0.35rem; }
  .pane-add { color: var(--accent); }
  .pane-remove { color: var(--text-secondary); }
  .pane-remove:hover { color: var(--danger, #e06c75); }
  .empty-pane { display: flex; width: 100%; height: 100%; align-items: center; justify-content: center; }
  .empty-pane button { font: inherit; font-size: 0.78rem; color: var(--text-secondary); background: var(--surface-hover); border: 1px solid var(--border); border-radius: 4px; padding: 0.4rem 0.7rem; cursor: pointer; }
  .tool-separator { height: 1px; margin: 0.2rem 0; background: var(--border); }
  .console-resizer { flex: 0 0 6px; cursor: row-resize; background: var(--border); touch-action: none; outline: none; }
  .console-resizer:hover, .console-resizer:focus-visible { background: var(--accent); }
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
  .picker-search {
    width: 100%;
    box-sizing: border-box;
    font: inherit;
    font-size: 0.8rem;
    color: var(--text);
    background: var(--app-bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.4rem 0.5rem;
  }
  .picker-search:focus { outline: none; border-color: var(--accent); }
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
