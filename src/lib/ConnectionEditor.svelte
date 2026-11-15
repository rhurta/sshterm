<script lang="ts">
  import { untrack } from "svelte";
  import { t } from "$lib/i18n/index.svelte";
  import type { Auth, Connection } from "$lib/types";

  let {
    conn,
    isNew,
    onsave,
    oncancel,
  }: {
    conn: Connection;
    isNew: boolean;
    onsave: (c: Connection) => void;
    oncancel: () => void;
  } = $props();

  type Section = "general" | "auth" | "advanced";

  // Snapshot the props once: the editor is a modal and its connection never
  // changes while it is open. `untrack` also keeps the initial state clean.
  const initial = untrack(() => ({ ...conn }));
  let form = $state(initial);
  let section = $state<Section>("general");
  let authType = $state<Auth["type"]>(initial.auth.type);
  let password = $state(initial.auth.type === "password" ? initial.auth.password : "");
  let keyPath = $state(initial.auth.type === "key" ? initial.auth.path : "");
  let passphrase = $state(
    initial.auth.type === "key" ? (initial.auth.passphrase ?? "") : "",
  );
  let validation = $state("");

  function submit(event: SubmitEvent) {
    event.preventDefault();
    const port = Number(form.port);
    const keepalive = Number(form.keepalive_secs);
    if (
      !form.host.trim() ||
      !form.user.trim() ||
      !Number.isInteger(port) ||
      port < 1 ||
      port > 65535
    ) {
      validation = t("editor.validation");
      return;
    }
    const auth: Auth =
      authType === "password"
        ? { type: "password", password }
        : authType === "key"
          ? { type: "key", path: keyPath, passphrase: passphrase || null }
          : { type: "agent" };
    validation = "";
    onsave({ ...form, port, keepalive_secs: keepalive, auth });
  }
</script>

<div class="backdrop">
  <form onsubmit={submit}>
    <h2>{isNew ? t("editor.title.new") : t("editor.title.edit")}</h2>

    <div class="sections">
      <button type="button" class:current={section === "general"} onclick={() => (section = "general")}>
        {t("editor.section.general")}
      </button>
      <button type="button" class:current={section === "auth"} onclick={() => (section = "auth")}>
        {t("editor.section.auth")}
      </button>
      <button type="button" class:current={section === "advanced"} onclick={() => (section = "advanced")}>
        {t("editor.section.advanced")}
      </button>
    </div>

    {#if section === "general"}
      <label>
        {t("editor.name")}
        <input bind:value={form.name} autocomplete="off" />
      </label>
      <label>
        {t("editor.host")}
        <input bind:value={form.host} required autocomplete="off" />
      </label>
      <label>
        {t("editor.port")}
        <input type="number" min="1" max="65535" bind:value={form.port} required />
      </label>
      <label>
        {t("editor.user")}
        <input bind:value={form.user} required autocomplete="off" />
      </label>
    {:else if section === "auth"}
      <label>
        {t("editor.authType")}
        <select bind:value={authType}>
          <option value="password">{t("editor.auth.password")}</option>
          <option value="key">{t("editor.auth.key")}</option>
          <option value="agent">{t("editor.auth.agent")}</option>
        </select>
      </label>
      {#if authType === "password"}
        <label>
          {t("editor.password")}
          <input type="password" bind:value={password} autocomplete="new-password" />
        </label>
        <p class="hint">{t("editor.passwordHint")}</p>
      {:else if authType === "key"}
        <label>
          {t("editor.keyPath")}
          <input bind:value={keyPath} autocomplete="off" spellcheck="false" />
        </label>
        <label>
          {t("editor.passphrase")}
          <input type="password" bind:value={passphrase} autocomplete="new-password" />
        </label>
      {:else}
        <p class="hint">{t("editor.auth.agent")}</p>
      {/if}
    {:else}
      <label>
        {t("editor.keepalive")}
        <input type="number" min="1" max="3600" bind:value={form.keepalive_secs} />
      </label>
      <label class="check">
        <input type="checkbox" bind:checked={form.protected} />
        {t("editor.protected")}
      </label>
    {/if}

    {#if validation}
      <p class="validation" role="alert">{validation}</p>
    {/if}

    <div class="buttons">
      <button type="submit">{t("editor.save")}</button>
      <button type="button" class="ghost" onclick={oncancel}>{t("editor.cancel")}</button>
    </div>
  </form>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(8, 11, 16, 0.7);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 20;
  }
  form {
    width: min(27rem, 92vw);
    max-height: 90vh;
    overflow-y: auto;
    background: #1f2530;
    border: 1px solid #2f3847;
    border-radius: 8px;
    padding: 1.35rem;
    display: flex;
    flex-direction: column;
    gap: 0.7rem;
    box-sizing: border-box;
  }
  h2 {
    margin: 0;
    font-size: 1.1rem;
    font-weight: 500;
  }
  .sections {
    display: flex;
    gap: 0.35rem;
    border-bottom: 1px solid #2f3847;
    padding-bottom: 0.5rem;
  }
  .sections button {
    font: inherit;
    font-size: 0.78rem;
    color: #b6bfcd;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 4px;
    padding: 0.3rem 0.55rem;
    cursor: pointer;
  }
  .sections button.current {
    color: #dfe4ec;
    background: #222a36;
    border-color: #8fb4e8;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    font-size: 0.82rem;
    color: #b6bfcd;
  }
  label.check {
    flex-direction: row;
    align-items: center;
    gap: 0.45rem;
  }
  input,
  select {
    font: inherit;
    font-size: 0.82rem;
    color: #dfe4ec;
    background: #161a22;
    border: 1px solid #2f3847;
    border-radius: 4px;
    padding: 0.45rem 0.55rem;
  }
  .hint {
    margin: 0;
    font-size: 0.75rem;
    color: #8d97a8;
  }
  .validation {
    margin: 0;
    font-size: 0.78rem;
    color: #f08a7a;
  }
  .buttons {
    display: flex;
    gap: 0.5rem;
    margin-top: 0.4rem;
  }
  .buttons button {
    font: inherit;
    font-size: 0.8rem;
    font-weight: 600;
    color: #12161d;
    background: #8fb4e8;
    border: 0;
    border-radius: 4px;
    padding: 0.5rem 0.9rem;
    cursor: pointer;
  }
  .buttons button.ghost {
    background: transparent;
    color: #b6bfcd;
    border: 1px solid #2f3847;
  }
</style>
