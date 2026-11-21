<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { t, type MessageKey } from "$lib/i18n/index.svelte";
  import type { TunnelInfo } from "$lib/types";
  import { THEMES, type AppTheme } from "./theme";

  let {
    theme,
    dir,
    unlocked,
    onchange,
    onsettings,
    ontunnels,
    onlock,
  }: {
    theme: AppTheme;
    dir: string;
    unlocked: boolean;
    onchange: (theme: AppTheme) => void;
    onsettings: () => void;
    ontunnels: () => void;
    onlock: () => void;
  } = $props();

  const appWindow = getCurrentWindow();
  let maximized = $state(false);
  let tunnelStatuses = $state<Record<string, TunnelInfo>>({});
  let runningTunnels = $derived(Object.values(tunnelStatuses).filter((item) =>
    item.status === "starting" || item.status === "active" || item.status === "stopping",
  ));

  onMount(() => {
    let mounted = true;
    let unlisten: (() => void) | undefined;
    let unlistenTunnels: (() => void) | undefined;
    const statusEventVersions: Record<string, number> = {};

    void appWindow.isMaximized()
      .then((value) => { if (mounted) maximized = value; })
      .catch(() => {});

    void appWindow.onResized(() => {
      void appWindow.isMaximized()
        .then((value) => { if (mounted) maximized = value; })
        .catch(() => {});
    }).then((stop) => {
      unlisten = stop;
      if (!mounted) stop();
    }).catch(() => {});

    void listen<TunnelInfo>("tunnel:status", (event) => {
      if (!mounted) return;
      const status = event.payload;
      statusEventVersions[status.connection_id] = (statusEventVersions[status.connection_id] ?? 0) + 1;
      tunnelStatuses = { ...tunnelStatuses, [status.connection_id]: status };
    }).then((stop) => {
      unlistenTunnels = stop;
      if (!mounted) {
        stop();
        return;
      }
      const versionsAtRequest = { ...statusEventVersions };
      void invoke<TunnelInfo[]>("tunnel_list").then((items) => {
        if (!mounted) return;
        const next = { ...tunnelStatuses };
        for (const item of items) {
          if ((statusEventVersions[item.connection_id] ?? 0) === (versionsAtRequest[item.connection_id] ?? 0)) {
            next[item.connection_id] = item;
          }
        }
        tunnelStatuses = next;
      }).catch(() => {});
    }).catch((error) => console.error("Could not subscribe to tunnel status", error));

    return () => {
      mounted = false;
      unlisten?.();
      unlistenTunnels?.();
    };
  });

  async function toggleMaximize() {
    try {
      await appWindow.toggleMaximize();
      maximized = await appWindow.isMaximized();
    } catch (error) {
      console.error("Could not toggle window maximize state", error);
    }
  }

  function dragWindow(event: MouseEvent) {
    if (event.button !== 0 || event.buttons !== 1) return;
    // Native hit-testing on the second mouse-down preserves double-click maximize.
    if (event.detail === 2) {
      void toggleMaximize();
      return;
    }
    void appWindow.startDragging().catch((error) => console.error("Could not drag window", error));
  }

  function dragTouchOrPen(event: PointerEvent) {
    if (event.pointerType === "mouse" || event.button !== 0) return;
    void appWindow.startDragging().catch((error) => console.error("Could not drag window", error));
  }

  function onDragKeydown(event: KeyboardEvent) {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      void toggleMaximize();
    }
  }

  async function resize(event: PointerEvent, direction: ResizeDirection) {
    if (event.button !== 0) return;
    event.preventDefault();
    try {
      await appWindow.startResizeDragging(direction);
    } catch (error) {
      console.error("Could not resize window", error);
    }
  }

  async function minimize() {
    try {
      await appWindow.minimize();
    } catch (error) {
      console.error("Could not minimize window", error);
    }
  }

  async function close() {
    try {
      await appWindow.close();
    } catch (error) {
      console.error("Could not close window", error);
    }
  }

  type ResizeDirection =
    | "East" | "North" | "NorthEast" | "NorthWest"
    | "South" | "SouthEast" | "SouthWest" | "West";
</script>

<header class="titlebar" aria-label="Application title bar">
  <button
    type="button"
    class="drag-area"
    aria-label="Window title bar; double-click or press Enter to maximize or restore"
    onmousedown={dragWindow}
    onpointerdown={dragTouchOrPen}
    onkeydown={onDragKeydown}
  >
    <span class="brand">sshterm</span>
    <span class="caption">SSH Terminal Manager</span>
    {#if dir}<span class="path" title={dir}>{dir}</span>{/if}
  </button>

  <div class="titlebar-actions">
    <select
      class="theme-select"
      value={theme}
      aria-label={t("theme.label")}
      title={t("theme.label")}
      onchange={(event) => onchange(event.currentTarget.value as AppTheme)}
    >
      {#each THEMES as id (id)}
        <option value={id}>{t(`theme.${id}` as MessageKey)}</option>
      {/each}
    </select>
    {#if runningTunnels.length > 0}
      <button
        type="button"
        class="tunnel-status-button"
        onclick={ontunnels}
        disabled={!unlocked}
        aria-label={t("tunnels.running", { count: runningTunnels.length })}
        title={unlocked ? t("tunnels.openRunning", { count: runningTunnels.length }) : t("tunnels.runningWhileLocked", { count: runningTunnels.length })}
      >
        <span class="tunnel-status-dot" aria-hidden="true"></span>
        <span>{t("tunnels.indicator", { count: runningTunnels.length })}</span>
      </button>
    {/if}
    {#if unlocked}
      <button type="button" class="action-button" onclick={onsettings}>{t("settings.title")}</button>
      <button type="button" class="action-button" onclick={onlock}>{t("home.lock")}</button>
    {/if}

    <div class="window-controls">
      <button type="button" class="window-button" onclick={minimize} aria-label={t("window.minimize")} title={t("window.minimize")}>
        <span class="minimize-icon" aria-hidden="true"></span>
      </button>
      <button
        type="button"
        class="window-button"
        onclick={toggleMaximize}
        aria-label={maximized ? t("window.restore") : t("window.maximize")}
        title={maximized ? t("window.restore") : t("window.maximize")}
      >
        {#if maximized}
          <svg viewBox="0 0 16 16" aria-hidden="true">
            <path d="M5.5 3.5h7v7h-2v2h-7v-7h2z" />
            <path d="M3.5 5.5h-1v9h9v-1" />
          </svg>
        {:else}
          <svg viewBox="0 0 16 16" aria-hidden="true">
            <rect x="3" y="3" width="10" height="10" rx=".5" />
          </svg>
        {/if}
      </button>
      <button type="button" class="window-button close-button" onclick={close} aria-label={t("window.close")} title={t("window.close")}>
        <span aria-hidden="true">×</span>
      </button>
    </div>
  </div>
</header>

<!-- Native resize gestures for the frameless window. -->
<button type="button" class="resize-hit edge-top" aria-label={t("window.resize")} tabindex="-1" onpointerdown={(event) => resize(event, "North")}></button>
<button type="button" class="resize-hit edge-bottom" aria-label={t("window.resize")} tabindex="-1" onpointerdown={(event) => resize(event, "South")}></button>
<button type="button" class="resize-hit edge-left" aria-label={t("window.resize")} tabindex="-1" onpointerdown={(event) => resize(event, "West")}></button>
<button type="button" class="resize-hit edge-right" aria-label={t("window.resize")} tabindex="-1" onpointerdown={(event) => resize(event, "East")}></button>
<button type="button" class="resize-hit corner-top-left" aria-label={t("window.resize")} tabindex="-1" onpointerdown={(event) => resize(event, "NorthWest")}></button>
<button type="button" class="resize-hit corner-top-right" aria-label={t("window.resize")} tabindex="-1" onpointerdown={(event) => resize(event, "NorthEast")}></button>
<button type="button" class="resize-hit corner-bottom-left" aria-label={t("window.resize")} tabindex="-1" onpointerdown={(event) => resize(event, "SouthWest")}></button>
<button type="button" class="resize-hit corner-bottom-right" aria-label={t("window.resize")} tabindex="-1" onpointerdown={(event) => resize(event, "SouthEast")}></button>

<style>
  .titlebar {
    position: relative;
    height: 2.25rem;
    min-height: 2.25rem;
    flex: 0 0 2.25rem;
    display: flex;
    align-items: center;
    gap: 0.6rem;
    padding-left: 0.8rem;
    background: var(--surface);
    border-bottom: 1px solid var(--border);
    color: var(--text);
    box-sizing: border-box;
    user-select: none;
    -webkit-user-select: none;
  }
  .drag-area {
    appearance: none;
    min-width: 0;
    height: 100%;
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: flex-start;
    gap: 0.65rem;
    padding: 0;
    border: 0;
    background: transparent;
    color: inherit;
    text-align: left;
    cursor: default;
    outline: none;
  }
  .brand {
    color: var(--accent);
    font-size: 0.84rem;
    font-weight: 700;
    letter-spacing: 0.015em;
  }
  .caption {
    color: var(--muted);
    font-size: 0.76rem;
    font-weight: 400;
    flex: 0 0 auto;
  }
  .path {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted);
    font-size: 0.68rem;
    font-weight: 400;
    margin-left: 0.35rem;
  }
  .tunnel-status-button {
    display: inline-flex;
    align-items: center;
    gap: 0.38rem;
    min-height: 1.55rem;
    padding: 0.2rem 0.48rem;
    border: 1px solid var(--success-border);
    border-radius: 4px;
    background: var(--success-soft);
    color: var(--success);
    font: inherit;
    font-size: 0.7rem;
    white-space: nowrap;
    cursor: pointer;
  }
  .tunnel-status-button:hover:not(:disabled) {
    border-color: var(--success);
    filter: brightness(1.08);
  }
  .tunnel-status-button:disabled {
    cursor: default;
    opacity: 0.9;
  }
  .tunnel-status-dot {
    width: 0.42rem;
    height: 0.42rem;
    flex: 0 0 0.42rem;
    border-radius: 50%;
    background: var(--success);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--success) 20%, transparent);
  }
  .window-controls {
    position: relative;
    z-index: 1002;
    display: flex;
    align-items: center;
    height: 100%;
  }
  .titlebar-actions { position:relative; z-index:1001; display:inline-flex; align-items:center; gap:.65rem; height:100%; flex:0 0 auto; }
  .theme-select {
    max-width: 7.5rem;
    font: inherit;
    font-size: 0.72rem;
    color: var(--text-secondary);
    background: var(--surface-raised);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.2rem 0.35rem;
    cursor: pointer;
  }
  .window-controls {
    gap: 0;
  }
  .action-button {
    font: inherit;
    font-size: 0.72rem;
    color: var(--text);
    background: var(--surface-hover);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.25rem 0.55rem;
    margin-right: 0.35rem;
    white-space: nowrap;
    cursor: pointer;
  }
  .action-button:hover { border-color: var(--accent); }
  .window-button {
    appearance: none;
    width: 2.85rem;
    min-width: 2.85rem;
    height: 100%;
    flex: 0 0 2.85rem;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    border: 0;
    border-radius: 0;
    background: transparent;
    color: var(--text-secondary);
    cursor: default;
  }
  .window-button:is(:hover, :focus-visible) {
    position: relative;
    z-index: 1003;
    background: var(--surface-raised);
    color: var(--accent);
    box-shadow: inset 0 -2px 0 var(--accent);
  }
  .window-button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .window-button:active {
    background: var(--surface-hover);
    box-shadow: inset 0 -3px 0 var(--accent);
  }
  .window-button svg {
    width: 0.72rem;
    height: 0.72rem;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.1;
  }
  .minimize-icon {
    display: block;
    width: 0.65rem;
    height: 1px;
    background: currentColor;
    transform: translateY(0.22rem);
  }
  .close-button {
    font-size: 1.25rem;
    line-height: 1;
  }
  .window-button.close-button:is(:hover, :focus-visible) {
    background: var(--danger-bg);
    color: var(--danger);
    box-shadow: inset 0 -2px 0 var(--danger);
  }
  .window-button.close-button:active {
    background: var(--danger);
    color: var(--accent-contrast);
    box-shadow: inset 0 -3px 0 var(--danger);
  }
  .resize-hit {
    position: fixed;
    z-index: 100;
    appearance: none;
    border: 0;
    border-radius: 0;
    padding: 0;
    margin: 0;
    background: transparent;
  }
  .edge-top, .edge-bottom {
    left: 7px;
    right: 7px;
    height: 5px;
    cursor: ns-resize;
  }
  /* Keep the top resize grip away from the title-bar action buttons. */
  .edge-top { top: 0; right: 220px; }
  .edge-bottom { bottom: 0; }
  .edge-left, .edge-right {
    top: 7px;
    bottom: 7px;
    width: 5px;
    cursor: ew-resize;
  }
  .edge-left { left: 0; }
  .edge-right { right: 0; top: 44px; }
  .corner-top-left, .corner-top-right, .corner-bottom-left, .corner-bottom-right {
    width: 10px;
    height: 10px;
  }
  .corner-top-left { top: 0; left: 0; cursor: nwse-resize; }
  /* The top-right grip would sit over the close control. */
  .corner-top-right { top: 0; right: 0; cursor: nesw-resize; pointer-events:none; }
  .corner-bottom-left { bottom: 0; left: 0; cursor: nesw-resize; }
  .corner-bottom-right { bottom: 0; right: 0; cursor: nwse-resize; }
  @media (max-width: 520px) {
    .caption { display: none; }
    .path { display: none; }
    .titlebar { gap: 0.35rem; padding-left: 0.55rem; }
    .window-button { width: 2.3rem; }
    .titlebar-actions { gap: 0.35rem; }
  }
</style>
