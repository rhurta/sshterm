<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { toBase64 } from "$lib/encoding";

  let {
    visible,
    sessionId,
    sessionName,
  }: {
    visible: boolean;
    sessionId: string | null;
    sessionName: string;
  } = $props();

  let draft = $state("");
  let confirmingMultiline = $state(false);
  let sending = $state(false);
  let status = $state("");
  let statusError = $state(false);
  let editor: HTMLTextAreaElement;

  let lineCount = $derived(draft.length ? draft.split(/\r\n|\r|\n/).length : 0);
  let hasMultipleLines = $derived(lineCount > 1);

  function requestSend() {
    if (!sessionId || draft.length === 0 || sending) return;
    if (hasMultipleLines && !confirmingMultiline) {
      confirmingMultiline = true;
      status = "";
      return;
    }
    void sendDraft();
  }

  async function sendDraft() {
    if (!sessionId || draft.length === 0 || sending) return;
    sending = true;
    confirmingMultiline = false;
    status = "";
    statusError = false;
    try {
      await invoke("session_write", {
        id: sessionId,
        data: toBase64(new TextEncoder().encode(draft)),
      });
      status = "Write request sent to the active session. Remote command completion is not verified.";
    } catch (error) {
      status = `Could not send to ${sessionName || "the active session"}: ${String(error)}`;
      statusError = true;
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
    editor?.focus();
  }
</script>

<section class="command-console" class:hidden={!visible} aria-label="Command console">
  <div class="console-heading">
    <strong>Command console</strong>
    <span class="target" aria-live="polite">
      Target: {sessionName ? sessionName : "No active session"}
    </span>
  </div>
  <textarea
    bind:this={editor}
    bind:value={draft}
    onkeydown={onEditorKeydown}
    placeholder="Write or paste commands here. Enter adds a new line; it does not execute."
    aria-label="Command text"
    spellcheck="false"
    disabled={!sessionId}
  ></textarea>
  <div class="console-actions">
    {#if confirmingMultiline}
      <span class="confirmation" role="alert">
        Send {lineCount} lines to {sessionName}? Commands will be written exactly as entered.
      </span>
      <button class="send" onclick={sendDraft} disabled={sending}>Confirm send</button>
      <button class="secondary" onclick={() => (confirmingMultiline = false)}>Cancel</button>
    {:else}
      <span class="status" class:error={statusError} role="status">{status}</span>
      <button class="secondary" onclick={clearDraft} disabled={draft.length === 0}>Clear</button>
      <button class="send" onclick={requestSend} disabled={!sessionId || draft.length === 0 || sending}>
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
    gap: 0.4rem;
    padding: 0.55rem 0.7rem 0.65rem;
    background: var(--surface);
    border-top: 1px solid var(--border);
    box-sizing: border-box;
  }
  .command-console.hidden { display: none; }
  .console-heading, .console-actions {
    display: flex;
    align-items: center;
    gap: 0.55rem;
    min-height: 1.45rem;
  }
  .console-heading strong { font-size: 0.78rem; font-weight: 600; color: var(--text); }
  .target { color: var(--text-secondary); font-size: 0.72rem; }
  textarea {
    width: 100%;
    min-height: 4.2rem;
    flex: 1 1 auto;
    resize: none;
    box-sizing: border-box;
    padding: 0.5rem 0.6rem;
    color: var(--text);
    background: var(--app-bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    font: 0.78rem/1.45 Consolas, "Cascadia Mono", monospace;
    tab-size: 4;
  }
  textarea:focus { outline: none; border-color: var(--accent); }
  textarea:disabled { opacity: 0.55; }
  .console-actions { justify-content: flex-end; }
  .status, .confirmation { flex: 1; min-width: 0; color: var(--text-secondary); font-size: 0.7rem; }
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
