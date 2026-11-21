<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getLastPaneSize, rememberOpenSize, subscribeSessionErrors } from "$lib/sessionBus";
  import { hasMessage, t } from "$lib/i18n/index.svelte";
  import ConnectionSidebar from "./ConnectionSidebar.svelte";
  import ConnectionPropertiesPane from "./ConnectionPropertiesPane.svelte";
  import ConnectionEditor from "./ConnectionEditor.svelte";
  import TerminalPane from "./TerminalPane.svelte";
  import CommandConsole from "./CommandConsole.svelte";
  import HighlightRulesPanel from "./HighlightRulesPanel.svelte";
  import TunnelManager from "./TunnelManager.svelte";
  import type { BroadcastSession } from "$lib/broadcastTargets";
  import SettingsDialog from "./SettingsDialog.svelte";
  import type { AppBehaviorSettings, AppearanceSettings, AppTheme, CommandConsoleSettings, Connection, ConnectionSummary, Tab, TerminalSettings, TreeNode, WorkspacePanelSettings } from "$lib/types";
  import { DEFAULT_APP_BEHAVIOR_SETTINGS, DEFAULT_COMMAND_CONSOLE_SETTINGS, DEFAULT_TERMINAL_SETTINGS, DEFAULT_WORKSPACE_PANEL_SETTINGS } from "$lib/types";
  import { addSessionToPane, createLayoutTree, getLayoutGeometry, moveSession, moveSessionToNewPane, neighborPane, paneOfSession, removeEmptyPane, removeSession, setSplitRatioAtPath, splitPaneById, splitPaneWithEmptyLeaf, type LayoutPreset, type PaneDirection, type LayoutRect, type SplitAxis, type SplitNode, type SplitPath } from "$lib/splitTree";

  let {
    dir,
    theme,
    appearance,
    terminalOpacity = 100,
    onthemechange,
    onappearancechange,
    settingsOpen = $bindable(false),
    tunnelPanelOpen = $bindable(false),
  }: {
    dir: string;
    theme: AppTheme;
    appearance: AppearanceSettings;
    /** Terminal background opacity in percent (100 unless a background image is set). */
    terminalOpacity?: number;
    onthemechange: (theme: AppTheme) => void;
    onappearancechange: (next: AppearanceSettings) => Promise<string | null>;
    settingsOpen?: boolean;
    tunnelPanelOpen?: boolean;
  } = $props();

  let tabs = $state<Tab[]>([]);
  let activeId = $state<string | null>(null);
  let layoutPreset = $state<LayoutPreset>("single");
  // The workspace starts with one empty pane (a fresh connection opens in it).
  let layoutTree = $state<SplitNode | null>({ type: "leaf", sessionId: crypto.randomUUID(), sessionIds: [] });
  let layoutGeometry = $derived(getLayoutGeometry(layoutTree));
  let paneActiveIds = $state<Record<string, string>>({});
  // Pane body elements, by stable pane ID. Terminals are rendered once per
  // session (see the parking area in the markup) and attached to these.
  let paneEls = $state<Record<string, HTMLElement>>({});
  let sessionPane = $derived<Record<string, string>>(Object.fromEntries(
    layoutGeometry.leaves.flatMap((pane) => pane.sessionIds.map((id) => [id, pane.sessionId])),
  ));
  // Tab being dragged between panes, and where it would land.
  let draggingTab = $state<string | null>(null);
  type DropZone = "tabs" | "center" | "left" | "right" | "top" | "bottom";
  let tabDrop = $state<{ paneId: string; zone: DropZone; index?: number } | null>(null);
  // Pane that receives the next connection (opened from the tree or picker) and
  // shows the focus outline. Usually follows the active tab, but an empty pane
  // can be focused too. Always read it through currentPaneId().
  let focusedPaneId = $state<string | null>(null);
  // Where a connection dragged from the tree would land (same zones as tab drags).
  let connDrop = $state<{ paneId: string; zone: DropZone } | null>(null);
  let pendingPaneId = $state<string | null>(null);
  let panesElement = $state<HTMLDivElement | null>(null);
  let dragState = $state<{ path: SplitPath; axis: SplitAxis; region: LayoutRect } | null>(null);
  let consoleVisible = $state(DEFAULT_COMMAND_CONSOLE_SETTINGS.visible);
  let consoleHeight = $state(DEFAULT_COMMAND_CONSOLE_SETTINGS.height);
  let treeVisible = $state(DEFAULT_WORKSPACE_PANEL_SETTINGS.tree_visible);
  let propertiesVisible = $state(DEFAULT_WORKSPACE_PANEL_SETTINGS.properties_visible);
  let selectedConnectionId = $state<string | null>(null), propertiesFocusToken = $state(0), propertiesWidth = $state(320);
  let workspacePanelSettingsDirty = false;
  let workspacePanelSaveQueue: Promise<void> = Promise.resolve();
  let propertiesResize: { startX: number; startWidth: number } | null = null;
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
  let appBehavior = $state<AppBehaviorSettings>({ ...DEFAULT_APP_BEHAVIOR_SETTINGS });
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

  function toggleCommandConsole() { consoleSettingsDirty = true; consoleVisible = !consoleVisible; void persistConsoleSettings(); }

  function persistWorkspacePanelSettings() {
    const settings: WorkspacePanelSettings = {
      tree_visible: treeVisible,
      properties_visible: propertiesVisible,
    };
    workspacePanelSaveQueue = workspacePanelSaveQueue
      .then(() => invoke("config_set_workspace_panel_settings", { settings }))
      .then(() => undefined)
      .catch((e) => { error = errorText(String(e)); });
  }

  function toggleTree() {
    workspacePanelSettingsDirty = true;
    treeVisible = !treeVisible;
    persistWorkspacePanelSettings();
  }

  function toggleProperties() {
    workspacePanelSettingsDirty = true;
    propertiesVisible = !propertiesVisible;
    if (propertiesVisible) propertiesFocusToken += 1;
    persistWorkspacePanelSettings();
  }
  function startPropertiesResize(event: PointerEvent) { if (event.button !== 0) return; event.preventDefault(); propertiesResize = { startX: event.clientX, startWidth: propertiesWidth }; }
  function movePropertiesResize(event: PointerEvent) { if (propertiesResize) propertiesWidth = Math.max(250, Math.min(450, propertiesResize.startWidth + propertiesResize.startX - event.clientX)); }
  function endPropertiesResize() { propertiesResize = null; }

  onMount(() => {
    void invoke<TerminalSettings>("config_get_terminal_settings")
      .then((value) => (settings = value))
      .catch(() => {});
    void invoke<AppBehaviorSettings>("config_get_app_behavior_settings")
      .then((value) => (appBehavior = value))
      .catch(() => {});
    void invoke<CommandConsoleSettings>("config_get_command_console_settings")
      .then((value) => {
        if (!consoleSettingsDirty) {
          consoleVisible = value.visible;
          consoleHeight = clampConsoleHeight(value.height);
        }
      })
      .catch(() => {});
    void invoke<WorkspacePanelSettings>("config_get_workspace_panel_settings")
      .then((value) => {
        if (!workspacePanelSettingsDirty) {
          treeVisible = value.tree_visible;
          propertiesVisible = value.properties_visible;
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

  async function setAppBehaviorSetting<K extends keyof AppBehaviorSettings>(
    key: K,
    value: AppBehaviorSettings[K],
  ) {
    const previous = appBehavior;
    const next = { ...appBehavior, [key]: value };
    appBehavior = next;
    try {
      await invoke("config_set_app_behavior_settings", { settings: next });
    } catch (e) {
      appBehavior = previous;
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

  async function connect(c: ConnectionSummary, paneOverride?: string) {
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
      const targetPaneId = paneOverride ?? pendingPaneId ?? currentPaneId();
      if (!layoutTree || !targetPaneId) {
        layoutTree = createLayoutTree([sessionId], "single", sessionId);
        paneActiveIds = { ...paneActiveIds, [sessionId]: sessionId };
      } else {
        layoutTree = addSessionToPane(layoutTree, targetPaneId, sessionId);
        paneActiveIds = { ...paneActiveIds, [targetPaneId]: sessionId };
      }
      pendingPaneId = null;
      activeId = sessionId;
      focusedPaneId = paneOfSession(layoutTree, sessionId);
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


  async function openPickerForPane(paneId: string) {
    pendingPaneId = paneId;
    await openPicker();
  }

  function splitFocusedPane(axis: SplitAxis) {
    const target = currentPaneId();
    if (!layoutTree || !target) return;
    const paneId = crypto.randomUUID();
    const next = splitPaneById(layoutTree, target, paneId, axis);
    if (next === layoutTree) return;
    layoutTree = next;
    focusedPaneId = paneId;
    // A new empty pane asks what to put in it right away.
    void openPickerForPane(paneId);
  }

  function closePicker() {
    picker = false;
    pendingPaneId = null;
    newTabButton?.focus();
  }

  function currentPaneId(): string | null {
    const leaves = layoutGeometry.leaves;
    if (focusedPaneId && leaves.some((pane) => pane.sessionId === focusedPaneId)) return focusedPaneId;
    return leaves.find((pane) => pane.sessionIds.includes(activeId ?? ""))?.sessionId ?? leaves[0]?.sessionId ?? null;
  }

  // When the active tab changes (open, close, switch), focus follows it.
  $effect(() => {
    const id = activeId;
    untrack(() => {
      const pane = id ? paneOfSession(layoutTree, id) : null;
      if (pane) focusedPaneId = pane;
    });
  });

  /** Focus a pane: its active terminal, or the open-connection button of an empty one. */
  async function focusPane(paneId: string) {
    const pane = layoutGeometry.leaves.find((item) => item.sessionId === paneId);
    if (!pane) return;
    focusedPaneId = paneId;
    const sessionId = paneActiveIds[paneId] ?? pane.sessionIds[0];
    if (sessionId) {
      activateSession(sessionId);
      return;
    }
    await tick();
    paneEls[paneId]?.querySelector<HTMLElement>(".empty-pane button")?.focus();
  }

  function registerPane(node: HTMLElement, paneId: string) {
    paneEls[paneId] = node;
    return {
      destroy() {
        if (paneEls[paneId] === node) delete paneEls[paneId];
      },
    };
  }

  /** Move (or reorder) a session's tab; `index` is relative to the target list without it. */
  function applyTabMove(sessionId: string, targetPaneId: string, index?: number) {
    const from = paneOfSession(layoutTree, sessionId);
    const next = moveSession(layoutTree, sessionId, targetPaneId, index);
    if (!from || next === layoutTree) return;
    layoutTree = next;
    finishTabMove(sessionId, from, targetPaneId);
  }

  function applyTabMoveToNewPane(sessionId: string, targetPaneId: string, axis: SplitAxis, before: boolean) {
    const from = paneOfSession(layoutTree, sessionId);
    const newPaneId = crypto.randomUUID();
    const next = moveSessionToNewPane(layoutTree, sessionId, targetPaneId, newPaneId, axis, before);
    if (!from || next === layoutTree) return;
    layoutTree = next;
    finishTabMove(sessionId, from, newPaneId);
  }

  function finishTabMove(sessionId: string, fromPaneId: string, toPaneId: string) {
    const next = { ...paneActiveIds };
    if (fromPaneId !== toPaneId && next[fromPaneId] === sessionId) {
      const remaining = getLayoutGeometry(layoutTree).leaves.find((pane) => pane.sessionId === fromPaneId)?.sessionIds ?? [];
      if (remaining.length) next[fromPaneId] = remaining[0];
      else delete next[fromPaneId];
    }
    next[toPaneId] = sessionId;
    paneActiveIds = next;
    activeId = sessionId;
    focusedPaneId = toPaneId;
  }

  /** Keyboard path for moving tabs: send the active tab to the neighboring pane. */
  function moveActiveTab(direction: PaneDirection) {
    if (!activeId) return;
    const from = paneOfSession(layoutTree, activeId);
    if (!from) return;
    const target = neighborPane(layoutGeometry.leaves, from, direction);
    if (target) applyTabMove(activeId, target);
  }

  function onTabDragStart(event: DragEvent, sessionId: string) {
    draggingTab = sessionId;
    if (event.dataTransfer) {
      event.dataTransfer.setData("application/x-sshterm-tab", sessionId);
      event.dataTransfer.effectAllowed = "move";
    }
  }

  function endTabDrag() {
    draggingTab = null;
    tabDrop = null;
  }

  function dropZoneAt(event: DragEvent, slot: HTMLElement): DropZone {
    const rect = slot.getBoundingClientRect();
    const x = (event.clientX - rect.left) / Math.max(rect.width, 1);
    const y = (event.clientY - rect.top) / Math.max(rect.height, 1);
    const tabBar = slot.querySelector(".pane-tabs")?.getBoundingClientRect();
    if (tabBar && event.clientY <= tabBar.bottom) return "tabs";
    const edge = 0.25;
    if (x < edge) return "left";
    if (x > 1 - edge) return "right";
    if (y < edge) return "top";
    if (y > 1 - edge) return "bottom";
    return "center";
  }

  const CONN_MIME = "application/x-sshterm-conn";
  const isConnDrag = (event: DragEvent) => !draggingTab && !!event.dataTransfer?.types.includes(CONN_MIME);

  function onPaneDragOver(event: DragEvent, paneId: string) {
    if (isConnDrag(event)) {
      event.preventDefault();
      if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
      const zone = dropZoneAt(event, event.currentTarget as HTMLElement);
      if (connDrop?.paneId !== paneId || connDrop.zone !== zone) connDrop = { paneId, zone };
      return;
    }
    if (!draggingTab) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
    const slot = event.currentTarget as HTMLElement;
    const zone = dropZoneAt(event, slot);
    let index: number | undefined;
    if (zone === "tabs") {
      const pane = layoutGeometry.leaves.find((item) => item.sessionId === paneId);
      const others = (pane?.sessionIds ?? []).filter((id) => id !== draggingTab);
      const tabEl = (event.target as HTMLElement).closest<HTMLElement>("[data-tab-session]");
      const overId = tabEl?.dataset.tabSession;
      if (overId && overId !== draggingTab) {
        const box = tabEl!.getBoundingClientRect();
        index = others.indexOf(overId) + (event.clientX > box.left + box.width / 2 ? 1 : 0);
      } else if (!overId) {
        index = others.length;
      } else {
        index = pane?.sessionIds.indexOf(draggingTab!);
      }
    }
    if (tabDrop?.paneId !== paneId || tabDrop.zone !== zone || tabDrop.index !== index) {
      tabDrop = { paneId, zone, index };
    }
  }

  function onPaneDragLeave(event: DragEvent) {
    const slot = event.currentTarget as HTMLElement;
    if (!slot.contains(event.relatedTarget as Node | null)) {
      tabDrop = null;
      connDrop = null;
    }
  }

  /** Open a connection dragged from the tree: into the pane, or into a new pane beside it. */
  async function dropConnection(connId: string, paneId: string, zone: DropZone) {
    try {
      const list = await invoke<ConnectionSummary[]>("conn_list");
      const summary = list.find((item) => item.id === connId);
      if (!summary) return;
      const sides: Partial<Record<DropZone, { axis: SplitAxis; before: boolean }>> = {
        left: { axis: "horizontal", before: true },
        right: { axis: "horizontal", before: false },
        top: { axis: "vertical", before: true },
        bottom: { axis: "vertical", before: false },
      };
      const side = sides[zone];
      if (!side) {
        await connect(summary, paneId);
        return;
      }
      const newPaneId = crypto.randomUUID();
      const next = splitPaneById(layoutTree, paneId, newPaneId, side.axis, side.before);
      if (next === layoutTree) return;
      layoutTree = next;
      const before = tabs.length;
      await connect(summary, newPaneId);
      // The connection failed: do not leave the pane created for it behind.
      if (tabs.length === before) removePane(newPaneId);
    } catch (e) {
      error = errorText(String(e));
    }
  }

  function onPaneDrop(event: DragEvent, paneId: string) {
    if (isConnDrag(event)) {
      const connId = event.dataTransfer?.getData(CONN_MIME);
      const zone = dropZoneAt(event, event.currentTarget as HTMLElement);
      connDrop = null;
      event.preventDefault();
      if (connId) void dropConnection(connId, paneId, zone);
      return;
    }
    const sessionId = draggingTab;
    const drop = tabDrop;
    endTabDrag();
    if (!sessionId || !drop) return;
    event.preventDefault();
    switch (drop.zone) {
      case "tabs": applyTabMove(sessionId, drop.paneId, drop.index); break;
      case "center": applyTabMove(sessionId, drop.paneId); break;
      case "left": applyTabMoveToNewPane(sessionId, drop.paneId, "horizontal", true); break;
      case "right": applyTabMoveToNewPane(sessionId, drop.paneId, "horizontal", false); break;
      case "top": applyTabMoveToNewPane(sessionId, drop.paneId, "vertical", true); break;
      case "bottom": applyTabMoveToNewPane(sessionId, drop.paneId, "vertical", false); break;
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (
      !e.repeat
      && !e.getModifierState("AltGraph")
      && e.ctrlKey
      && e.altKey
      && !e.metaKey
      && !settingsOpen
      && !tunnelPanelOpen
      && !picker
      && !multilinePaste
    ) {
      const key = e.key.toLowerCase();
      const arrows: Record<string, PaneDirection> = { arrowleft: "left", arrowright: "right", arrowup: "up", arrowdown: "down" };
      if (arrows[key] && layoutGeometry.leaves.length > 0) {
        e.preventDefault();
        if (e.shiftKey) {
          moveActiveTab(arrows[key]);
        } else {
          const from = currentPaneId();
          const target = from ? neighborPane(layoutGeometry.leaves, from, arrows[key]) : null;
          if (target) void focusPane(target);
        }
        return;
      }
      // Previous / next pane. Ctrl+Alt+Arrows can be taken by the OS or a graphics
      // driver (screen rotation) before the app sees them, so these always work.
      if (!e.shiftKey && (e.code === "Comma" || e.code === "Period")) {
        e.preventDefault();
        const leaves = layoutGeometry.leaves;
        if (leaves.length > 1) {
          const at = leaves.findIndex((pane) => pane.sessionId === currentPaneId());
          const step = e.code === "Period" ? 1 : -1;
          void focusPane(leaves[(at + step + leaves.length) % leaves.length].sessionId);
        }
        return;
      }
      if (key === "k" && tabs.length > 0) {
        e.preventDefault();
        toggleCommandConsole();
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
    } else if (e.key === "Escape" && tunnelPanelOpen) {
      e.preventDefault();
      tunnelPanelOpen = false;
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
    if (pane) {
      paneActiveIds = { ...paneActiveIds, [pane.sessionId]: sessionId };
      focusedPaneId = pane.sessionId;
    }
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
    if (tabs.length === 0) return; // presets arrange open tabs; split buttons make empty panes
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

<svelte:window ondragend={() => (connDrop = null)} onkeydown={onKeydown} onpointermove={(event) => { moveConsoleResize(event); movePropertiesResize(event); }} onpointerup={() => { endConsoleResize(); endPropertiesResize(); }} onpointercancel={() => { endConsoleResize(); endPropertiesResize(); }} />

<div class="app">

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}

  <div class="body">
    {#if treeVisible}<ConnectionSidebar {openIds} {refreshToken} onopen={connect} onedit={edit} onnewconn={(parentId) => (editor = { conn: blank(), isNew: true, parentId })} ontunnels={() => (tunnelPanelOpen = true)} onselectconnection={(id) => (selectedConnectionId = id)} />{/if}
    {#if propertiesVisible}
      <ConnectionPropertiesPane connectionId={selectedConnectionId} focusToken={propertiesFocusToken} width={propertiesWidth} onsave={(connection) => save(connection, null)} />
      <div class="properties-resizer" role="separator" aria-orientation="vertical" aria-label={t("properties.resize")} aria-valuemin="250" aria-valuemax="450" aria-valuenow={propertiesWidth} tabindex="0" onpointerdown={startPropertiesResize} onkeydown={(event) => { if (event.key === "ArrowLeft" || event.key === "ArrowRight") { event.preventDefault(); propertiesWidth = Math.max(250, Math.min(450, propertiesWidth + (event.key === "ArrowLeft" ? 8 : -8))); } }}></div>
    {/if}
    <div class="layout-tools" role="toolbar" aria-label={t("workspace.layout")}>
      <button class:chosen={treeVisible} title={t("workspace.toggleTree")} aria-label={t("workspace.toggleTree")} aria-pressed={treeVisible} onclick={toggleTree}><svg viewBox="0 0 24 24"><path d="M4 5h16M4 12h16M4 19h16"/></svg></button>
      <button class:chosen={propertiesVisible} title={t("workspace.toggleProperties")} aria-label={t("workspace.toggleProperties")} aria-pressed={propertiesVisible} onclick={toggleProperties}><svg viewBox="0 0 24 24"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M10 4v16M13 8h5M13 12h5M13 16h3"/></svg></button>
      <span class="tool-separator"></span>
      <button bind:this={newTabButton} title={t("workspace.newTab")} aria-label={t("workspace.newTab")} onclick={openPicker}><svg viewBox="0 0 24 24"><rect x="3.5" y="4" width="17" height="16" rx="1.5"/><path d="m7 9 3 3-3 3m6 0h4"/></svg></button>
      <button title={t("workspace.layout.single")} aria-label={t("workspace.layout.single")} aria-pressed={layoutPreset === "single"} class:chosen={layoutPreset === "single"} onclick={() => chooseLayout("single")}><svg viewBox="0 0 24 24"><rect x="4" y="4" width="16" height="16"/></svg></button>
      <button title={t("workspace.layout.leftRight")} aria-label={t("workspace.layout.leftRight")} aria-pressed={layoutPreset === "left-right"} class:chosen={layoutPreset === "left-right"} onclick={() => chooseLayout("left-right")}><svg viewBox="0 0 24 24"><rect x="3" y="4" width="8" height="16"/><rect x="13" y="4" width="8" height="16"/></svg></button>
      <button title={t("workspace.layout.topBottom")} aria-label={t("workspace.layout.topBottom")} aria-pressed={layoutPreset === "top-bottom"} class:chosen={layoutPreset === "top-bottom"} onclick={() => chooseLayout("top-bottom")}><svg viewBox="0 0 24 24"><rect x="4" y="3" width="16" height="8"/><rect x="4" y="13" width="16" height="8"/></svg></button>
      <button title={t("workspace.layout.grid")} aria-label={t("workspace.layout.grid")} aria-pressed={layoutPreset === "grid"} class:chosen={layoutPreset === "grid"} onclick={() => chooseLayout("grid")}><svg viewBox="0 0 24 24"><rect x="3" y="3" width="8" height="8"/><rect x="13" y="3" width="8" height="8"/><rect x="3" y="13" width="8" height="8"/><rect x="13" y="13" width="8" height="8"/></svg></button>
      <span class="tool-separator"></span>
      <button title={t("workspace.splitLeftRight")} aria-label={t("workspace.splitLeftRight")} disabled={!currentPaneId()} onclick={() => splitFocusedPane("horizontal")}><svg viewBox="0 0 24 24"><path d="M8 6 4 10l4 4M4 10h16m-4-4 4 4-4 4"/></svg></button>
      <button title={t("workspace.splitTopBottom")} aria-label={t("workspace.splitTopBottom")} disabled={!currentPaneId()} onclick={() => splitFocusedPane("vertical")}><svg viewBox="0 0 24 24"><path d="m6 8 4-4 4 4M10 4v16m-4-4 4 4 4-4"/></svg></button>
      <div class="tool-spacer"></div>
      <button class:chosen={consoleVisible} title={consoleVisible ? t("workspace.commandConsole.hide") : t("workspace.commandConsole.show")} aria-label={consoleVisible ? t("workspace.commandConsole.hide") : t("workspace.commandConsole.show")} aria-pressed={consoleVisible} onclick={toggleCommandConsole}><svg viewBox="0 0 24 24"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="m7 9 3 3-3 3m6 0h4"/></svg></button>
    </div>
    <main>
        <div class="workspace-layout">
          <div class="panes" bind:this={panesElement}>
            {#each layoutGeometry.leaves as pane (pane.sessionId)}
              <div class="pane-slot" ondragover={(event) => onPaneDragOver(event, pane.sessionId)} ondragleave={onPaneDragLeave} ondrop={(event) => onPaneDrop(event, pane.sessionId)} onfocusin={() => (focusedPaneId = pane.sessionId)} onpointerdown={() => (focusedPaneId = pane.sessionId)} role="group" class:focused={pane.sessionId === currentPaneId()} class:split-layout={layoutGeometry.leaves.length > 1} style={`left: ${pane.rect.x * 100}%; top: ${pane.rect.y * 100}%; width: ${pane.rect.width * 100}%; height: ${pane.rect.height * 100}%;`}>
                <div class="pane-tabs" class:drop-tabs={tabDrop?.paneId === pane.sessionId && tabDrop.zone === "tabs"} role="tablist" aria-label={t("workspace.paneTabs")}>
                  {#each pane.sessionIds as sessionId (sessionId)}
                    {@const tab = tabs.find((item) => item.sessionId === sessionId)}
                    {#if tab}
                      <!-- svelte-ignore a11y_no_static_element_interactions (drag is a mouse shortcut; Ctrl+Alt+Shift+Arrows is the keyboard path) -->
                      <div class="pane-tab" data-tab-session={sessionId} draggable="true" ondragstart={(event) => onTabDragStart(event, sessionId)} ondragend={endTabDrag} class:dragging={draggingTab === sessionId} class:active={paneActiveIds[pane.sessionId] === sessionId} class:disconnected={!tab.connected}>
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
                <div class="pane-terminals" use:registerPane={pane.sessionId}>
                  {#if pane.sessionIds.length === 0}
                    <div class="empty-pane">
                      <p>{t("workspace.emptyPaneHint")}</p>
                      <button onclick={() => openPickerForPane(pane.sessionId)}>{t("workspace.openConnection")}</button>
                    </div>
                  {/if}
                </div>
                {#if connDrop?.paneId === pane.sessionId}
                  <div class="drop-zone {connDrop.zone === 'tabs' ? 'center' : connDrop.zone}" aria-hidden="true"></div>
                {/if}
                {#if draggingTab && tabDrop?.paneId === pane.sessionId && tabDrop.zone !== "tabs"}
                  <div class="drop-zone {tabDrop.zone}" aria-hidden="true"></div>
                {/if}
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
          <!-- Terminals are created once per session and attached to their pane's
               element, so moving a tab between panes never rebuilds the terminal. -->
          <div class="terminal-parking" aria-hidden="true">
            {#each tabs as tab (tab.sessionId)}
              <TerminalPane sessionId={tab.sessionId} container={paneEls[sessionPane[tab.sessionId]] ?? null}
                active={paneActiveIds[sessionPane[tab.sessionId]] === tab.sessionId}
                focused={activeId === tab.sessionId} onactivate={activateSession} onclosed={handleSessionClosed}
                onmultilinepaste={requestMultilinePaste} highlightSets={tab.highlightSets ?? []} {settings} {theme} {terminalOpacity} />
            {/each}
          </div>
        </div>
      {#if consoleVisible}
        {#if tabs.length > 0}
          <!-- svelte-ignore a11y_no_noninteractive_tabindex a11y_no_noninteractive_element_interactions -->
          <div class="console-resizer" role="separator" aria-orientation="horizontal" aria-label={t("workspace.resizeConsole")} aria-valuemin="110" aria-valuemax={maxConsoleHeight()} aria-valuenow={consoleHeight} tabindex="0" onpointerdown={startConsoleResize} onkeydown={adjustConsoleHeight} onkeyup={saveConsoleResizeAfterKeyboard} title={t("workspace.resizeConsole")}></div>
        {/if}
        <CommandConsole visible={true} sessions={consoleSessions} activeSessionId={activeId} height={consoleHeight} />
      {/if}
    </main>
  </div>

  {#if settingsOpen}
    <SettingsDialog
      {settings}
      {appBehavior}
      {theme}
      {appearance}
      onterminal={setTerminalSetting}
      onbehavior={setAppBehaviorSetting}
      onhighlight={() => (highlightPanelOpen = true)}
      onthemechange={onthemechange}
      onappearance={onappearancechange}
      onclose={() => (settingsOpen = false)}
    />
  {/if}

  {#if highlightPanelOpen}
    <HighlightRulesPanel onclose={() => (highlightPanelOpen = false)} />
  {/if}

  {#if tunnelPanelOpen}
    <TunnelManager
      onclose={() => (tunnelPanelOpen = false)}
      onedit={(connection) => {
        tunnelPanelOpen = false;
        void edit(connection);
      }}
    />
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
          <div class="picker-list">
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
  .properties-resizer { flex: 0 0 4px; cursor: col-resize; background: var(--border); touch-action: none; }
  .properties-resizer:hover, .properties-resizer:focus-visible { background: var(--accent); }
  .layout-tools { display:flex; flex-direction:column; align-items:center; flex:0 0 48px; gap:4px; padding:8px 7px; box-sizing:border-box; background:var(--surface); border-right:1px solid var(--border); }
  .layout-tools button { width:32px; height:32px; min-width:32px; min-height:32px; display:inline-flex; align-items:center; justify-content:center; padding:0; color:var(--muted); background:transparent; border:1px solid transparent; border-radius:4px; cursor:pointer; }
  .layout-tools button svg { width:20px; height:20px; fill:none; stroke:currentColor; stroke-width:1.7; stroke-linecap:round; stroke-linejoin:round; }
  .layout-tools button:hover { color:var(--text); background:var(--surface-hover); border-color:var(--border); }
  .layout-tools button:focus-visible { outline:2px solid var(--accent); outline-offset:1px; }
  .layout-tools button:disabled { opacity:.4; cursor:default; }
  .layout-tools button.chosen { color:var(--accent); background:var(--surface-raised); border-color:var(--border); }
  .tool-spacer { flex:1 1 auto; }
  .panes { position: relative; flex: 1; min-height: 0; min-width: 0; overflow: hidden; background: var(--app-bg); }
  .pane-slot { position: absolute; display: flex; flex-direction: column; min-width: 0; min-height: 0; overflow: hidden; background: var(--surface); }
  .pane-slot.focused { outline:1px solid var(--accent); outline-offset:-1px; z-index:2; }
  .pane-tabs { display: flex; flex: 0 0 auto; min-height: 1.7rem; overflow-x: auto; gap: 2px; padding: 2px 3px; background: var(--surface); border-bottom: 1px solid var(--border); }
  .pane-tab { display: flex; align-items: center; min-width: 0; max-width: 11rem; border: 1px solid var(--border); border-radius: 3px; background: var(--surface-hover); }
  .pane-tab.active { border-color:var(--border); border-top:2px solid var(--accent); border-bottom-color:var(--surface-raised); border-radius:4px 4px 0 0; margin-bottom:-1px; background:var(--surface-raised); }
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
  .pane-add { width:28px; height:28px; display:inline-flex; align-items:center; justify-content:center; color:var(--accent); font-size:18px; line-height:1; }
  .pane-add:hover { color:var(--text); background:var(--surface-hover); border-radius:4px; }
  .pane-remove { color: var(--text-secondary); }
  .pane-remove:hover { color: var(--danger, #e06c75); }
  .empty-pane { display: flex; flex-direction: column; gap: 0.6rem; width: 100%; height: 100%; align-items: center; justify-content: center; text-align: center; }
  .empty-pane p { margin: 0; max-width: 18rem; font-size: 0.74rem; color: var(--muted); }
  .empty-pane button { font: inherit; font-size: 0.78rem; color: var(--text-secondary); background: var(--surface-hover); border: 1px solid var(--border); border-radius: 4px; padding: 0.4rem 0.7rem; cursor: pointer; }
  .tool-separator { width:24px; height:1px; margin:3px 0; background:var(--border); }
  .console-resizer { flex: 0 0 6px; cursor: row-resize; background: var(--border); touch-action: none; outline: none; }
  .console-resizer:hover, .console-resizer:focus-visible { background: var(--accent); }
  .pane-terminals { position: relative; flex: 1; min-height: 0; overflow: hidden; }
  /* Off-screen but laid out, so xterm can measure characters while parked. */
  .terminal-parking { position: fixed; left: -10000px; top: 0; width: 800px; height: 400px; overflow: hidden; visibility: hidden; pointer-events: none; }
  .pane-tab[draggable="true"] { cursor: grab; }
  .pane-tab.dragging { opacity: 0.45; }
  .pane-tabs.drop-tabs { outline:2px solid var(--accent); outline-offset:-2px; background:var(--surface-raised); }
  .drop-zone { position: absolute; z-index: 3; pointer-events: none; background: color-mix(in srgb, var(--accent) 22%, transparent); border: 2px solid var(--accent); border-radius: 3px; }
  .drop-zone.center { inset: 1.9rem 0 0 0; }
  .drop-zone.left { top: 1.9rem; bottom: 0; left: 0; width: 50%; }
  .drop-zone.right { top: 1.9rem; bottom: 0; right: 0; width: 50%; }
  .drop-zone.top { top: 1.9rem; left: 0; right: 0; height: calc(50% - 1rem); }
  .drop-zone.bottom { bottom: 0; left: 0; right: 0; height: calc(50% - 1rem); }
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
