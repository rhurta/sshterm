<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { hasMessage, t, type MessageKey } from "$lib/i18n/index.svelte";
  import LanguageSelect from "$lib/LanguageSelect.svelte";
  import PasswordInput from "$lib/PasswordInput.svelte";

  type Status = "loading" | "missing" | "locked" | "unlocked";
  type Problem = { key: MessageKey; params?: Record<string, string> };

  let { onunlocked }: { onunlocked: () => void } = $props();

  let status = $state<Status>("loading");
  let password = $state("");
  let confirm = $state("");
  // The message key is stored, not the text, so it updates on locale change.
  let problem = $state<Problem | null>(null);
  let busy = $state(false);
  let field: { focus: () => void } | undefined = $state();

  async function refresh() {
    status = await invoke<Status>("vault_status");
    if (status === "unlocked") onunlocked();
  }
  refresh();

  $effect(() => {
    if (status === "missing" || status === "locked") field?.focus();
  });

  // The backend returns codes ("wrong_password"...). Here they are converted
  // to messages.
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
  <div class="lang-corner">
    <LanguageSelect />
  </div>
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
        <PasswordInput bind:value={password} bind:this={field} disabled={busy} />
      </label>

      {#if status === "missing"}
        <label>
          {t("lock.field.confirm")}
          <PasswordInput bind:value={confirm} disabled={busy} />
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
    height: 100%;
    min-height: 0;
    display: flex;
    align-items: center;
    padding: 0 max(2rem, 8vw);
    box-sizing: border-box;
    position: relative;
  }
  .lang-corner {
    position: absolute;
    top: 1rem;
    right: 1.25rem;
  }
  form {
    width: min(22rem, 100%);
    display: flex;
    flex-direction: column;
    gap: 1rem;
    border-left: 2px solid var(--accent);
    padding-left: 1.5rem;
  }
  .mark {
    margin: 0;
    font-size: 0.95rem;
    font-weight: 600;
    letter-spacing: 0.02em;
    color: var(--accent);
  }
  h1 {
    margin: 0;
    font-size: 1.7rem;
    font-weight: 600;
    line-height: 1.2;
  }
  .hint {
    margin: 0;
    color: var(--muted);
    line-height: 1.5;
    font-size: 0.92rem;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    font-size: 0.9rem;
    color: var(--text-secondary);
  }
  label {
    --field-bg: var(--surface-raised);
    --field-pad: 0.6rem 0.7rem;
  }
  button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  button {
    font: inherit;
    font-weight: 600;
    color: var(--accent-contrast);
    background: var(--accent);
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
    color: var(--danger);
    font-size: 0.9rem;
  }
</style>
