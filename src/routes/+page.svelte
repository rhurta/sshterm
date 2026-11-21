<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import LockScreen from "$lib/LockScreen.svelte";
  import Workspace from "$lib/Workspace.svelte";
  import TitleBar from "$lib/TitleBar.svelte";
  import { initLocale } from "$lib/i18n/index.svelte";
  import { startSessionBus } from "$lib/sessionBus";
  import type { AppTheme } from "$lib/theme";
  import { DEFAULT_APPEARANCE_SETTINGS, type AppearanceSettings } from "$lib/types";

  // Register before any session can be opened, so no early PTY output is lost.
  startSessionBus();

  const DEFAULT_THEME: AppTheme = "dark";
  let ready = $state(false); // avoid briefly showing the wrong locale or theme
  let unlocked = $state(false);
  let settingsOpen = $state(false);
  let tunnelPanelOpen = $state(false);
  let dir = $state("");
  let theme = $state<AppTheme>(DEFAULT_THEME);
  let appearance = $state<AppearanceSettings>({ ...DEFAULT_APPEARANCE_SETTINGS });
  // Background image as a data URL ("" = none). Opacity only applies while one is set.
  let backgroundData = $state("");

  async function loadBackground(path: string): Promise<string> {
    if (!path) return "";
    return invoke<string>("appearance_background_data", { path });
  }

  Promise.all([
    initLocale(),
    invoke<AppTheme>("config_get_theme").catch(() => DEFAULT_THEME),
    invoke<AppearanceSettings>("config_get_appearance_settings").catch(() => ({ ...DEFAULT_APPEARANCE_SETTINGS })),
  ])
    .then(async ([, savedTheme, savedAppearance]) => {
      theme = savedTheme;
      appearance = savedAppearance;
      // A missing image must never block startup.
      backgroundData = await loadBackground(savedAppearance.background_image).catch(() => "");
    })
    .finally(() => (ready = true));

  invoke<string>("app_info").then((d) => (dir = d));

  async function changeTheme(next: AppTheme) {
    const previous = theme;
    theme = next;
    try {
      await invoke("config_set_theme", { theme: next });
    } catch (error) {
      theme = previous;
      console.error("Could not save application theme", error);
    }
  }

  /**
   * Saves appearance settings. When the image path changed it is loaded first, so an
   * invalid file is rejected without touching the saved settings. Returns an error
   * code for the settings dialog, or null on success.
   */
  async function changeAppearance(next: AppearanceSettings): Promise<string | null> {
    let data = backgroundData;
    if (next.background_image !== appearance.background_image) {
      try {
        data = await loadBackground(next.background_image);
      } catch (error) {
        return String(error);
      }
    }
    try {
      await invoke("config_set_appearance_settings", { settings: next });
    } catch (error) {
      return String(error);
    }
    appearance = next;
    backgroundData = data;
    return null;
  }

  const panelAlpha = $derived(backgroundData ? appearance.panel_opacity : 100);
  const terminalOpacity = $derived(backgroundData ? appearance.terminal_opacity : 100);

  async function lock() {
    // Without this, ssh.exe processes would be left open after unmounting
    // the Workspace.
    await invoke("session_close_all");
    await invoke("vault_lock");
    unlocked = false;
  }
</script>

<div
  class="window-shell"
  data-theme={theme}
  style:--panel-alpha="{panelAlpha}%"
  style:--raised-alpha="{Math.max(panelAlpha, 85)}%"
>
  <!-- Solid base plus the optional image; the interface sits above and may be translucent. -->
  <div class="bg-layer" aria-hidden="true" style:background-image={backgroundData ? `url("${backgroundData}")` : "none"}></div>
  <div class="shell-content">
    <TitleBar
      {theme}
      {dir}
      {unlocked}
      onchange={changeTheme}
      onsettings={() => (settingsOpen = true)}
      ontunnels={() => (tunnelPanelOpen = true)}
      onlock={lock}
    />
    {#if ready}
      <div class="screen">
        {#if unlocked}
          <Workspace
            {dir}
            {theme}
            {appearance}
            {terminalOpacity}
            onthemechange={changeTheme}
            onappearancechange={changeAppearance}
            bind:settingsOpen
            bind:tunnelPanelOpen
          />
        {:else}
          <LockScreen onunlocked={() => (unlocked = true)} />
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .window-shell {
    --c-app-bg: #161a22;
    --c-surface: #1a1f29;
    --c-surface-raised: #1f2530;
    --c-surface-hover: #222a36;
    --c-surface-deep: #171d26;
    --border: #2f3847;
    --border-strong: #3a4557;
    --text: #dfe4ec;
    --text-secondary: #b6bfcd;
    --muted: #8d97a8;
    --accent: #8fb4e8;
    --accent-contrast: #12161d;
    --danger: #f08a7a;
    --danger-bg: #3a2226;
    --warning: #d7a56b;
    --warning-bg: #30291f;
    --warning-border: #59452d;
    --warning-highlight: #6b5320;
    --warning-highlight-text: #ffe6a3;
    --success: #5dcaa5;
    --success-soft: #1d3029;
    --success-border: #315844;
    /* Opacity of the surfaces over the background image (set from settings). */
    --panel-alpha: 100%;
    --raised-alpha: 100%;
    --app-bg: color-mix(in srgb, var(--c-app-bg) var(--panel-alpha), transparent);
    --surface: color-mix(in srgb, var(--c-surface) var(--panel-alpha), transparent);
    --surface-deep: color-mix(in srgb, var(--c-surface-deep) var(--panel-alpha), transparent);
    --surface-raised: color-mix(in srgb, var(--c-surface-raised) var(--raised-alpha), transparent);
    --surface-hover: color-mix(in srgb, var(--c-surface-hover) var(--raised-alpha), transparent);
    position: relative;
    height: 100vh;
    width: 100vw;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--c-app-bg);
    color: var(--text);
    font-family: "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif;
  }
  .window-shell[data-theme="light"] {
    --c-app-bg: #f5f7fb;
    --c-surface: #eaf0f7;
    --c-surface-raised: #ffffff;
    --c-surface-hover: #e2eaf4;
    --c-surface-deep: #edf2f8;
    --border: #cbd5e2;
    --border-strong: #aebcce;
    --text: #253247;
    --text-secondary: #46566d;
    --muted: #6b788c;
    --accent: #356bb3;
    --accent-contrast: #ffffff;
    --danger: #b42336;
    --danger-bg: #fae8ea;
    --warning: #976200;
    --warning-bg: #fff4d6;
    --warning-border: #e2c77b;
    --warning-highlight: #f5e5b4;
    --warning-highlight-text: #684900;
    --success: #237a55;
    --success-soft: #e2f3e9;
    --success-border: #94c9a8;
  }
  .window-shell[data-theme="nord"] {
    --c-app-bg: #2e3440;
    --c-surface: #3b4252;
    --c-surface-raised: #434c5e;
    --c-surface-hover: #4c566a;
    --c-surface-deep: #353c49;
    --border: #596579;
    --border-strong: #68768b;
    --text: #eceff4;
    --text-secondary: #d8dee9;
    --muted: #a3b0c2;
    --accent: #88c0d0;
    --accent-contrast: #2e3440;
    --danger: #bf616a;
    --danger-bg: #49363e;
    --warning: #ebcb8b;
    --warning-bg: #4a4132;
    --warning-border: #756247;
    --warning-highlight: #64563d;
    --warning-highlight-text: #f4dca8;
    --success: #a3be8c;
    --success-soft: #39473e;
    --success-border: #607957;
  }
  /* Catppuccin flavors (https://catppuccin.com/palette): base / mantle / surface0 /
     surface1 / crust fill the five surfaces; accents are the flavor's blue, red,
     yellow and green. */
  .window-shell[data-theme="mocha"] {
    --c-app-bg: #1e1e2e;
    --c-surface: #181825;
    --c-surface-raised: #313244;
    --c-surface-hover: #45475a;
    --c-surface-deep: #11111b;
    --border: #45475a;
    --border-strong: #585b70;
    --text: #cdd6f4;
    --text-secondary: #bac2de;
    --muted: #a6adc8;
    --accent: #89b4fa;
    --accent-contrast: #11111b;
    --danger: #f38ba8;
    --warning: #f9e2af;
    --success: #a6e3a1;
  }
  .window-shell[data-theme="macchiato"] {
    --c-app-bg: #24273a;
    --c-surface: #1e2030;
    --c-surface-raised: #363a4f;
    --c-surface-hover: #494d64;
    --c-surface-deep: #181926;
    --border: #494d64;
    --border-strong: #5b6078;
    --text: #cad3f5;
    --text-secondary: #b8c0e0;
    --muted: #a5adcb;
    --accent: #8aadf4;
    --accent-contrast: #181926;
    --danger: #ed8796;
    --warning: #eed49f;
    --success: #a6da95;
  }
  .window-shell[data-theme="frappe"] {
    --c-app-bg: #303446;
    --c-surface: #292c3c;
    --c-surface-raised: #414559;
    --c-surface-hover: #51576d;
    --c-surface-deep: #232634;
    --border: #51576d;
    --border-strong: #626880;
    --text: #c6d0f5;
    --text-secondary: #b5bfe2;
    --muted: #a5adce;
    --accent: #8caaee;
    --accent-contrast: #232634;
    --danger: #e78284;
    --warning: #e5c890;
    --success: #a6d189;
  }
  /* Latte is the light flavor. Muted, warning and success are a little darker than
     the official swatches so small text keeps at least 4.5:1 contrast. */
  .window-shell[data-theme="latte"] {
    --c-app-bg: #eff1f5;
    --c-surface: #e6e9ef;
    --c-surface-raised: #ffffff;
    --c-surface-hover: #ccd0da;
    --c-surface-deep: #dce0e8;
    --border: #bcc0cc;
    --border-strong: #9ca0b0;
    --text: #4c4f69;
    --text-secondary: #5c5f77;
    --muted: #5f6278;
    --accent: #1e66f5;
    --accent-contrast: #ffffff;
    --danger: #d20f39;
    --warning: #8a5a0a;
    --success: #26741a;
  }
  .window-shell[data-theme="mocha"],
  .window-shell[data-theme="macchiato"],
  .window-shell[data-theme="frappe"],
  .window-shell[data-theme="latte"] {
    --danger-bg: color-mix(in srgb, var(--danger) 16%, var(--c-app-bg));
    --warning-bg: color-mix(in srgb, var(--warning) 14%, var(--c-app-bg));
    --warning-border: color-mix(in srgb, var(--warning) 42%, var(--c-app-bg));
    --warning-highlight: color-mix(in srgb, var(--warning) 32%, var(--c-app-bg));
    --warning-highlight-text: var(--text);
    --success-soft: color-mix(in srgb, var(--success) 14%, var(--c-app-bg));
    --success-border: color-mix(in srgb, var(--success) 42%, var(--c-app-bg));
  }
  .bg-layer {
    position: absolute;
    inset: 0;
    z-index: 0;
    background-color: var(--c-app-bg);
    background-position: center;
    background-size: cover;
    background-repeat: no-repeat;
    pointer-events: none;
  }
  .shell-content {
    position: relative;
    z-index: 1;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .screen {
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }
  :global(html, body) {
    width: 100%;
    height: 100%;
    margin: 0;
    overflow: hidden;
    background: transparent;
  }
  /* WebView2 uses Chromium scrollbars; keep them slim and theme-aware. */
  :global(::-webkit-scrollbar) {
    width: 6px;
    height: 6px;
  }
  :global(::-webkit-scrollbar-track) {
    background: transparent;
  }
  :global(::-webkit-scrollbar-thumb) {
    background: var(--border-strong);
    border-radius: 3px;
  }
  :global(::-webkit-scrollbar-thumb:hover) {
    background: var(--muted);
  }
  :global(::-webkit-scrollbar-thumb:active) {
    background: var(--accent);
  }
  :global(::-webkit-scrollbar-button) {
    display: none;
    width: 0;
    height: 0;
  }
  :global(::-webkit-scrollbar-corner) {
    background: transparent;
  }
</style>
