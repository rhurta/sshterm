<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { tick } from "svelte";
  import { toBase64 } from "$lib/encoding";
  import { t } from "$lib/i18n/index.svelte";
  import ScriptsDialog from "./ScriptsDialog.svelte";
  import type { FavoriteScript, ScriptDetail } from "$lib/types";
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
  // Scripts library: the dialog (save or load) and the script the text came from.
  let scriptsDialog = $state<"save" | "load" | null>(null);
  let loadedScript = $state<{ id: string; name: string } | null>(null);
  // Favorite scripts: one-click buttons under the editor.
  let favorites = $state<FavoriteScript[]>([]);
  let pendingFavorite = $state<ScriptDetail | null>(null);
  let saveButton: HTMLButtonElement | undefined = $state();
  let loadButton: HTMLButtonElement | undefined = $state();
  let targetMode = $state<BroadcastTargetMode>("active");
  let selectedSessionIds = $state<string[]>([]);
  let includeProtected = $state(false);
  let sessionQuery = $state("");
  let confirmingMultiline = $state(false);
  let pendingSendText = $state<string | null>(null);
  let pendingSelection = $state<{ start: number; end: number } | null>(null);
  let sending = $state(false);
  let status = $state("");
  let statusError = $state(false);
  let editor: HTMLTextAreaElement;

  // Target panel resize state
  let targetPanelWidth = $state(10.5 * 16); // default ~10.5rem in px (assuming 16px base)
  let targetResize: { startX: number; startWidth: number } | null = null;
  const MIN_TARGET_WIDTH = 8 * 16; // 8rem
  const MAX_TARGET_WIDTH = 24 * 16; // 24rem

  function startTargetResize(event: PointerEvent) {
    if (event.button !== 0) return;
    event.preventDefault();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    targetResize = { startX: event.clientX, startWidth: targetPanelWidth };
  }

  function moveTargetResize(event: PointerEvent) {
    if (!targetResize) return;
    const delta = event.clientX - targetResize.startX;
    targetPanelWidth = Math.max(MIN_TARGET_WIDTH, Math.min(MAX_TARGET_WIDTH, targetResize.startWidth + delta));
  }

  function endTargetResize() {
    if (!targetResize) return;
    targetResize = null;
  }

  let pendingLineCount = $derived(pendingSendText
    ? pendingSendText.replace(/(?:\r\n|\r|\n)+$/, "").split(/\r\n|\r|\n/).length
    : 0);
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
    pendingSendText = null;
    pendingSelection = null;
    status = "";
  }

  function toggleSelected(sessionId: string, checked: boolean) {
    selectedSessionIds = checked
      ? [...new Set([...selectedSessionIds, sessionId])]
      : selectedSessionIds.filter((id) => id !== sessionId);
    confirmingMultiline = false;
    pendingSendText = null;
    pendingSelection = null;
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
    pendingSendText = null;
    pendingSelection = null;
    status = "";
  }

  async function restoreEditorFocus(selectionStart?: number, selectionEnd?: number) {
    // Wait for Svelte to apply state changes (including removing the disabled
    // attribute), then focus after the browser finishes the current click/key event.
    await tick();
    requestAnimationFrame(() => {
      if (!editor || editor.disabled) return;
      editor.focus();
      if (selectionStart !== undefined) {
        editor.setSelectionRange(selectionStart, selectionEnd ?? selectionStart);
      }
    });
  }

  async function requestSendText(text: string, selectionStart: number, selectionEnd: number) {
    const resolution = resolveBroadcastTargets(
      sessions,
      activeSessionId,
      targetMode,
      selectedSessionIds,
      includeProtected,
    );
    if (resolution.targets.length === 0 || text.length === 0 || sending || confirmingMultiline) return;

    pendingSendText = text;
    pendingSelection = { start: selectionStart, end: selectionEnd };
    const textLineCount = text.replace(/(?:\r\n|\r|\n)+$/, "").split(/\r\n|\r|\n/).length;
    if (textLineCount > 1 || resolution.targets.length > 1) {
      confirmingMultiline = true;
      status = "";
      await restoreEditorFocus(selectionStart, selectionEnd);
      return;
    }
    void sendDraft();
  }

  async function requestSend() {
    if (draft.length === 0 || sending) return;
    const selectionStart = editor?.selectionStart ?? draft.length;
    const selectionEnd = editor?.selectionEnd ?? selectionStart;
    await requestSendText(draft, selectionStart, selectionEnd);
  }

  async function requestSendCurrentLine() {
    if (!editor || sending || confirmingMultiline) return;
    // Read the textarea value directly; it is the source of truth during keydown.
    const currentDraft = editor.value;
    if (draft !== currentDraft) draft = currentDraft;
    const cursor = editor.selectionDirection === "backward"
      ? editor.selectionStart
      : editor.selectionEnd;
    const beforeCursor = currentDraft.slice(0, cursor);
    const previousBreak = Math.max(beforeCursor.lastIndexOf("\n"), beforeCursor.lastIndexOf("\r"));
    const lineStart = previousBreak + 1;
    const afterCursor = currentDraft.slice(cursor);
    const nextLf = afterCursor.indexOf("\n");
    const nextCr = afterCursor.indexOf("\r");
    const nextBreak = nextLf < 0 ? nextCr : nextCr < 0 ? nextLf : Math.min(nextLf, nextCr);
    const lineEnd = nextBreak < 0 ? currentDraft.length : cursor + nextBreak;
    const line = currentDraft.slice(lineStart, lineEnd);
    if (!line.trim()) return;

    // Include a line feed so the remote shell executes this command. The draft
    // itself is left untouched, and the caret is restored to its original spot.
    await requestSendText(line + "\n", cursor, cursor);
  }

  async function sendDraft() {
    const text = pendingSendText ?? draft;
    if (text.length === 0 || sending) return;
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
      cancelPendingSend();
      status = t("console.noEligibleSessions");
      statusError = true;
      return;
    }

    const selection = pendingSelection ?? {
      start: editor?.selectionStart ?? draft.length,
      end: editor?.selectionEnd ?? draft.length,
    };
    sending = true;
    confirmingMultiline = false;
    status = "";
    statusError = false;
    const payload = toBase64(new TextEncoder().encode(text));
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
          failed.push(target.name + ": " + String(error));
        }
      }

      const summary = t("console.writeAccepted", { accepted: accepted.length, total: targets.length });
      status = failed.length ? summary + " " + t("console.failed", { details: failed.join(" | ") }) : summary;
      statusError = failed.length > 0;
    } finally {
      sending = false;
      pendingSendText = null;
      pendingSelection = null;
      // A line send leaves the editor contents intact and restores the original
      // caret so the console remains ready for the next command.
      await restoreEditorFocus(selection.start, selection.end);
    }
  }

  function cancelPendingSend() {
    confirmingMultiline = false;
    pendingSendText = null;
    pendingSelection = null;
  }

  function onEditorKeydown(event: KeyboardEvent) {
    if (event.key === "Enter" && event.shiftKey && !event.ctrlKey && !event.metaKey && !event.altKey) {
      event.preventDefault();
      void requestSendCurrentLine();
    } else if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
      event.preventDefault();
      void requestSend();
    } else if (event.key === "Escape" && confirmingMultiline) {
      cancelPendingSend();
    }
  }

  function applyLoadedScript(script: ScriptDetail, how: "replace" | "append") {
    if (how === "append" && draft.length > 0) {
      draft = `${draft}${draft.endsWith("\n") ? "" : "\n"}${script.body}`;
    } else {
      draft = script.body;
    }
    // The text now comes from this script, so Save can update it. Appending mixes
    // sources, so it keeps the previous link only when there was none.
    loadedScript = how === "replace" || !loadedScript ? { id: script.id, name: script.name } : loadedScript;
    cancelPendingSend();
    status = t("scripts.loadedStatus", { name: script.name });
    statusError = false;
  }

  async function loadFavorites() {
    try {
      favorites = await invoke<FavoriteScript[]>("scripts_favorites");
    } catch {
      favorites = []; // library locked, needs its password, or unavailable: no buttons
    }
  }

  // Refresh whenever the console is shown (the library may have changed meanwhile).
  $effect(() => {
    if (visible) void loadFavorites();
  });

  async function useFavorite(favorite: FavoriteScript) {
    try {
      const script = await invoke<ScriptDetail>("scripts_get", { id: favorite.id });
      if (draft.trim().length === 0) {
        applyLoadedScript(script, "replace");
      } else {
        pendingFavorite = script; // ask whether to replace or append
      }
    } catch {
      await loadFavorites(); // it was deleted or the library closed
    }
  }

  function finishFavorite(how: "replace" | "append") {
    if (pendingFavorite) applyLoadedScript(pendingFavorite, how);
    pendingFavorite = null;
    editor?.focus();
  }

  function scriptsDialogClosed() {
    const opener = scriptsDialog === "save" ? saveButton : loadButton;
    scriptsDialog = null;
    void loadFavorites();
    void tick().then(() => (draft.length > 0 ? opener : loadButton)?.focus());
  }

  function clearDraft() {
    loadedScript = null;
    draft = "";
    cancelPendingSend();
    status = "";
    statusError = false;
    editor?.focus();
  }
</script>

<svelte:window onpointermove={(event) => { moveTargetResize(event); }} onpointerup={endTargetResize} onpointercancel={endTargetResize} />

<section class="command-console" class:hidden={!visible} style={`height: ${height}px;`} aria-label={t("console.commandConsole")}>
  <div class="console-heading workspace-panel-header">
    <strong class="panel-title">{t("console.commandConsole")}</strong>
    <button
      class="shortcut-help"
      type="button"
      title={t("console.shortcutSendHelp")}
      aria-label={t("console.shortcutSendHelp")}
    >?</button>
    {#if confirmingMultiline}
      <div class="confirmation-banner" role="alert">
        <span>{t("console.confirmation", {
          lines: pendingLineCount,
          lineLabel: pendingLineCount === 1 ? t("console.line") : t("console.lines"),
          sessions: targetResolution.targets.length,
          sessionLabel: targetResolution.targets.length === 1 ? t("console.session") : t("console.sessions"),
          names: targetNames,
        })}</span>
        <button class="send" onclick={sendDraft} disabled={sending || targetResolution.targets.length === 0}>{t("console.confirmSend")}</button>
        <button class="secondary" onclick={cancelPendingSend}>{t("console.cancel")}</button>
      </div>
    {/if}
  </div>

  <div class="console-layout" style={`--target-width: ${targetPanelWidth}px;`}>
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

    <!-- svelte-ignore a11y_no_noninteractive_tabindex a11y_no_noninteractive_element_interactions -->
    <div class="target-resizer" role="separator" aria-orientation="vertical" aria-label={t("console.resizeTargets")} aria-valuemin={MIN_TARGET_WIDTH} aria-valuemax={MAX_TARGET_WIDTH} aria-valuenow={targetPanelWidth} tabindex="0" onpointerdown={startTargetResize} onkeydown={(event) => { if (event.key === "ArrowLeft" || event.key === "ArrowRight") { event.preventDefault(); targetPanelWidth = Math.max(MIN_TARGET_WIDTH, Math.min(MAX_TARGET_WIDTH, targetPanelWidth + (event.key === "ArrowLeft" ? -48 : 48))); } }} title={t("console.resizeTargets")}></div>

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

      {#if pendingFavorite}
        <div class="favorite-confirm" role="alert">
          <span>{t("scripts.draftNotEmpty")}</span>
          <button class="send" onclick={() => finishFavorite("replace")}>{t("scripts.replace")}</button>
          <button class="secondary" onclick={() => finishFavorite("append")}>{t("scripts.append")}</button>
          <button class="secondary" onclick={() => (pendingFavorite = null)}>{t("scripts.cancel")}</button>
        </div>
      {/if}

      <div class="console-actions">
        <span class="status" class:error={statusError} role="status">{status}</span>
        {#if loadedScript}
          <span class="loaded-script" title={loadedScript.name}>{t("console.loadedScript", { name: loadedScript.name })}</span>
        {/if}
        <button
          class="secondary"
          bind:this={saveButton}
          onclick={() => (scriptsDialog = "save")}
          disabled={draft.trim().length === 0}
          title={draft.trim().length === 0 ? t("console.saveScriptEmpty") : undefined}
        >{t("console.saveScript")}</button>
        <button class="secondary" bind:this={loadButton} onclick={() => (scriptsDialog = "load")}>{t("console.loadScript")}</button>
        <button class="secondary" onclick={clearDraft} disabled={draft.length === 0}>{t("console.clear")}</button>
        <button class="send" onclick={requestSend} disabled={targetResolution.targets.length === 0 || draft.length === 0 || sending}>
          {sending ? t("console.sending") : t("console.send")}
        </button>
      </div>

      {#if favorites.length > 0}
        <div class="favorites" role="group" aria-label={t("console.favorites")}>
          <span class="favorites-label" aria-hidden="true">★</span>
          {#each favorites as favorite (favorite.id)}
            <button
              class="favorite"
              onclick={() => useFavorite(favorite)}
              title={favorite.folder_path ? `${favorite.folder_path} / ${favorite.name}` : favorite.name}
              aria-label={t("console.loadFavorite", { name: favorite.name })}
            >{favorite.name}</button>
          {/each}
        </div>
      {/if}
    </div>
  </div>
</section>

{#if scriptsDialog}
  <ScriptsDialog
    mode={scriptsDialog}
    {draft}
    loaded={loadedScript}
    onclose={scriptsDialogClosed}
    onload={applyLoadedScript}
    onsaved={(script) => {
      loadedScript = script;
      status = t("scripts.savedStatus", { name: script.name });
      statusError = false;
    }}
  />
{/if}

<style>
  .command-console {
    flex: 0 0 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 0;
    padding: 0;
    background: var(--surface);
    border-top: 1px solid var(--border);
    box-sizing: border-box;
    overflow: hidden;
  }
  .command-console.hidden { display: none; }
  .console-heading { flex: 0 0 auto; }
  .console-heading .panel-title { flex: 0 0 auto; }
  .shortcut-help {
    flex: 0 0 20px;
    width: 20px;
    height: 20px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    color: var(--muted);
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 50%;
    font: inherit;
    font-size: var(--ui-font-meta);
    line-height: 1;
    cursor: help;
  }
  .shortcut-help:hover,
  .shortcut-help:focus-visible {
    color: var(--accent);
    border-color: var(--accent);
    outline: none;
  }
  .confirmation-banner {
    display: flex; flex: 1 1 auto; align-items: center; gap: 0.5rem; min-width: 0;
    padding: 0.28rem 0.45rem; color: var(--warning, #d6a85f);
    background: color-mix(in srgb, var(--warning, #d6a85f) 12%, var(--surface));
    border: 1px solid color-mix(in srgb, var(--warning, #d6a85f) 55%, var(--border));
    border-radius: 4px; font-size: var(--ui-font-dense); line-height: 1.3;
  }
  .confirmation-banner span { flex: 1 1 auto; min-width: 0; }
  .confirmation-banner button { padding: 0.22rem 0.5rem; }
  .console-layout {
    box-sizing: border-box;
    padding: 8px 12px 10px;
    display: grid;
    grid-template-columns: var(--target-width, 168px) 6px minmax(0, 1fr);
    gap: 0 0.65rem;
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
  .target-resizer {
    flex: 0 0 6px;
    cursor: col-resize;
    background: var(--border);
    touch-action: none;
    outline: none;
  }
  .target-resizer:hover,
  .target-resizer:focus-visible {
    background: var(--accent);
  }
  .session-picker-heading {
    display: flex;
    justify-content: space-between;
    gap: 0.3rem;
    color: var(--text-secondary);
    font-size: var(--ui-font-meta);
  }
  .target-mode {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 0.12rem;
    color: var(--text-secondary);
    font-size: var(--ui-font-meta);
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
    font-size: var(--ui-font-meta);
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
    font-size: var(--ui-font-meta);
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
  .session-row small { flex: 0 0 auto; color: var(--warning, #d6a85f); font-size: var(--ui-font-meta); }
  .session-row small.offline { color: var(--danger); }
  .session-row small.protected-tag { color: var(--warning, #d6a85f); }
  .session-target-state { flex: 0 0 0.85rem; width: 0.85rem; color: var(--muted); text-align: center; font-size: var(--ui-font-dense); }
  .session-target-state.targeted { color: var(--accent); }
  .session-empty { padding: 0.35rem; color: var(--muted); }
  .protected-opt-in { flex: 0 0 auto; display: flex; align-items: center; gap: 0.25rem; color: var(--warning, #d6a85f); font-size: var(--ui-font-meta); }
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
    font-size: var(--ui-font-meta);
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
  .status, .confirmation { flex: 1; min-width: 0; color: var(--text-secondary); font-size: var(--ui-font-meta); }
  .status.error { color: var(--danger); }
  /* One line that scrolls sideways, so a short console keeps its editor. */
  .favorites { display: flex; flex-wrap: nowrap; align-items: center; gap: 0.3rem; overflow-x: auto; overflow-y: hidden; padding-bottom: 0.15rem; flex: 0 0 auto; }
  .favorites-label { color: var(--warning); font-size: var(--ui-font-meta); }
  .favorite { flex: 0 0 auto; font: inherit; font-size: var(--ui-font-meta); max-width: 14rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text-secondary); background: var(--surface); border: 1px solid var(--border); border-radius: 4px; padding: 0.2rem 0.55rem; cursor: pointer; }
  .favorite:hover { color: var(--text); background: var(--surface-hover); }
  .favorite:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .favorite-confirm { display: flex; flex-wrap: wrap; align-items: center; gap: 0.4rem; padding: 0.35rem 0.5rem; background: var(--warning-bg, var(--surface)); border: 1px solid var(--warning-border, var(--border)); border-radius: 4px; font-size: var(--ui-font-meta); }
  .favorite-confirm span { flex: 1 1 10rem; }
  .loaded-script { max-width: 16rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--muted); font-size: var(--ui-font-meta); }
  button {
    flex: 0 0 auto;
    padding: 0.3rem 0.65rem;
    border: 1px solid var(--border);
    border-radius: 4px;
    font: inherit;
    font-size: var(--ui-font-dense);
    cursor: pointer;
  }
  button.send { color: var(--accent-contrast); background: var(--accent); border-color: var(--accent); }
  button.secondary { color: var(--text-secondary); background: transparent; }
  button:disabled { opacity: 0.45; cursor: default; }
  @media (max-width: 720px) {
    .console-heading { gap: 8px; }
    .confirmation-banner { min-width: 0; padding: 2px 6px; }
    .confirmation-banner span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .console-layout { grid-template-columns: var(--target-width, 136px) 4px minmax(0, 1fr); gap: 0 0.4rem; }
    .target-panel { padding-right: 0.4rem; }
    .target-resizer { flex: 0 0 4px; }
    .session-row { gap: 0.25rem; padding-inline: 0.25rem; }
  }
</style>
