<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import LockScreen from "$lib/LockScreen.svelte";
  import Workspace from "$lib/Workspace.svelte";
  import TitleBar from "$lib/TitleBar.svelte";
  import { initLocale } from "$lib/i18n/index.svelte";
  import { startSessionBus } from "$lib/sessionBus";
  import type { AppTheme } from "$lib/theme";

  // Register before any session can be opened, so no early PTY output is lost.
  startSessionBus();

  const DEFAULT_THEME: AppTheme = "dark";
  let ready = $state(false); // avoid briefly showing the wrong locale or theme
  let unlocked = $state(false);
  let settingsOpen = $state(false);
  let dir = $state("");
  let theme = $state<AppTheme>(DEFAULT_THEME);

  Promise.all([
    initLocale(),
    invoke<AppTheme>("config_get_theme").catch(() => DEFAULT_THEME),
  ])
    .then(([, savedTheme]) => {
      theme = savedTheme;
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

  async function lock() {
    // Without this, ssh.exe processes would be left open after unmounting
    // the Workspace.
    await invoke("session_close_all");
    await invoke("vault_lock");
    unlocked = false;
  }
</script>

<div class="window-shell" data-theme={theme}>
  <TitleBar
    {theme}
    {dir}
    {unlocked}
    onchange={changeTheme}
    onsettings={() => (settingsOpen = true)}
    onlock={lock}
  />
  {#if ready}
    <div class="screen">
      {#if unlocked}
        <Workspace {dir} {theme} onlock={lock} bind:settingsOpen />
      {:else}
        <LockScreen onunlocked={() => (unlocked = true)} />
      {/if}
    </div>
  {/if}
</div>

<style>
  .window-shell {
    --app-bg: #161a22;
    --surface: #1a1f29;
    --surface-raised: #1f2530;
    --surface-hover: #222a36;
    --surface-deep: #171d26;
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
    height: 100vh;
    width: 100vw;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--app-bg);
    color: var(--text);
    font-family: "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif;
  }
  .window-shell[data-theme="light"] {
    --app-bg: #f5f7fb;
    --surface: #eaf0f7;
    --surface-raised: #ffffff;
    --surface-hover: #e2eaf4;
    --surface-deep: #edf2f8;
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
    --app-bg: #2e3440;
    --surface: #3b4252;
    --surface-raised: #434c5e;
    --surface-hover: #4c566a;
    --surface-deep: #353c49;
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
