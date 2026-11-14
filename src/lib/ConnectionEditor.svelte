<script lang="ts">
  import { untrack } from "svelte";
  import { t } from "$lib/i18n/index.svelte";
  import type { Connection } from "$lib/types";

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

  // Local copy: edits don't touch the list until saved.
  // The password is normalized to a string so it can be bound to the input.
  let form = $state(untrack(() => ({ ...conn, password: conn.password ?? "" })));

  function submit(event: SubmitEvent) {
    event.preventDefault();
    onsave({ ...form, port: Number(form.port), keepalive_secs: Number(form.keepalive_secs) });
  }
</script>

<div class="backdrop">
  <form onsubmit={submit}>
    <h2>{isNew ? t("editor.title.new") : t("editor.title.edit")}</h2>

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
    <label>
      {t("editor.password")}
      <input type="password" bind:value={form.password} autocomplete="new-password" />
    </label>
    <label>
      {t("editor.keepalive")}
      <input type="number" min="1" max="3600" bind:value={form.keepalive_secs} />
    </label>

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
    width: min(26rem, 92vw);
    max-height: 90vh;
    overflow-y: auto;
    background: #1f2530;
    border: 1px solid #2f3847;
    border-radius: 8px;
    padding: 1.5rem;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    box-sizing: border-box;
  }
  h2 {
    margin: 0 0 0.25rem;
    font-size: 1.15rem;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    font-size: 0.82rem;
    color: #b6bfcd;
  }
  input {
    font: inherit;
    color: #dfe4ec;
    background: #161a22;
    border: 1px solid #2f3847;
    border-radius: 4px;
    padding: 0.5rem 0.6rem;
  }
  .buttons {
    display: flex;
    gap: 0.5rem;
    margin-top: 0.5rem;
  }
  button {
    font: inherit;
    font-weight: 600;
    color: #12161d;
    background: #8fb4e8;
    border: 0;
    border-radius: 4px;
    padding: 0.55rem 1rem;
    cursor: pointer;
  }
  button.ghost {
    background: transparent;
    color: #b6bfcd;
    border: 1px solid #2f3847;
  }
</style>
