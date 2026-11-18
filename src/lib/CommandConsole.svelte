<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { toBase64 } from "$lib/encoding";
  import { t } from "$lib/i18n/index.svelte";
  import { resolveBroadcastTargets, type BroadcastSession, type BroadcastTargetMode } from "$lib/broadcastTargets";

  let {
    visible,
    sessions,
    activeSessionId,
    height,
  }: {
    visible: boolean;
    sessions: BroadcastSession[];
    activeSessionId: string | null;
    height: number;
  } = $props();

  let draft = $state("");
  let targetMode = $state<BroadcastTargetMode>("active");
  let selectedSessionIds = $state<string[]>([]);
  let includeProtected = $state(false);
  let sessionQuery = $state("");
  let confirmingMultiline = $state(false);
  let sending = $state(false);
  let status = $state("");
  let statusError = $state(false);
  let editor: HTMLTextAreaElement;

  let lineCount = $derived(draft.length ? draft.split(/\r\n|\r|\n/).length : 0);
  let hasMultipleLines = $derived(lineCount > 1);
  let targetResolution = $derived(resolveBroadcastTargets(
    sessions,
    activeSessionId,
    targetMode,
    selectedSessionIds,
    includeProtected,
  ));
  let targetNames = $derived(targetResolution.targets.map((session) => session.name).join(", "));
  let hasProtectedSessions = $derived(sessions.some((session) => session.connected && session.protected));
  let visibleSessions = $derived.by(() => {
    const query = sessionQuery.trim().toLocaleLowerCase();
    return query
      ? sessions.filter((session) => session.name.toLocaleLowerCase().includes(query))
      : sessions;
  });

  function onTargetModeChange(event: Event) {
    targetMode = (event.currentTarget as HTMLSelectElement).value as BroadcastTargetMode;
    if (targetMode === "selected"
      && !sessions.some((session) => session.connected && selectedSessionIds.includes(session.sessionId))
      && activeSessionId
      && sessions.some((session) => session.sessionId === activeSessionId && session.connected)) {
      selectedSessionIds = [activeSessionId];
    }
    sessionQuery = "";
    confirmingMultiline = false;
    status = "";
  }

  function toggleSelected(sessionId: string, checked: boolean) {
    selectedSessionIds = checked
      ? [...new Set([...selectedSessionIds, sessionId])]
      : selectedSessionIds.filter((id) => id !== sessionId);
    confirmingMultiline = false;
    status = "";
  }

  function isTargeted(session: BroadcastSession): boolean {
    if (!session.connected) return false;
    if (targetMode === "active") return session.sessionId === activeSessionId;
    if (session.protected && !includeProtected) return false;
    return targetMode === "all" || selectedSessionIds.includes(session.sessionId);
  }

  function toggleProtected(event: Event) {
    includeProtected = (event.currentTarget as HTMLInputElement).checked;
    confirmingMultiline = false;
    status = "";
  }

  function requestSend() {
    const resolution = resolveBroadcastTargets(
      sessions,
      activeSessionId,
      targetMode,
      selectedSessionIds,
      includeProtected,
    );
    if (resolution.targets.length === 0 || draft.length === 0 || sending) return;
    if ((hasMultipleLines || resolution.targets.length > 1) && !confirmingMultiline) {
      confirmingMultiline = true;
      status = "";
      return;
    }
    void sendDraft();
  }

  async function sendDraft() {
    if (draft.length === 0 || sending) return;
    // Resolve again at dispatch time so closed sessions and changed selections
    // cannot remain in a stale confirmation target set.
    const resolution = resolveBroadcastTargets(
      sessions,
      activeSessionId,
      targetMode,
      selectedSessionIds,
      includeProtected,
    );
    const targets = resolution.targets;
    if (targets.length === 0) {
      confirmingMultiline = false;
      status = t("console.noEligibleSessions");
      statusError = true;
      return;
    }

    sending = true;
    confirmingMultiline = false;
    status = "";
    statusError = false;
    const payload = toBase64(new TextEncoder().encode(draft));
    const accepted: string[] = [];
    const failed: string[] = [];
    try {
      // Sequential dispatch preserves per-session ordering and reports each
      // failure independently. There are deliberately no automatic retries.
      for (const target of targets) {
        try {
          await invoke("session_write", { id: target.sessionId, data: payload });
          accepted.push(target.name);
        } catch (error) {
          failed.push(`${target.name}: ${String(error)}`);
        }
      }

      const summary = t("console.writeAccepted", { accepted: accepted.length, total: targets.length });
      status = failed.length ? `${summary} ${t("console.failed", { details: failed.join(" | ") })}` : summary;
      statusError = failed.length > 0;
    } finally {
      sending = false;
    }
  }

  function onEditorKeydown(event: KeyboardEvent) {
    if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
      event.preventDefault();
      requestSend();
    } else if (event.key === "Escape" && confirmingMultiline) {
      confirmingMultiline = false;
    }
  }

  function clearDraft() {
    draft = "";
    confirmingMultiline = false;
    status = "";
    statusError = false;
    editor?.focus();
  }
</script>

<section class="command-console" class:hidden={!visible} style={`height: ${height}px;`} aria-label={t("console.commandConsole")}>
  <div class="console-heading">
    <strong>{t("console.commandConsole")}</strong>
    {#if confirmingMultiline}
      <div class="confirmation-banner" role="alert">
        <span>{t("console.confirmation", {
          lines: lineCount,
          lineLabel: lineCount === 1 ? t("console.line") : t("console.lines"),
          sessions: targetResolution.targets.length,
          sessionLabel: targetResolution.targets.length === 1 ? t("console.session") : t("console.sessions"),
          names: targetNames,
        })}</span>
        <button class="send" onclick={sendDraft} disabled={sending || targetResolution.targets.length === 0}>{t("console.confirmSend")}</button>
        <button class="secondary" onclick={() => (confirmingMultiline = false)}>{t("console.cancel")}</button>
      </div>
    {/if}
  </div>

  <div class="console-layout">
    <aside class="target-panel" aria-label={t("console.commandTargets")}>
      <div class="session-picker-heading">
        <span>{t("console.openConnections")}</span>
        <span>{t("console.liveCount", { count: sessions.filter((session) => session.connected).length })}</span>
      </div>
      <label class="target-mode">
        <span>{t("console.sendTo")}</span>
        <select value={targetMode} onchange={onTargetModeChange} disabled={sending} aria-label={t("console.commandTarget")}>
          <option value="active">{t("console.target.active")}</option>
          <option value="selected">{t("console.target.selected")}</option>
          <option value="all">{t("console.target.all")}</option>
        </select>
      </label>

      <input
        class="session-search"
        type="search"
        bind:value={sessionQuery}
        placeholder={t("console.filterConnections")}
        aria-label={t("console.filterOpenConnections")}
        disabled={sending}
      />

      <div class="session-choices" role="list" aria-label={t("console.openConnections")}>
        {#if visibleSessions.length === 0}
          <span class="session-empty" role="status">{t("console.noFilterMatches")}</span>
        {:else}
          {#each visibleSessions as session (session.sessionId)}
            {#if targetMode === "selected"}
              <label role="listitem" class:protected={session.protected} class:disconnected={!session.connected} class="session-row">
                <input
                  type="checkbox"
                  checked={session.connected && selectedSessionIds.includes(session.sessionId)}
                  onchange={(event) => toggleSelected(session.sessionId, event.currentTarget.checked)}
                  disabled={sending || !session.connected}
                />
                <span class="session-name">{session.name}</span>
                {#if session.protected}<small class="protected-tag">{t("console.protected")}</small>{/if}
                {#if !session.connected}<small class="offline">{t("console.disconnected")}</small>{/if}
              </label>
            {:else}
              <div role="listitem" class:protected={session.protected} class:disconnected={!session.connected} class="session-row">
                <span class="session-target-state" class:targeted={isTargeted(session)} aria-hidden="true">
                  {isTargeted(session) ? "●" : "·"}
                </span>
                <span class="session-name">{session.name}</span>
                {#if session.protected}<small class="protected-tag">{t("console.protected")}</small>{/if}
                {#if !session.connected}<small class="offline">{t("console.disconnected")}</small>{/if}
              </div>
            {/if}
          {/each}
        {/if}
      </div>

      {#if targetMode !== "active" && hasProtectedSessions}
        <label class="protected-opt-in">
          <input type="checkbox" checked={includeProtected} onchange={toggleProtected} disabled={sending} />
          {t("console.includeProtected")}
        </label>
      {/if}
    </aside>

    <div class="editor-panel">
      <div class="target-summary" aria-live="polite">
        <span>{t("console.targetSummary", { count: targetResolution.targets.length, names: targetNames || t("console.noEligible") })}</span>
        {#if targetResolution.excludedProtected > 0}
          <span class="excluded">{t("console.protectedExcluded", { count: targetResolution.excludedProtected })}</span>
        {/if}
      </div>

      <textarea
        bind:this={editor}
        bind:value={draft}
        onkeydown={onEditorKeydown}
        placeholder={t("console.editorPlaceholder")}
        aria-label={t("console.commandText")}
        spellcheck="false"
        disabled={targetResolution.targets.length === 0 || sending}
      ></textarea>

      <div class="console-actions">
        <span class="status" class:error={statusError} role="status">{status}</span>
        <button class="secondary" onclick={clearDraft} disabled={draft.length === 0}>{t("console.clear")}</button>
        <button class="send" onclick={requestSend} disabled={targetResolution.targets.length === 0 || draft.length === 0 || sending}>
          {sending ? t("console.sending") : t("console.send")}
        </button>
      </div>
    </div>
  </div>
</section>

<style>
  .command-console {
    flex: 0 0 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    padding: 0.45rem 0.7rem 0.55rem;
    background: var(--surface);
    border-top: 1px solid var(--border);
    box-sizing: border-box;
    overflow: hidden;
  }
  .command-console.hidden { display: none; }
  .console-heading { display: flex; align-items: center; gap: 0.75rem; min-height: 1.65rem; flex: 0 0 auto; }
  .console-heading strong { flex: 0 0 auto; font-size: 0.78rem; font-weight: 600; color: var(--text); }
  .confirmation-banner {
    display: flex; flex: 1 1 auto; align-items: center; gap: 0.5rem; min-width: 0;
    padding: 0.28rem 0.45rem; color: var(--warning, #d6a85f);
    background: color-mix(in srgb, var(--warning, #d6a85f) 12%, var(--surface));
    border: 1px solid color-mix(in srgb, var(--warning, #d6a85f) 55%, var(--border));
    border-radius: 4px; font-size: 0.72rem; line-height: 1.3;
  }
  .confirmation-banner span { flex: 1 1 auto; min-width: 0; }
  .confirmation-banner button { padding: 0.22rem 0.5rem; }
  .console-layout {
    display: grid;
    grid-template-columns: minmax(10.5rem, 0.34fr) minmax(0, 1fr);
    gap: 0.65rem;
    flex: 1 1 auto;
    min-height: 0;
  }
  .target-panel {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    min-width: 0;
    min-height: 0;
    padding-right: 0.65rem;
    border-right: 1px solid var(--border);
  }
  .session-picker-heading {
    display: flex;
    justify-content: space-between;
    gap: 0.3rem;
    color: var(--text-secondary);
    font-size: 0.65rem;
  }
  .target-mode {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 0.12rem;
    color: var(--text-secondary);
    font-size: 0.68rem;
  }
  .target-mode select {
    width: 100%;
    min-width: 0;
    padding: 0.18rem 0.35rem;
    color: var(--text);
    background: var(--app-bg);
    border: 1px solid var(--border);
    border-radius: 3px;
    font: inherit;
  }
  .target-mode select:focus { outline: none; border-color: var(--accent); }
  .session-search {
    flex: 0 0 auto;
    width: 100%;
    min-height: 1.3rem;
    box-sizing: border-box;
    padding: 0.12rem 0.35rem;
    color: var(--text);
    background: var(--app-bg);
    border: 1px solid var(--border);
    border-radius: 3px;
    font: inherit;
    font-size: 0.66rem;
  }
  .session-search:focus { outline: none; border-color: var(--accent); }
  .session-choices {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    border: 1px solid var(--border);
    border-radius: 3px;
    color: var(--text-secondary);
    font-size: 0.68rem;
  }
  .session-row {
    display: flex;
    flex: 0 0 auto;
    align-items: center;
    min-width: 0;
    min-height: 1.25rem;
    gap: 0.35rem;
    padding: 0.08rem 0.35rem;
    border-bottom: 1px solid var(--border);
  }
  .session-row:last-child { border-bottom: 0; }
  .session-row.protected { background: color-mix(in srgb, var(--warning, #d6a85f) 6%, transparent); }
  .session-row.disconnected { opacity: 0.6; }
  .session-row input, .protected-opt-in input { flex: 0 0 auto; accent-color: var(--accent); }
  .session-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .session-row small { flex: 0 0 auto; color: var(--warning, #d6a85f); font-size: 0.58rem; }
  .session-row small.offline { color: var(--danger); }
  .session-row small.protected-tag { color: var(--warning, #d6a85f); }
  .session-target-state { flex: 0 0 0.85rem; width: 0.85rem; color: var(--muted); text-align: center; font-size: 0.72rem; }
  .session-target-state.targeted { color: var(--accent); }
  .session-empty { padding: 0.35rem; color: var(--muted); }
  .protected-opt-in { flex: 0 0 auto; display: flex; align-items: center; gap: 0.25rem; color: var(--warning, #d6a85f); font-size: 0.64rem; }
  .editor-panel {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    min-width: 0;
    min-height: 0;
  }
  .target-summary {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem 0.65rem;
    flex: 0 0 auto;
    color: var(--text-secondary);
    font-size: 0.68rem;
    line-height: 1.25;
  }
  .excluded { color: var(--warning, #d6a85f); }
  textarea {
    width: 100%;
    min-height: 0;
    flex: 1 1 auto;
    resize: none;
    box-sizing: border-box;
    padding: 0.45rem 0.6rem;
    color: var(--text);
    background: var(--app-bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    font: 0.78rem/1.45 Consolas, "Cascadia Mono", monospace;
    tab-size: 4;
  }
  textarea:focus { outline: none; border-color: var(--accent); }
  textarea:disabled { opacity: 0.55; }
  .console-actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 0.55rem;
    min-height: 1.45rem;
    flex: 0 0 auto;
  }
  .status, .confirmation { flex: 1; min-width: 0; color: var(--text-secondary); font-size: 0.68rem; }
  .status.error { color: var(--danger); }
  button {
    flex: 0 0 auto;
    padding: 0.3rem 0.65rem;
    border: 1px solid var(--border);
    border-radius: 4px;
    font: inherit;
    font-size: 0.72rem;
    cursor: pointer;
  }
  button.send { color: var(--accent-contrast); background: var(--accent); border-color: var(--accent); }
  button.secondary { color: var(--text-secondary); background: transparent; }
  button:disabled { opacity: 0.45; cursor: default; }
  @media (max-width: 720px) {
    .console-heading { align-items: flex-start; flex-wrap: wrap; }
    .confirmation-banner { flex-basis: 100%; flex-wrap: wrap; }
    .console-layout { grid-template-columns: minmax(8.5rem, 0.42fr) minmax(0, 1fr); gap: 0.4rem; }
    .target-panel { padding-right: 0.4rem; }
    .session-row { gap: 0.25rem; padding-inline: 0.25rem; }
  }
</style>
