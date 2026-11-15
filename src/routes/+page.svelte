<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import LockScreen from "$lib/LockScreen.svelte";
  import Workspace from "$lib/Workspace.svelte";
  import { initLocale } from "$lib/i18n/index.svelte";
  import { startSessionBus } from "$lib/sessionBus";

  // Register before any session can be opened, so no early PTY output is lost.
  startSessionBus();

  let ready = $state(false); // avoid briefly showing the wrong locale
  let unlocked = $state(false);
  let dir = $state("");

  initLocale().finally(() => (ready = true));
  invoke<string>("app_info").then((d) => (dir = d));

  async function lock() {
    // Without this, ssh.exe processes would be left open after unmounting
    // the Workspace.
    await invoke("session_close_all");
    await invoke("vault_lock");
    unlocked = false;
  }
</script>

{#if ready}
  {#if unlocked}
    <Workspace {dir} onlock={lock} />
  {:else}
    <LockScreen onunlocked={() => (unlocked = true)} />
  {/if}
{/if}

<style>
  :global(html, body) {
    margin: 0;
    background: #161a22;
    color: #dfe4ec;
    font-family: "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif;
  }

  /* WebView2 is Chromium-based. Styling these pseudo-elements replaces the
     thick light Windows scrollbar with a slim one that matches the dark theme.
     `scrollbar-width` / `scrollbar-color` are intentionally left at `auto`:
     setting them makes Chromium ignore the rules below. */
  :global(::-webkit-scrollbar) {
    width: 6px;
    height: 6px;
  }
  :global(::-webkit-scrollbar-track) {
    background: transparent;
  }
  :global(::-webkit-scrollbar-thumb) {
    background: #3a4557;
    border-radius: 3px;
  }
  :global(::-webkit-scrollbar-thumb:hover) {
    background: #52627c;
  }
  :global(::-webkit-scrollbar-thumb:active) {
    background: #8fb4e8;
  }
  /* Hide the native arrow buttons and the bottom-right corner patch. */
  :global(::-webkit-scrollbar-button) {
    display: none;
    width: 0;
    height: 0;
  }
  :global(::-webkit-scrollbar-corner) {
    background: transparent;
  }
</style>
