<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { hasMessage, t } from "$lib/i18n/index.svelte";
  import type { Auth, Connection, ConnectionSummary } from "$lib/types";

  let {
    conn,
    isNew,
    onsave,
    oncancel,
  }: {
    conn: Connection;
    isNew: boolean;
    onsave: (c: Connection) => Promise<string | null>;
    oncancel: () => void;
  } = $props();

  type Section = "general" | "auth" | "advanced" | "jumps";

  // Snapshot the props once: the editor is a modal and its connection never
  // changes while it is open. Clone the jump list because it is edited in-place.
  const initial = untrack(() => ({ ...conn, jump: [...(conn.jump ?? [])] }));
  let form = $state(initial);
  let section = $state<Section>("general");
  let connections = $state<ConnectionSummary[]>([]);
  let connectionsError = $state("");
  let jumpCandidates = $derived(
    connections.filter((candidate) => candidate.id !== form.id && !form.jump.includes(candidate.id)),
  );

  onMount(() => {
    void invoke<ConnectionSummary[]>("conn_list")
      .then((items) => (connections = items))
      .catch((error) => (connectionsError = errorText(String(error))));
  });

  function addJump(id: string) {
    if (id !== form.id && !form.jump.includes(id)) form.jump = [...form.jump, id];
  }

  function moveJump(index: number, delta: number) {
    const next = [...form.jump];
    const target = index + delta;
    if (target < 0 || target >= next.length) return;
    [next[index], next[target]] = [next[target], next[index]];
    form.jump = next;
  }

  function removeJump(id: string) {
    form.jump = form.jump.filter((item) => item !== id);
  }

  function jumpName(id: string): string {
    return connections.find((candidate) => candidate.id === id)?.name ?? id;
  }
  let authType = $state<Auth["type"]>(initial.auth.type);
  let password = $state(initial.auth.type === "password" ? initial.auth.password : "");
  let keyPath = $state(initial.auth.type === "key" ? initial.auth.path : "");
  let passphrase = $state(
    initial.auth.type === "key" ? (initial.auth.passphrase ?? "") : "",
  );
  let validation = $state("");
  let saving = $state(false);

  function errorText(code: string): string {
    const key = `error.${code}`;
    return hasMessage(key) ? t(key) : t("lock.error.unexpected", { detail: code });
  }

  function jumpWouldCycle(): boolean {
    const byId = new Map(connections.map((candidate) => [candidate.id, candidate]));
    function reachesRoot(id: string, seen: Set<string>): boolean {
      if (id === form.id || seen.has(id)) return true;
      const hop = byId.get(id);
      if (!hop) return false;
      const next = new Set(seen);
      next.add(id);
      return (hop.jump ?? []).some((nested) => reachesRoot(nested, next));
    }
    return form.jump.some((id) => reachesRoot(id, new Set()));
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (saving) return;
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
    if (jumpWouldCycle()) {
      validation = t("editor.jumpCycleValidation");
      section = "jumps";
      return;
    }
    const auth: Auth =
      authType === "password"
        ? { type: "password", password }
        : authType === "key"
          ? { type: "key", path: keyPath, passphrase: passphrase || null }
          : { type: "agent" };
    validation = "";
    saving = true;
    const failure = await onsave({ ...form, port, keepalive_secs: keepalive, auth });
    saving = false;
    if (failure) {
      validation = errorText(failure);
      if (failure.startsWith("jump_")) section = "jumps";
    }
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
      <button type="button" class:current={section === "jumps"} onclick={() => (section = "jumps")}>
        {t("editor.section.jumps")}
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
    {:else if section === "advanced"}
      <label>
        {t("editor.keepalive")}
        <input type="number" min="1" max="3600" bind:value={form.keepalive_secs} />
      </label>
      <label class="check">
        <input type="checkbox" bind:checked={form.protected} />
        {t("editor.protected")}
      </label>
    {:else}
      <p class="hint">{t("editor.jumpHint")}</p>
      <p class="hint">{t("editor.jumpAuthHint")}</p>
      {#if form.jump.length > 0}
        <ol class="jump-chain">
          {#each form.jump as id, index (id)}
            <li>
              <div class="jump-info">
                <span class="jump-name">{index + 1}. {jumpName(id)}</span>
                {#if connections.find((candidate) => candidate.id === id)}
                  {@const hop = connections.find((candidate) => candidate.id === id)!}
                  <span class="hint">{hop.user}@{hop.host}:{hop.port}</span>
                {:else}
                  <span class="validation">{t("editor.jumpMissing")}</span>
                {/if}
              </div>
              <button type="button" class="icon-button" disabled={index === 0} aria-label={t("editor.jump.moveUp")} onclick={() => moveJump(index, -1)}>↑</button>
              <button type="button" class="icon-button" disabled={index === form.jump.length - 1} aria-label={t("editor.jump.moveDown")} onclick={() => moveJump(index, 1)}>↓</button>
              <button type="button" class="icon-button remove" aria-label={t("editor.jump.remove")} onclick={() => removeJump(id)}>×</button>
            </li>
          {/each}
        </ol>
      {:else}
        <p class="hint">{t("editor.jumpNone")}</p>
      {/if}
      <h3 class="jump-heading">{t("editor.jumpAvailable")}</h3>
      <div class="jump-candidates">
        {#each jumpCandidates as candidate (candidate.id)}
          <button type="button" class="jump-candidate" onclick={() => addJump(candidate.id)}>
            <span>{candidate.name}</span>
            <small>{candidate.user}@{candidate.host}:{candidate.port}</small>
          </button>
        {:else}
          <p class="hint">{t("editor.jumpNoCandidates")}</p>
        {/each}
      </div>
      {#if connectionsError}
        <p class="validation" role="alert">{connectionsError}</p>
      {/if}
    {/if}

    {#if validation}
      <p class="validation" role="alert">{validation}</p>
    {/if}

    <div class="buttons">
      <button type="submit" disabled={saving}>{t("editor.save")}</button>
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
    flex-wrap: wrap;
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
  .jump-heading {
    margin: 0.35rem 0 0;
    font-size: 0.8rem;
    font-weight: 500;
    color: #b6bfcd;
  }
  .jump-chain {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }
  .jump-chain li {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0.35rem;
    border: 1px solid #2f3847;
    border-radius: 4px;
    background: #171d26;
  }
  .jump-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
  }
  .jump-name {
    color: #dfe4ec;
    font-size: 0.8rem;
  }
  .icon-button {
    flex: 0 0 auto;
    padding: 0.2rem 0.4rem !important;
    color: #b6bfcd !important;
    background: #222a36 !important;
    border: 1px solid #2f3847 !important;
  }
  .icon-button:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .icon-button.remove {
    color: #f08a7a !important;
  }
  .jump-candidates {
    max-height: 10rem;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }
  .jump-candidate {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.1rem;
    text-align: left;
    color: #dfe4ec !important;
    background: #222a36 !important;
    border: 1px solid #2f3847 !important;
  }
  .jump-candidate small {
    color: #8d97a8;
    font-weight: 400;
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
