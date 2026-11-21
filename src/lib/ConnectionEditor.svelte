<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { hasMessage, t } from "$lib/i18n/index.svelte";
  import PasswordInput from "$lib/PasswordInput.svelte";
  import type { Auth, Connection, ConnectionSummary, HighlightRuleSetStatus, Tunnel } from "$lib/types";

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

  type Section = "general" | "auth" | "advanced" | "jumps" | "tunnels" | "highlight";

  // Snapshot the props once: clone mutable M5/M7 arrays before editing.
  const initial = untrack(() => ({
    ...conn,
    jump: [...(conn.jump ?? [])],
    jump_disabled: [...(conn.jump_disabled ?? [])],
    tunnels: (conn.tunnels ?? []).map((tunnel) => ({ ...tunnel, enabled: tunnel.enabled ?? true })),
  }));
  let form = $state(initial);
  let section = $state<Section>("general");
  let connections = $state<ConnectionSummary[]>([]);
  let connectionsError = $state("");
  let highlightRuleSets = $state<HighlightRuleSetStatus[]>([]);
  let highlightQuery = $state("");
  let highlightSearchInput = $state<HTMLInputElement | null>(null);
  let filteredHighlightRuleSets = $derived.by(() => {
    const query = highlightQuery.trim().toLocaleLowerCase();
    return highlightRuleSets.filter((item) => !query || item.name.toLocaleLowerCase().includes(query) || item.file.toLocaleLowerCase().includes(query));
  });
  let jumpQuery = $state("");
  let jumpCandidates = $derived.by(() => {
    const query = jumpQuery.trim().toLocaleLowerCase();
    return connections.filter((candidate) => {
      if (candidate.id === form.id || form.jump.includes(candidate.id)) return false;
      if (!query) return true;
      return [candidate.name, candidate.host, candidate.user]
        .some((value) => value.toLocaleLowerCase().includes(query));
    });
  });

  onMount(() => {
    void invoke<ConnectionSummary[]>("conn_list")
      .then((items) => (connections = items))
      .catch((error) => (connectionsError = errorText(String(error))));
    void invoke<HighlightRuleSetStatus[]>("highlight_list")
      .then((items) => (highlightRuleSets = items))
      .catch((error) => (connectionsError = errorText(String(error))));
  });

  async function openHighlightSection() {
    section = "highlight";
    await tick();
    highlightSearchInput?.focus();
  }

  function toggleHighlightRuleSet(name: string) {
    form.highlight = form.highlight.includes(name)
      ? form.highlight.filter((item) => item !== name)
      : [...form.highlight, name];
  }

  function addJump(id: string) {
    if (id !== form.id && !form.jump.includes(id)) form.jump = [...form.jump, id];
    form.jump_disabled = form.jump_disabled.filter((item) => item !== id);
  }

  function isJumpEnabled(id: string): boolean {
    return !form.jump_disabled.includes(id);
  }

  function toggleJump(id: string) {
    form.jump_disabled = isJumpEnabled(id)
      ? [...form.jump_disabled, id]
      : form.jump_disabled.filter((item) => item !== id);
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
    form.jump_disabled = form.jump_disabled.filter((item) => item !== id);
  }

  function jumpName(id: string): string {
    return connections.find((candidate) => candidate.id === id)?.name ?? id;
  }

  let newTunnelKind = $state<"local" | "remote" | "dynamic">("local");
  let tunnelBind = $state("127.0.0.1:15432");
  let tunnelDestination = $state("127.0.0.1:5432");

  /** Suggested listen/destination endpoints for each forward type. */
  function tunnelDefaults(kind: "local" | "remote" | "dynamic") {
    if (kind === "dynamic") return { bind: "127.0.0.1:1080", dest: "" };
    if (kind === "remote") return { bind: "127.0.0.1:18080", dest: "127.0.0.1:8080" };
    return { bind: "127.0.0.1:15432", dest: "127.0.0.1:5432" };
  }

  function resetTunnelDefaults() {
    const defaults = tunnelDefaults(newTunnelKind);
    tunnelBind = defaults.bind;
    tunnelDestination = defaults.dest;
  }

  function addTunnel() {
    const dynamic = newTunnelKind === "dynamic";
    if (!tunnelBind.trim() || (!dynamic && !tunnelDestination.trim())) {
      validation = dynamic ? t("editor.tunnelRequiredDynamic") : t("editor.tunnelRequired");
      return;
    }
    const tunnel: Tunnel = dynamic
      ? { kind: "dynamic", bind: tunnelBind.trim(), enabled: true }
      : { kind: newTunnelKind as "local" | "remote", bind: tunnelBind.trim(), dest: tunnelDestination.trim(), enabled: true };
    form.tunnels = [...form.tunnels, tunnel];
    resetTunnelDefaults();
    validation = "";
  }

  function tunnelHint(kind: "local" | "remote" | "dynamic"): string {
    if (kind === "dynamic") return t("editor.tunnel.dynamicHint");
    return kind === "local" ? t("editor.tunnel.localHint") : t("editor.tunnel.remoteHint");
  }

  function removeTunnel(index: number) {
    form.tunnels = form.tunnels.filter((_, i) => i !== index);
  }

  function toggleTunnel(index: number) {
    form.tunnels = form.tunnels.map((tunnel, i) =>
      i === index ? { ...tunnel, enabled: !tunnel.enabled } : tunnel,
    );
  }

  function tunnelName(tunnel: Tunnel): string {
    if (tunnel.kind === "local") return t("editor.tunnel.local");
    if (tunnel.kind === "remote") return t("editor.tunnel.remote");
    return t("editor.tunnel.dynamic");
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
      return (hop.jump ?? [])
        .filter((nested) => !(hop.jump_disabled ?? []).includes(nested))
        .some((nested) => reachesRoot(nested, next));
    }
    return form.jump
      .filter((id) => isJumpEnabled(id))
      .some((id) => reachesRoot(id, new Set()));
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
      if (failure.startsWith("tunnel_")) section = "tunnels";
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
      <button type="button" class:current={section === "tunnels"} onclick={() => (section = "tunnels")}>
        {t("editor.section.tunnels")}
      </button>
      <button type="button" class:current={section === "highlight"} onclick={openHighlightSection}>
        {t("editor.section.highlight")}
      </button>
    </div>

    <div class="body">
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
          <PasswordInput bind:value={password} autocomplete="new-password" />
        </label>
        <p class="hint">{t("editor.passwordHint")}</p>
      {:else if authType === "key"}
        <label>
          {t("editor.keyPath")}
          <input bind:value={keyPath} autocomplete="off" spellcheck="false" />
        </label>
        <label>
          {t("editor.passphrase")}
          <PasswordInput bind:value={passphrase} autocomplete="new-password" />
        </label>
      {:else}
        <p class="hint">{t("editor.auth.agent")}</p>
      {/if}
    {:else if section === "advanced"}
      <label>
        {t("editor.keepalive")}
        <input type="number" min="0" max="3600" bind:value={form.keepalive_secs} />
      </label>
      <label class="check">
        <input type="checkbox" bind:checked={form.protected} />
        {t("editor.protected")}
      </label>
    {:else if section === "jumps"}
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
                  {#if !isJumpEnabled(id)}
                    <span class="state-disabled">{t("editor.disabled")}</span>
                  {/if}
                {:else}
                  <span class="validation">{t("editor.jumpMissing")}</span>
                {/if}
              </div>
              <button
                type="button"
                class:enabled={isJumpEnabled(id)}
                class="toggle-button"
                aria-pressed={isJumpEnabled(id)}
                onclick={() => toggleJump(id)}
              >{isJumpEnabled(id) ? t("editor.disable") : t("editor.enable")}</button>
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
      <input
        class="jump-search"
        bind:value={jumpQuery}
        placeholder={t("editor.jumpSearch")}
        aria-label={t("editor.jumpSearch")}
        autocomplete="off"
      />
      <div class="jump-candidates">
        {#each jumpCandidates as candidate (candidate.id)}
          <button type="button" class="jump-candidate" onclick={() => addJump(candidate.id)}>
            <span>{candidate.name}</span>
            <small>{candidate.user}@{candidate.host}:{candidate.port}</small>
          </button>
        {:else}
          <p class="hint">{jumpQuery.trim() ? t("editor.jumpNoSearchMatches") : t("editor.jumpNoCandidates")}</p>
        {/each}
      </div>
      {#if connectionsError}
        <p class="validation" role="alert">{connectionsError}</p>
      {/if}
    {:else if section === "highlight"}
      <p class="hint">{t("editor.highlightHint")}</p>
      <input
        class="jump-search"
        bind:this={highlightSearchInput}
        bind:value={highlightQuery}
        placeholder={t("editor.highlightSearch")}
        aria-label={t("editor.highlightSearch")}
        autocomplete="off"
      />
      <div class="jump-candidates highlight-candidates">
        {#each filteredHighlightRuleSets as ruleSet (ruleSet.file)}
          <label class="check highlight-choice">
            <input
              type="checkbox"
              checked={form.highlight.includes(ruleSet.name)}
              disabled={!ruleSet.valid}
              onchange={() => toggleHighlightRuleSet(ruleSet.name)}
            />
            <span>{ruleSet.name} ({ruleSet.rule_count})</span>
            {#if !ruleSet.valid}
              <small class="validation">{ruleSet.error}</small>
            {/if}
          </label>
        {:else}
          <p class="hint">{highlightQuery.trim() ? t("editor.highlightNoMatches") : t("editor.highlightNone")}</p>
        {/each}
      </div>
      <p class="hint">{t("editor.highlightOverrideHint")}</p>
      <button type="button" class="add-tunnel" onclick={() => (form.highlight = [])}>{t("editor.highlightInherit")}</button>
    {:else}
      <p class="hint">{t("editor.tunnelHint")}</p>
      {#if form.tunnels.length > 0}
        <ol class="jump-chain">
          {#each form.tunnels as tunnel, index (index)}
            <li>
              <div class="jump-info">
                <span class="jump-name">{tunnelName(tunnel)}</span>
                {#if !tunnel.enabled}
                  <span class="state-disabled">{t("editor.disabled")}</span>
                {/if}
                {#if tunnel.kind === "dynamic"}
                  <span class="hint">{t("editor.tunnel.bind")}: {tunnel.bind}</span>
                {:else}
                  <span class="hint">{t("editor.tunnel.bind")}: {tunnel.bind}</span>
                  <span class="hint">{t("editor.tunnel.destination")}: {tunnel.dest}</span>
                {/if}
              </div>
              <button
                type="button"
                class="toggle-button"
                class:enabled={tunnel.enabled}
                aria-pressed={tunnel.enabled}
                onclick={() => toggleTunnel(index)}
              >{tunnel.enabled ? t("editor.disable") : t("editor.enable")}</button>
              <button type="button" class="icon-button remove" aria-label={t("editor.tunnel.remove")} onclick={() => removeTunnel(index)}>×</button>
            </li>
          {/each}
        </ol>
      {:else}
        <p class="hint">{t("editor.tunnelNone")}</p>
      {/if}

      <h3 class="jump-heading">{t("editor.tunnelAdd")}</h3>
      <label>
        {t("editor.tunnel.type")}
        <select bind:value={newTunnelKind} onchange={resetTunnelDefaults}>
          <option value="local">{t("editor.tunnel.local")} (-L)</option>
          <option value="remote">{t("editor.tunnel.remote")} (-R)</option>
          <option value="dynamic">{t("editor.tunnel.dynamic")} (-D)</option>
        </select>
      </label>
      <label>
        {t("editor.tunnel.bind")}
        <input bind:value={tunnelBind} placeholder="127.0.0.1:15432" autocomplete="off" spellcheck="false" />
      </label>
      {#if newTunnelKind !== "dynamic"}
        <label>
          {t("editor.tunnel.destination")}
          <input bind:value={tunnelDestination} placeholder="127.0.0.1:5432" autocomplete="off" spellcheck="false" />
        </label>
      {/if}
      <p class="hint">{tunnelHint(newTunnelKind)}</p>
      <button type="button" class="add-tunnel" onclick={addTunnel}>{t("editor.tunnel.add")}</button>
    {/if}
    </div>

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
  /* Fixed size, proportional to the window: switching sections never resizes it.
     Only the section content scrolls; title, section tabs and buttons stay put. */
  form {
    width: clamp(380px, 46vw, 820px);
    height: clamp(360px, 72vh, 780px);
    max-width: 96vw;
    max-height: 96vh;
    overflow: hidden;
    background: var(--surface-raised);
    border: 1px solid var(--border);
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
    flex: 0 0 auto;
  }
  .body {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.7rem;
    padding-right: 0.2rem;
  }
  .sections {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
    border-bottom: 1px solid var(--border);
    padding-bottom: 0.5rem;
    flex: 0 0 auto;
  }
  .sections button {
    font: inherit;
    font-size: 0.78rem;
    color: var(--text-secondary);
    background: transparent;
    border: 1px solid transparent;
    border-radius: 4px;
    padding: 0.3rem 0.55rem;
    cursor: pointer;
  }
  .sections button.current {
    color: var(--text);
    background: var(--surface-hover);
    border-color: var(--accent);
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    font-size: 0.82rem;
    color: var(--text-secondary);
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
    color: var(--text);
    background: var(--app-bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.45rem 0.55rem;
  }
  .hint {
    margin: 0;
    font-size: 0.75rem;
    color: var(--muted);
  }
  .jump-heading {
    margin: 0.35rem 0 0;
    font-size: 0.8rem;
    font-weight: 500;
    color: var(--text-secondary);
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
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface-deep);
  }
  .jump-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
  }
  .jump-name {
    color: var(--text);
    font-size: 0.8rem;
  }
  .icon-button {
    flex: 0 0 auto;
    padding: 0.2rem 0.4rem !important;
    color: var(--text-secondary) !important;
    background: var(--surface-hover) !important;
    border: 1px solid var(--border) !important;
  }
  .icon-button:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .icon-button.remove {
    color: var(--danger) !important;
  }
  .jump-search {
    width: 100%;
    box-sizing: border-box;
  }
  .state-disabled {
    color: var(--warning);
    font-size: 0.68rem;
  }
  .toggle-button {
    flex: 0 0 auto;
    padding: 0.25rem 0.45rem;
    color: var(--warning);
    background: var(--warning-bg);
    border: 1px solid var(--warning-border);
    border-radius: 4px;
    font: inherit;
    font-size: 0.68rem;
    cursor: pointer;
  }
  .toggle-button.enabled {
    color: var(--success);
    background: var(--success-soft);
    border-color: var(--success-border);
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
    color: var(--text) !important;
    background: var(--surface-hover) !important;
    border: 1px solid var(--border) !important;
  }
  .jump-candidate small {
    color: var(--muted);
    font-weight: 400;
  }
  .add-tunnel {
    align-self: flex-start;
    font: inherit;
    font-size: 0.8rem;
    color: var(--accent-contrast);
    background: var(--accent);
    border: 0;
    border-radius: 4px;
    padding: 0.45rem 0.8rem;
    cursor: pointer;
  }
  .validation {
    margin: 0;
    font-size: 0.78rem;
    color: var(--danger);
  }
  .buttons {
    display: flex;
    gap: 0.5rem;
    margin-top: 0.4rem;
    flex: 0 0 auto;
  }
  .buttons button {
    font: inherit;
    font-size: 0.8rem;
    font-weight: 600;
    color: var(--accent-contrast);
    background: var(--accent);
    border: 0;
    border-radius: 4px;
    padding: 0.5rem 0.9rem;
    cursor: pointer;
  }
  .buttons button.ghost {
    background: transparent;
    color: var(--text-secondary);
    border: 1px solid var(--border);
  }
</style>
