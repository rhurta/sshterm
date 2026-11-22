<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { ConnectionSummary, SftpDirectory, SftpEntry, SftpSessionInfo } from "$lib/types";

  let {
    onclose,
    onlock,
    staleSessionIds = [],
    onclearstale,
  }: {
    onclose: () => void;
    onlock: (sessionIds: string[]) => void | Promise<void>;
    staleSessionIds?: string[];
    onclearstale?: () => void;
  } = $props();

  let connections = $state<ConnectionSummary[]>([]);
  let query = $state("");
  let selectedConnectionId = $state("");
  let sessions = $state<SftpSessionInfo[]>([]);
  let activeId = $state("");
  let sessionPaths = $state<Record<string, string>>({});
  let directory = $state<SftpDirectory | null>(null);
  let pathInput = $state("");
  let busy = $state(false);
  let loadingConnections = $state(false);
  let errorMessage = $state("");
  let events = $state<{ time: string; message: string; kind: "info" | "success" | "error" }[]>([]);
  let searchField: HTMLInputElement | undefined = $state();

  const activeSession = $derived(sessions.find((session) => session.session_id === activeId) ?? null);
  const filteredConnections = $derived.by(() => {
    const value = query.trim().toLocaleLowerCase();
    if (!value) return connections;
    return connections.filter((connection) =>
      [connection.name, connection.host, connection.user, connection.user + "@" + connection.host]
        .some((part) => part.toLocaleLowerCase().includes(value)),
    );
  });

  function record(message: string, kind: "info" | "success" | "error" = "info") {
    events = [...events.slice(-49), { time: new Date().toLocaleTimeString(), message, kind }];
    errorMessage = kind === "error" ? message : "";
  }

  async function refreshConnections() {
    loadingConnections = true;
    try {
      connections = await invoke<ConnectionSummary[]>("conn_list");
      if (!connections.some((item) => item.id === selectedConnectionId)) selectedConnectionId = connections[0]?.id ?? "";
      record("Loaded " + connections.length + " saved connection(s).", "success");
    } catch (error) {
      record("Could not load saved connections: " + String(error), "error");
    } finally {
      loadingConnections = false;
    }
  }

  async function listDirectory(sessionId: string, path: string) {
    busy = true;
    try {
      const result = await invoke<SftpDirectory>("sftp_list", { sessionId, remotePath: path });
      sessionPaths = { ...sessionPaths, [sessionId]: result.current_path };
      if (activeId === sessionId) {
        directory = result;
        pathInput = result.current_path;
      }
      record("Listed " + result.entries.length + " item(s) in " + result.current_path, "success");
    } catch (error) {
      record("Directory listing failed: " + String(error), "error");
    } finally {
      busy = false;
    }
  }

  async function openSession() {
    if (!selectedConnectionId || busy) return;
    const connection = connections.find((item) => item.id === selectedConnectionId);
    if (!connection) return;
    busy = true;
    let opened: SftpSessionInfo | null = null;
    try {
      opened = await invoke<SftpSessionInfo>("sftp_open", { connectionId: connection.id });
      sessions = [...sessions, opened];
      activeId = opened.session_id;
      sessionPaths = { ...sessionPaths, [opened.session_id]: opened.current_path };
      directory = null;
      pathInput = opened.current_path;
      record("Opened " + opened.connection_name + " (" + opened.user + "@" + opened.host + ":" + opened.port + ")"
        + (opened.jump_names.length ? " via " + opened.jump_names.join(" → ") : " directly")
        + (opened.agent_binding === "legacy_unbound" ? " — WARNING: legacy agent mode without session binding" : ""), "success");
    } catch (error) {
      record("SFTP open failed: " + String(error), "error");
    } finally {
      busy = false;
    }
    if (opened) await listDirectory(opened.session_id, opened.current_path);
  }

  async function selectSession(session: SftpSessionInfo) {
    if (busy || session.session_id === activeId) return;
    activeId = session.session_id;
    directory = null;
    await listDirectory(session.session_id, sessionPaths[session.session_id] ?? session.current_path);
  }

  async function navigate(path: string) {
    if (activeSession && !busy && path.trim()) await listDirectory(activeSession.session_id, path.trim());
  }

  async function closeSession(session: SftpSessionInfo) {
    if (busy) return;
    busy = true;
    try {
      await invoke("sftp_close", { sessionId: session.session_id });
      sessions = sessions.filter((item) => item.session_id !== session.session_id);
      const paths = { ...sessionPaths };
      delete paths[session.session_id];
      sessionPaths = paths;
      record("Closed SFTP session " + session.connection_name + ".", "success");
      if (activeId === session.session_id) {
        const next = sessions[0] ?? null;
        activeId = next?.session_id ?? "";
        directory = null;
        pathInput = "";
        if (next) {
          busy = false;
          await listDirectory(next.session_id, sessionPaths[next.session_id] ?? next.current_path);
          return;
        }
      }
    } catch (error) {
      record("Could not close SFTP session: " + String(error), "error");
    } finally {
      busy = false;
    }
  }

  async function closeAllSessions() {
    if (busy) return;
    busy = true;
    try {
      await invoke("sftp_close_all");
      record("Closed all SFTP sessions (" + sessions.length + " tracked in this panel).", "success");
      sessions = [];
      activeId = "";
      sessionPaths = {};
      directory = null;
      pathInput = "";
    } catch (error) {
      record("Could not close all SFTP sessions: " + String(error), "error");
    } finally {
      busy = false;
    }
  }

  async function lockVault() {
    if (busy) return;
    record("Locking vault with " + sessions.length + " active SFTP session(s).");
    try {
      await onlock(sessions.map((session) => session.session_id));
    } catch (error) {
      record("Vault lock failed: " + String(error), "error");
    }
  }

  async function verifyVaultCleanup() {
    if (busy || staleSessionIds.length === 0) return;
    busy = true;
    let passed = 0;
    let failed = 0;
    try {
      for (const sessionId of staleSessionIds) {
        try {
          await invoke<SftpDirectory>("sftp_list", { sessionId, remotePath: "/" });
          failed += 1;
          record("FAIL: old session " + sessionId.slice(0, 8) + " still accepts requests.", "error");
        } catch (error) {
          if (String(error).includes("sftp_session_not_found")) {
            passed += 1;
            record("PASS: old session " + sessionId.slice(0, 8) + " was invalidated by vault lock.", "success");
          } else {
            failed += 1;
            record("FAIL: old session " + sessionId.slice(0, 8) + " returned " + String(error), "error");
          }
        }
      }
      if (failed === 0) {
        record("Vault-lock cleanup verified: " + passed + " stale session(s) rejected.", "success");
        onclearstale?.();
      } else {
        record("Vault-lock cleanup check failed for " + failed + " session(s).", "error");
      }
    } finally {
      busy = false;
    }
  }

  function formatSize(size: number | null): string {
    if (size == null) return "—";
    if (size < 1024) return size + " B";
    if (size < 1048576) return (size / 1024).toFixed(1) + " KB";
    if (size < 1073741824) return (size / 1048576).toFixed(1) + " MB";
    return (size / 1073741824).toFixed(1) + " GB";
  }

  function modifiedLabel(entry: SftpEntry): string {
    return entry.modified == null ? "—" : new Date(entry.modified * 1000).toLocaleString();
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      onclose();
    }
  }

  onMount(() => {
    void refreshConnections();
    searchField?.focus();
    if (staleSessionIds.length) record(staleSessionIds.length + " session ID(s) from the previous vault-lock test are ready to verify.");
  });
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="qa-overlay">
  <section class="qa-panel" role="dialog" aria-modal="true" aria-labelledby="qa-title">
    <header class="qa-header">
      <div>
        <span class="qa-eyebrow">TEMPORARY DEVELOPER TOOL</span>
        <h2 id="qa-title">SFTP acceptance panel</h2>
        <p>Validate native SFTP sessions before enabling the M8c browser.</p>
      </div>
      <div class="qa-actions">
        <button class="qa-button" type="button" onclick={refreshConnections} disabled={busy || loadingConnections}>Refresh connections</button>
        <button class="qa-close" type="button" aria-label="Close panel" title="Close" onclick={onclose}>×</button>
      </div>
    </header>

    <div class="qa-body">
      <aside class="qa-sidebar">
        <div class="qa-section-title"><h3>Saved connections</h3><span>{connections.length}</span></div>
        <label for="qa-search">Search connections</label>
        <input id="qa-search" bind:this={searchField} bind:value={query} class="qa-input" type="search" placeholder="Name, host or user…" autocomplete="off" />
        <div class="qa-connection-list">
          {#if loadingConnections}
            <p class="qa-empty">Loading connections…</p>
          {:else if filteredConnections.length === 0}
            <p class="qa-empty">No matching saved connections.</p>
          {:else}
            {#each filteredConnections as connection (connection.id)}
              <button type="button" class="qa-connection" class:selected={selectedConnectionId === connection.id} aria-pressed={selectedConnectionId === connection.id} onclick={() => (selectedConnectionId = connection.id)}>
                <strong>{connection.name}</strong>
                <span>{connection.user}@{connection.host}:{connection.port}</span>
                <small>{connection.jump.length ? connection.jump.length + " jump hop(s)" : "Direct connection"}</small>
              </button>
            {/each}
          {/if}
        </div>
        <button class="qa-button qa-primary" type="button" onclick={openSession} disabled={!selectedConnectionId || busy}>Open SFTP session</button>
      </aside>

      <main class="qa-main">
        <div class="qa-session-strip">
          <div class="qa-session-tabs">
            {#each sessions as session (session.session_id)}
              <button type="button" class="qa-session-tab" class:active={activeId === session.session_id} onclick={() => selectSession(session)} disabled={busy}>
                <span class="qa-dot"></span>{session.connection_name}
              </button>
            {/each}
            {#if sessions.length === 0}<span class="qa-muted">No active SFTP sessions</span>{/if}
          </div>
          <button class="qa-button qa-danger" type="button" onclick={closeAllSessions} disabled={busy || sessions.length === 0}>Close all</button>
        </div>

        {#if activeSession}
          <div class="qa-info">
            <div><label>Host</label><strong>{activeSession.user}@{activeSession.host}:{activeSession.port}</strong></div>
            <div><label>Route</label><strong>{activeSession.jump_names.length ? activeSession.jump_names.join(" → ") : "Direct"}</strong></div>
            <div><label>Session ID</label><code>{activeSession.session_id}</code></div>
          </div>
          {#if activeSession.agent_binding === "legacy_unbound"}
            <div class="qa-warning" role="note">
              This Windows SSH agent rejected or closed the session-binding request. Authentication is continuing in legacy unbound mode.
              This session is not cryptographically bound to the destination. Do not rely on destination constraints; use an agent that successfully supports session binding when those restrictions are required.
            </div>
          {:else if activeSession.agent_binding === "bound"}
            <div class="qa-binding-ok"><span class="qa-dot"></span> SSH agent is bound to the verified server session.</div>
          {/if}
          <div class="qa-path-row">
            <button class="qa-button" type="button" onclick={() => directory?.parent_path && navigate(directory.parent_path)} disabled={busy || !directory?.parent_path}>↑ Up</button>
            <form class="qa-path-form" onsubmit={(event) => { event.preventDefault(); void navigate(pathInput); }}>
              <input class="qa-input qa-path" aria-label="Remote POSIX path" bind:value={pathInput} spellcheck="false" />
              <button class="qa-button" type="submit" disabled={busy || !pathInput.trim()}>Go</button>
            </form>
            <button class="qa-button" type="button" onclick={() => activeSession && navigate(directory?.current_path ?? activeSession.current_path)} disabled={busy}>↻</button>
            <button class="qa-button qa-danger" type="button" onclick={() => activeSession && closeSession(activeSession)} disabled={busy}>Close session</button>
          </div>
          <div class="qa-directory-title">
            <h3>Remote directory</h3>
            <span>{directory?.current_path ?? pathInput}</span>
            {#if directory}<small>{directory.entries.length} item(s)</small>{/if}
          </div>
          <div class="qa-directory">
            {#if busy && !directory}
              <p class="qa-empty">Loading remote directory…</p>
            {:else if !directory}
              <p class="qa-empty">Directory listing is not available.</p>
            {:else if directory.entries.length === 0}
              <p class="qa-empty">This directory is empty.</p>
            {:else}
              <table>
                <thead><tr><th>Name</th><th>Type</th><th class="qa-size">Size</th><th class="qa-modified">Modified</th></tr></thead>
                <tbody>
                  {#each directory.entries as entry (entry.path)}
                    <tr>
                      <td>
                        {#if entry.kind === "directory"}
                          <button type="button" class="qa-entry-link" onclick={() => navigate(entry.path)} disabled={busy}>▰ {entry.name}</button>
                        {:else}
                          <span class="qa-file-name">▤ {entry.name}</span>
                        {/if}
                      </td>
                      <td>{entry.kind}</td>
                      <td class="qa-size">{entry.kind === "directory" ? "—" : formatSize(entry.size)}</td>
                      <td class="qa-modified">{modifiedLabel(entry)}</td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            {/if}
          </div>
        {:else}
          <div class="qa-no-session">
            <div>↔</div><h3>No SFTP session selected</h3>
            <p>Select a saved connection and open an SFTP session. File transfers are intentionally outside this M8b test panel.</p>
          </div>
        {/if}

        {#if staleSessionIds.length > 0}
          <div class="qa-stale-check">
            <span>{staleSessionIds.length} session ID(s) captured before the previous vault lock.</span>
            <button class="qa-button qa-primary" type="button" onclick={verifyVaultCleanup} disabled={busy}>Verify vault-lock cleanup</button>
          </div>
        {/if}
        {#if errorMessage}<div class="qa-error" role="alert">{errorMessage}</div>{/if}
        <details class="qa-log" open>
          <summary>Activity log <span>{events.length} events</span></summary>
          <div class="qa-log-list">
            {#each events.slice().reverse() as entry, index (entry.time + "-" + index)}
              <div class:success={entry.kind === "success"} class:error={entry.kind === "error"}>
                <time>{entry.time}</time><span>{entry.message}</span>
              </div>
            {/each}
            {#if events.length === 0}<p class="qa-empty">SFTP actions and errors will appear here.</p>{/if}
          </div>
        </details>
      </main>
    </div>

    <footer class="qa-footer">
      <span><i class="qa-dot"></i> Runtime-only sessions should be invalidated when the vault locks.</span>
      <div class="qa-actions">
        <button class="qa-button qa-danger" type="button" onclick={lockVault} disabled={busy}>Lock vault and close SFTP</button>
        <button class="qa-button" type="button" onclick={onclose}>Done</button>
      </div>
    </footer>
  </section>
</div>

<style>
  .qa-overlay { position: fixed; inset: 0; z-index: 8000; display: grid; place-items: center; padding: 20px; box-sizing: border-box; background: rgb(4 7 12 / 70%); backdrop-filter: blur(3px); }
  .qa-panel { display: flex; flex-direction: column; width: min(1120px, 100%); height: min(760px, 100%); min-height: 440px; overflow: hidden; color: var(--text); background: var(--surface); border: 1px solid var(--border-strong); border-radius: 9px; box-shadow: 0 24px 80px #0008; }
  .qa-header, .qa-footer { display: flex; flex: 0 0 auto; align-items: center; justify-content: space-between; gap: 14px; padding: 12px 16px; }
  .qa-header { border-bottom: 1px solid var(--border); }
  .qa-eyebrow { color: var(--warning); font-size: var(--ui-font-meta); font-weight: 700; letter-spacing: .08em; }
  h2, h3, p { margin: 0; }
  .qa-header h2 { margin-top: 2px; font-size: var(--ui-font-title); }
  .qa-header p { margin-top: 3px; color: var(--muted); font-size: var(--ui-font-control); }
  .qa-actions { display: flex; align-items: center; gap: 7px; }
  .qa-close { width: 30px; height: 30px; border: 1px solid var(--border); border-radius: 5px; background: var(--surface-raised); color: var(--text); font-size: 21px; cursor: pointer; }
  .qa-body { display: grid; flex: 1; min-height: 0; grid-template-columns: 270px minmax(0, 1fr); }
  .qa-sidebar { display: flex; min-height: 0; flex-direction: column; gap: 9px; padding: 13px; border-right: 1px solid var(--border); background: var(--surface-deep); }
  .qa-section-title, .qa-directory-title { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  h3 { font-size: var(--ui-font-section); font-weight: 650; }
  .qa-section-title span, .qa-muted, .qa-directory-title small { color: var(--muted); font-size: var(--ui-font-meta); }
  .qa-sidebar label, .qa-info label { color: var(--muted); font-size: var(--ui-font-meta); }
  .qa-input { width: 100%; min-width: 0; box-sizing: border-box; padding: 7px 9px; border: 1px solid var(--border-strong); border-radius: 5px; background: var(--surface-raised); color: var(--text); font: inherit; font-size: var(--ui-font-control); }
  input:focus-visible, button:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .qa-connection-list { display: flex; flex: 1; min-height: 0; flex-direction: column; gap: 5px; overflow: auto; }
  .qa-connection { display: flex; flex-direction: column; align-items: flex-start; gap: 2px; padding: 8px; border: 1px solid transparent; border-radius: 5px; background: transparent; color: var(--text); text-align: left; cursor: pointer; }
  .qa-connection:hover { background: var(--surface-hover); }
  .qa-connection.selected { border-color: var(--accent); background: color-mix(in srgb, var(--accent) 12%, var(--surface)); }
  .qa-connection strong { font-size: var(--ui-font-body); }
  .qa-connection span, .qa-connection small { max-width: 100%; overflow: hidden; color: var(--muted); font-size: var(--ui-font-meta); text-overflow: ellipsis; white-space: nowrap; }
  .qa-connection small { color: var(--text-secondary); }
  .qa-main { display: flex; min-width: 0; min-height: 0; flex-direction: column; gap: 9px; padding: 12px 14px; }
  .qa-session-strip, .qa-session-tabs, .qa-path-row, .qa-path-form { display: flex; align-items: center; gap: 6px; min-width: 0; }
  .qa-session-tabs { flex: 1; overflow-x: auto; }
  .qa-session-tab { display: inline-flex; flex: 0 0 auto; align-items: center; gap: 7px; padding: 6px 9px; border: 1px solid var(--border); border-radius: 5px; background: var(--surface-deep); color: var(--text-secondary); font: inherit; font-size: var(--ui-font-control); cursor: pointer; }
  .qa-session-tab.active { border-color: var(--accent); background: var(--surface-hover); color: var(--text); }
  .qa-dot { display: inline-block; width: 7px; height: 7px; flex: 0 0 7px; border-radius: 50%; background: var(--success); }
  .qa-info { display: grid; grid-template-columns: 1fr 1fr 1.2fr; gap: 9px; padding: 9px; border: 1px solid var(--border); border-radius: 6px; background: var(--surface-deep); }
  .qa-info > div { display: flex; min-width: 0; flex-direction: column; gap: 2px; }
  .qa-info strong, .qa-info code { overflow: hidden; color: var(--text-secondary); font-size: var(--ui-font-control); text-overflow: ellipsis; white-space: nowrap; }
  .qa-path-form { flex: 1; }
  .qa-path { font-family: Consolas, monospace; }
  .qa-directory-title { gap: 10px; }
  .qa-directory-title > span { flex: 1; min-width: 0; overflow: hidden; color: var(--muted); font-family: Consolas, monospace; font-size: var(--ui-font-meta); text-overflow: ellipsis; white-space: nowrap; }
  .qa-directory { flex: 1; min-height: 80px; overflow: auto; border: 1px solid var(--border); border-radius: 6px; background: var(--surface-deep); }
  table { width: 100%; border-collapse: collapse; font-size: var(--ui-font-control); }
  th { position: sticky; top: 0; padding: 7px 9px; border-bottom: 1px solid var(--border); background: var(--surface-raised); color: var(--muted); text-align: left; }
  td { padding: 6px 9px; border-bottom: 1px solid color-mix(in srgb, var(--border) 65%, transparent); color: var(--text-secondary); }
  tr:last-child td { border-bottom: 0; }
  tbody tr:hover { background: var(--surface-hover); }
  td:first-child { max-width: 380px; color: var(--text); overflow-wrap: anywhere; }
  .qa-size { width: 80px; text-align: right; white-space: nowrap; }
  .qa-modified { width: 170px; white-space: nowrap; color: var(--muted); }
  .qa-entry-link, .qa-file-name { display: inline-flex; gap: 7px; border: 0; padding: 0; background: transparent; color: inherit; font: inherit; text-align: left; overflow-wrap: anywhere; }
  .qa-entry-link { color: var(--accent); cursor: pointer; }
  .qa-entry-link:hover { text-decoration: underline; }
  .qa-no-session { display: flex; flex: 1; flex-direction: column; align-items: center; justify-content: center; gap: 9px; padding: 24px; text-align: center; }
  .qa-no-session > div { display: grid; width: 46px; height: 46px; place-items: center; border: 1px solid var(--border); border-radius: 50%; color: var(--accent); font-size: 25px; }
  .qa-no-session p { max-width: 400px; color: var(--muted); font-size: var(--ui-font-control); line-height: 1.5; }
  .qa-empty { padding: 13px 8px; color: var(--muted); font-size: var(--ui-font-control); }
  .qa-stale-check { display: flex; align-items: center; justify-content: space-between; gap: 10px; padding: 7px 9px; border: 1px solid var(--success-border); border-radius: 5px; background: var(--success-soft); color: var(--text-secondary); font-size: var(--ui-font-meta); }
  .qa-warning { padding: 8px 10px; border: 1px solid var(--warning-border); border-radius: 5px; background: var(--warning-bg); color: var(--warning); font-size: var(--ui-font-control); line-height: 1.45; }
  .qa-binding-ok { display: flex; align-items: center; gap: 8px; padding: 5px 2px; color: var(--success); font-size: var(--ui-font-meta); }
  .qa-error { padding: 7px 9px; border: 1px solid var(--danger); border-radius: 5px; background: var(--danger-bg); color: var(--danger); font-size: var(--ui-font-control); overflow-wrap: anywhere; }
  .qa-log { flex: 0 0 96px; min-height: 0; overflow: hidden; border: 1px solid var(--border); border-radius: 5px; background: var(--surface-deep); }
  .qa-log summary { padding: 6px 9px; color: var(--text-secondary); font-size: var(--ui-font-control); font-weight: 600; cursor: pointer; }
  .qa-log summary span { margin-left: 6px; color: var(--muted); font-weight: 400; }
  .qa-log-list { max-height: 66px; overflow: auto; padding: 0 9px 5px; }
  .qa-log-list > div { display: flex; gap: 9px; padding: 2px 0; font-size: var(--ui-font-meta); }
  .qa-log-list time { flex: 0 0 67px; color: var(--muted); }
  .qa-log-list span { overflow-wrap: anywhere; color: var(--text-secondary); }
  .qa-log-list .success span { color: var(--success); }
  .qa-log-list .error span { color: var(--danger); }
  .qa-footer { border-top: 1px solid var(--border); background: var(--surface-deep); }
  .qa-footer > span { display: flex; align-items: center; gap: 8px; color: var(--muted); font-size: var(--ui-font-meta); }
  .qa-button { padding: 6px 9px; border: 1px solid var(--border-strong); border-radius: 5px; background: var(--surface-raised); color: var(--text); font: inherit; font-size: var(--ui-font-control); font-weight: 600; white-space: nowrap; cursor: pointer; }
  .qa-button:hover:not(:disabled), .qa-close:hover { background: var(--surface-hover); }
  .qa-button:disabled { opacity: .45; cursor: not-allowed; }
  .qa-primary { border-color: var(--accent); background: var(--accent); color: var(--accent-contrast); }
  .qa-danger { border-color: color-mix(in srgb, var(--danger) 55%, var(--border)); background: transparent; color: var(--danger); }
  @media (max-width: 760px) {
    .qa-overlay { padding: 7px; }
    .qa-panel { height: 100%; }
    .qa-body { grid-template-columns: 205px minmax(0, 1fr); }
    .qa-info { grid-template-columns: 1fr; }
    .qa-modified { display: none; }
    .qa-footer > span { display: none; }
  }
</style>
