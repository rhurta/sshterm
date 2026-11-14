<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import LockScreen from "$lib/LockScreen.svelte";
  import LanguageSelect from "$lib/LanguageSelect.svelte";
  import { initLocale, t } from "$lib/i18n/index.svelte";

  let ready = $state(false); // evita mostrar un instante el idioma equivocado
  let unlocked = $state(false);
  let dir = $state("");

  initLocale().finally(() => (ready = true));
  invoke<string>("app_info").then((d) => (dir = d));

  async function lock() {
    await invoke("vault_lock");
    unlocked = false;
  }
</script>

{#if ready}
  <LanguageSelect />
  {#if unlocked}
    <main class="home">
      <h1>sshterm</h1>
      <p>{t("home.unlocked", { dir })}</p>
      <button onclick={lock}>{t("home.lock")}</button>
    </main>
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
  .home {
    padding: 2rem max(2rem, 8vw);
  }
  .home button {
    font: inherit;
    font-weight: 600;
    color: #12161d;
    background: #8fb4e8;
    border: 0;
    border-radius: 4px;
    padding: 0.6rem 1rem;
    cursor: pointer;
  }
</style>
