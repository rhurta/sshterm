<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { toBase64 } from "$lib/encoding";
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
  let hasProtectedSessions = $derived(sessions.some((session) => session.protected));

  function onTargetModeChange(event: Event) {
    targetMode = (event.currentTarget as HTMLSelectElement).value as BroadcastTargetMode;
    if (targetMode === "selected" && selectedSessionIds.length === 0 && activeSessionId) {
      selectedSessionIds = [activeSessionId];
    }
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
      status = "No eligible open sessions are selected.";
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

      const summary = `Write requests accepted by ${accepted.length} of ${targets.length} sessions. Remote command completion is not verified.`;
      status = failed.length ? `${summary} Failed: ${failed.join(" | ")}` : summary;
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

<section class="command-console" class:hidden={!visible} style={`height: ${height}px;`} aria-label="Command console">
  <div class="console-heading">
    <strong>Command console</strong>
    <label class="target-mode">
      <span>Send to</span>
      <select value={targetMode} onchange={onTargetModeChange} disabled={sending} aria-label="Command target">
        <option value="active">Active session</option>
        <option value="selected">Selected sessions</option>
        <option value="all">All open sessions</option>
      </select>
    </label>
  </div>

  {#if targetMode === "selected"}
    <div class="session-choices" aria-label="Select target sessions">
      {#each sessions as session (session.sessionId)}
        <label class:protected={session.protected}>
          <input
            type="checkbox"
            checked={selectedSessionIds.includes(session.sessionId)}
            onchange={(event) => toggleSelected(session.sessionId, event.currentTarget.checked)}
            disabled={sending}
          />
          <span>{session.name}</span>
          {#if session.protected}<small>Protected</small>{/if}
        </label>
      {/each}
    </div>
  {/if}

  {#if targetMode !== "active" && hasProtectedSessions}
    <label class="protected-opt-in">
      <input type="checkbox" checked={includeProtected} onchange={toggleProtected} disabled={sending} />
      Include protected connections
    </label>
  {/if}

  <div class="target-summary" aria-live="polite">
    <span>Targets ({targetResolution.targets.length}): {targetNames || "No eligible sessions"}</span>
    {#if targetResolution.excludedProtected > 0}
      <span class="excluded">{targetResolution.excludedProtected} protected excluded</span>
    {/if}
  </div>

  <textarea
    bind:this={editor}
    bind:value={draft}
    onkeydown={onEditorKeydown}
    placeholder="Write or paste commands here. Enter adds a new line; it does not execute."
    aria-label="Command text"
    spellcheck="false"
    disabled={targetResolution.targets.length === 0 || sending}
  ></textarea>

  <div class="console-actions">
    {#if confirmingMultiline}
      <span class="confirmation" role="alert">
        Send {lineCount} line{lineCount === 1 ? "" : "s"} to {targetResolution.targets.length} session{targetResolution.targets.length === 1 ? "" : "s"}: {targetNames}.
        Each host receives the same bytes independently; remote completion is not verified.
      </span>
      <button class="send" onclick={sendDraft} disabled={sending || targetResolution.targets.length === 0}>Confirm send</button>
      <button class="secondary" onclick={() => (confirmingMultiline = false)}>Cancel</button>
    {:else}
      <span class="status" class:error={statusError} role="status">{status}</span>
      <button class="secondary" onclick={clearDraft} disabled={draft.length === 0}>Clear</button>
      <button class="send" onclick={requestSend} disabled={targetResolution.targets.length === 0 || draft.length === 0 || sending}>
        {sending ? "Sending…" : "Send"}
      </button>
    {/if}
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
  .console-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.6rem;
    min-height: 1.45rem;
  }
  .console-heading strong { font-size: 0.78rem; font-weight: 600; color: var(--text); }
  .target-mode { display: flex; align-items: center; gap: 0.4rem; color: var(--text-secondary); font-size: 0.72rem; }
  .target-mode select {
    max-width: 12rem;
    padding: 0.2rem 0.4rem;
    color: var(--text);
    background: var(--app-bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    font: inherit;
  }
  .target-mode select:focus { outline: none; border-color: var(--accent); }
  .target-summary {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem 0.65rem;
    color: var(--text-secondary);
    font-size: 0.68rem;
    line-height: 1.25;
  }
  .excluded { color: var(--warning, #d6a85f); }
  .session-choices {
    display: flex;
    flex-wrap: wrap;
    gap: 0.2rem 0.7rem;
    max-height: 3.2rem;
    overflow-y: auto;
    color: var(--text-secondary);
    font-size: 0.7rem;
  }
  .session-choices label, .protected-opt-in {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
  }
  .session-choices input, .protected-opt-in input { accent-color: var(--accent); }
  .session-choices small { color: var(--warning, #d6a85f); font-size: 0.62rem; }
  .protected-opt-in { color: var(--warning, #d6a85f); font-size: 0.68rem; }
  textarea {
    width: 100%;
    min-height: 3.6rem;
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
  }
  .status, .confirmation { flex: 1; min-width: 0; color: var(--text-secondary); font-size: 0.68rem; }
  .status.error { color: var(--danger); }
  .confirmation { color: var(--warning, #d6a85f); }
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
</style>
