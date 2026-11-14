<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { hasMessage, t, type MessageKey } from "$lib/i18n/index.svelte";

  type Status = "loading" | "missing" | "locked" | "unlocked";
  type Problem = { key: MessageKey; params?: Record<string, string> };

  let { onunlocked }: { onunlocked: () => void } = $props();

  let status = $state<Status>("loading");
  let password = $state("");
  let confirm = $state("");
  // Se guarda la clave del mensaje, no el texto: así cambia al cambiar de idioma.
  let problem = $state<Problem | null>(null);
  let busy = $state(false);
  let field: HTMLInputElement | undefined = $state();

  async function refresh() {
    status = await invoke<Status>("vault_status");
    if (status === "unlocked") onunlocked();
  }
  refresh();

  $effect(() => {
    if (status === "missing" || status === "locked") field?.focus();
  });

  // El backend devuelve códigos ("wrong_password"...). Aquí se convierten en mensajes.
  function toProblem(code: string): Problem {
    const key = `error.${code}`;
    return hasMessage(key)
      ? { key }
      : { key: "lock.error.unexpected", params: { detail: code } };
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    problem = null;
    if (status === "missing" && password !== confirm) {
      problem = { key: "lock.error.mismatch" };
      return;
    }
    busy = true;
    try {
      await invoke(status === "missing" ? "vault_create" : "vault_unlock", { password });
      password = "";
      confirm = "";
      onunlocked();
    } catch (e) {
      problem = toProblem(String(e));
    } finally {
      busy = false;
    }
  }
</script>

<main class="lock">
  {#if status !== "loading"}
    <form onsubmit={submit}>
      <p class="mark">sshterm</p>

      {#if status === "missing"}
        <h1>{t("lock.title.create")}</h1>
        <p class="hint">{t("lock.hint.create")}</p>
      {:else}
        <h1>{t("lock.title.unlock")}</h1>
      {/if}

      <label>
        {t("lock.field.password")}
        <input
          type="password"
          bind:value={password}
          bind:this={field}
          autocomplete="off"
          disabled={busy}
        />
      </label>

      {#if status === "missing"}
        <label>
          {t("lock.field.confirm")}
          <input type="password" bind:value={confirm} autocomplete="off" disabled={busy} />
        </label>
      {/if}

      {#if problem}
        <p class="error" role="alert">{t(problem.key, problem.params)}</p>
      {/if}

      <button type="submit" disabled={busy}>
        {busy
          ? t("lock.button.busy")
          : status === "missing"
            ? t("lock.button.create")
            : t("lock.button.unlock")}
      </button>
    </form>
  {/if}
</main>

<style>
  .lock {
    min-height: 100vh;
    display: flex;
    align-items: center;
    padding: 0 max(2rem, 8vw);
    box-sizing: border-box;
  }
  form {
    width: min(22rem, 100%);
    display: flex;
    flex-direction: column;
    gap: 1rem;
    border-left: 2px solid #8fb4e8;
    padding-left: 1.5rem;
  }
  .mark {
    margin: 0;
    font-size: 0.95rem;
    font-weight: 600;
    letter-spacing: 0.02em;
    color: #8fb4e8;
  }
  h1 {
    margin: 0;
    font-size: 1.7rem;
    font-weight: 600;
    line-height: 1.2;
  }
  .hint {
    margin: 0;
    color: #8d97a8;
    line-height: 1.5;
    font-size: 0.92rem;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    font-size: 0.9rem;
    color: #b6bfcd;
  }
  input {
    font: inherit;
    color: #dfe4ec;
    background: #1f2530;
    border: 1px solid #2f3847;
    border-radius: 4px;
    padding: 0.6rem 0.7rem;
  }
  input:focus-visible,
  button:focus-visible {
    outline: 2px solid #8fb4e8;
    outline-offset: 2px;
  }
  button {
    font: inherit;
    font-weight: 600;
    color: #12161d;
    background: #8fb4e8;
    border: 0;
    border-radius: 4px;
    padding: 0.65rem 1rem;
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.6;
    cursor: progress;
  }
  .error {
    margin: 0;
    color: #f08a7a;
    font-size: 0.9rem;
  }
</style>
