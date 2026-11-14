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
</style>
