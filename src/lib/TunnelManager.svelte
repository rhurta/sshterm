<script lang="ts">
  import { onMount, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { hasMessage, t } from "$lib/i18n/index.svelte";
  import type { ConnectionSummary, TunnelInfo } from "$lib/types";

  let {
    onclose,
    onedit,
  }: {
    onclose: () => void;
    onedit: (connection: ConnectionSummary) => void;
  } = $props();

  let connections = $state<ConnectionSummary[]>([]);
  let statuses = $state<Record<string, TunnelInfo>>({});
  let query = $state("");
  let searchInput = $state<HTMLInputElement | null>(null);
  let busy = $state<Record<string, boolean>>({});
  let bulkBusy = $state(false);
  let error = $state("");
  let loading = $state(true);
  let statusEventVersions: Record<string, number> = {};

  const configured = $derived(connections.filter((connection) => connection.tunnel_count > 0));
  const filtered = $derived.by(() => {
    const needle = query.trim().toLocaleLowerCase();
    if (!needle) return configured;
    return configured.filter((connection) =>
      [connection.name, connection.host, connection.user, `${connection.user}@${connection.host}:${connection.port}`]
        .some((value) => value.toLocaleLowerCase().includes(needle)),
    );
  });
  const running = $derived(Object.values(statuses).filter((item) =>
    item.status === "starting" || item.status === "active" || item.status === "stopping",
  ));
  const canStartAll = $derived(configured.some((connection) => {
    const status = statuses[connection.id]?.status;
    return status !== "starting" && status !== "active" && status !== "stopping";
  }));

  function errorText(value: string): string {
    const [code, ...detailParts] = value.split(":");
    const key = `error.${code.trim()}`;
    const base = hasMessage(key) ? t(key) : value;
    const detail = detailParts.join(":").trim();
    return detail ? `${base} — ${detail}` : base;
  }

  function applyStatus(value: TunnelInfo, fromEvent = false) {
    if (fromEvent) {
      const id = value.connection_id;
      statusEventVersions[id] = (statusEventVersions[id] ?? 0) + 1;
    }
    statuses = { ...statuses, [value.connection_id]: value };
  }

  async function refresh() {
    loading = true;
    error = "";
    try {
      const eventVersions = { ...statusEventVersions };
      const [items, active] = await Promise.all([
        invoke<ConnectionSummary[]>("conn_list"),
        invoke<TunnelInfo[]>("tunnel_list"),
      ]);
      connections = items;
      const next = { ...statuses };
      for (const item of active) {
        // Do not overwrite a newer event that arrived while the list request
        // was in flight.
        if ((statusEventVersions[item.connection_id] ?? 0) === (eventVersions[item.connection_id] ?? 0)) {
          next[item.connection_id] = item;
        }
      }
      statuses = next;
    } catch (e) {
      error = errorText(String(e));
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    let mounted = true;
    let unlisten: (() => void) | undefined;
    void listen<TunnelInfo>("tunnel:status", (event) => {
      if (mounted) applyStatus(event.payload, true);
    }).then((stop) => {
      unlisten = stop;
      if (!mounted) {
        stop();
        return;
      }
      void refresh();
    }).catch((e) => {
      error = String(e);
      void refresh();
    });
    void tick().then(() => searchInput?.focus());
    return () => {
      mounted = false;
      unlisten?.();
    };
  });

  async function start(connection: ConnectionSummary) {
    if (busy[connection.id] || bulkBusy) return;
    busy = { ...busy, [connection.id]: true };
    error = "";
    try {
      const eventVersion = statusEventVersions[connection.id] ?? 0;
      const result = await invoke<TunnelInfo>("tunnel_start", { connectionId: connection.id });
      if ((statusEventVersions[connection.id] ?? 0) === eventVersion) applyStatus(result);
    } catch (e) {
      error = `${connection.name}: ${errorText(String(e))}`;
    } finally {
      busy = { ...busy, [connection.id]: false };
    }
  }

  async function stop(connection: ConnectionSummary) {
    if (busy[connection.id] || bulkBusy) return;
    busy = { ...busy, [connection.id]: true };
    error = "";
    try {
      await invoke("tunnel_stop", { connectionId: connection.id });
    } catch (e) {
      error = `${connection.name}: ${errorText(String(e))}`;
    } finally {
      busy = { ...busy, [connection.id]: false };
    }
  }

  async function startAll() {
    if (bulkBusy) return;
    bulkBusy = true;
    error = "";
    try {
      for (const connection of configured) {
        const status = statuses[connection.id]?.status;
        if (status === "starting" || status === "active" || status === "stopping") continue;
        try {
          const eventVersion = statusEventVersions[connection.id] ?? 0;
          const result = await invoke<TunnelInfo>("tunnel_start", { connectionId: connection.id });
          if ((statusEventVersions[connection.id] ?? 0) === eventVersion) applyStatus(result);
        } catch (e) {
          error = error ? `${error}\n${connection.name}: ${errorText(String(e))}` : `${connection.name}: ${errorText(String(e))}`;
        }
      }
    } finally {
      bulkBusy = false;
    }
  }

  async function stopAll() {
    if (bulkBusy || running.length === 0) return;
    bulkBusy = true;
    error = "";
    try {
      await invoke("tunnel_stop_all");
    } catch (e) {
      error = errorText(String(e));
    } finally {
      bulkBusy = false;
    }
  }

  function statusLabel(connectionId: string): string {
    return t(`tunnels.status.${statuses[connectionId]?.status ?? "stopped"}`);
  }

  function isRunning(connectionId: string): boolean {
    const status = statuses[connectionId]?.status;
    return status === "starting" || status === "active" || status === "stopping";
  }

  function closeFromKeyboard(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onclose();
    }
  }
</script>

<div class="backdrop">
  <button class="scrim" type="button" tabindex="-1" aria-label={t("workspace.close")} onclick={onclose}></button>
  <section
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="tunnel-manager-title"
    tabindex="-1"
    onkeydown={closeFromKeyboard}
  >
    <div class="head">
      <div>
        <h2 id="tunnel-manager-title">{t("tunnels.title")}</h2>
        <p class="hint">{t("tunnels.hint")}</p>
      </div>
      <button class="close" aria-label={t("workspace.close")} title={t("workspace.close")} onclick={onclose}>×</button>
    </div>

    <div class="toolbar">
      <input
        bind:this={searchInput}
        bind:value={query}
        type="search"
        placeholder={t("tunnels.search")}
        aria-label={t("tunnels.search")}
        autocomplete="off"
        spellcheck="false"
      />
      <button class="primary" disabled={!canStartAll || bulkBusy || loading} onclick={startAll}>
        {t("tunnels.startAll")}
      </button>
      <button class="ghost" disabled={running.length === 0 || bulkBusy} onclick={stopAll}>
        {t("tunnels.stopAll")}
      </button>
    </div>

    {#if error}
      <p class="error" role="alert">{error}</p>
    {/if}

    <div class="list" aria-live="polite">
      {#if loading}
        <p class="hint">{t("tunnels.loading")}</p>
      {:else if configured.length === 0}
        <p class="hint">{t("tunnels.noneConfigured")}</p>
      {:else if filtered.length === 0}
        <p class="hint">{t("tunnels.noMatches")}</p>
      {:else}
        {#each filtered as connection (connection.id)}
          {@const tunnel = statuses[connection.id]}
          <article class="connection">
            <div class="connection-head">
              <div class="identity">
                <strong>{connection.name}</strong>
                <span class="detail">{connection.user}@{connection.host}:{connection.port}</span>
                {#if tunnel?.jump_names.length}
                  <span class="route">{t("workspace.jumpVia", { route: tunnel.jump_names.join(" → ") })}</span>
                {/if}
              </div>
              <span class="status" class:active={tunnel?.status === "active"} class:failed={tunnel?.status === "failed"} class:stopping={tunnel?.status === "stopping"} class:starting={tunnel?.status === "starting"}>
                {statusLabel(connection.id)}
              </span>
              <div class="actions">
                <button class="primary" disabled={busy[connection.id] || bulkBusy || isRunning(connection.id)} onclick={() => start(connection)}>
                  {t("tunnels.start")}
                </button>
                <button class="ghost" disabled={busy[connection.id] || bulkBusy || !isRunning(connection.id)} onclick={() => stop(connection)}>
                  {t("tunnels.stop")}
                </button>
                <button class="ghost" disabled={busy[connection.id] || bulkBusy} onclick={() => onedit(connection)}>
                  {t("workspace.edit")}
                </button>
              </div>
            </div>

            <div class="forwards">
              {#if tunnel}
                {#each tunnel.forwards as forward (forward.index)}
                  <div class="forward">
                    <span class="kind">{t(`editor.tunnel.${forward.kind}`)}</span>
                    <code>{forward.bind}{forward.destination ? ` → ${forward.destination}` : ""}</code>
                    <span class="forward-status" class:active={forward.status === "active"} class:failed={forward.status === "failed"}>
                      {t(`tunnels.forwardStatus.${forward.status}`)}
                    </span>
                    {#if forward.error}<span class="forward-error">{forward.error}</span>{/if}
                  </div>
                {/each}
                {#if tunnel.error}
                  <p class="tunnel-error">{tunnel.error}</p>
                {/if}
              {:else}
                <p class="hint">{t("tunnels.notStarted", { count: connection.tunnel_count })}</p>
              {/if}
            </div>
          </article>
        {/each}
      {/if}
    </div>

    <div class="foot">
      <span class="hint">{t("tunnels.activeNote")}</span>
      <button class="ghost" onclick={onclose}>{t("editor.cancel")}</button>
    </div>
  </section>
</div>

<style>
  .backdrop { position:fixed; inset:0; z-index:36; display:grid; place-items:center; padding:1rem; background:rgba(8,11,16,.72); }
  .scrim { position:absolute; inset:0; border:0; padding:0; background:transparent; cursor:default; }
  .dialog { position:relative; z-index:1; width:min(58rem,96vw); max-height:90vh; display:flex; flex-direction:column; gap:.7rem; padding:1.1rem; box-sizing:border-box; overflow:hidden; background:var(--surface-raised); border:1px solid var(--border); border-radius:8px; color:var(--text); }
  .head,.toolbar,.connection-head,.actions,.foot { display:flex; align-items:center; gap:.55rem; }
  .head { justify-content:space-between; }
  h2 { margin:0 0 .2rem; font-size:1rem; font-weight:600; }
  .hint { margin:0; color:var(--muted); font-size:.75rem; line-height:1.45; }
  .toolbar input { flex:1; min-width:8rem; box-sizing:border-box; padding:.45rem .55rem; color:var(--text); background:var(--app-bg); border:1px solid var(--border); border-radius:4px; font:inherit; font-size:.8rem; }
  input:focus { outline:none; border-color:var(--accent); }
  button { flex:0 0 auto; font:inherit; font-size:.76rem; padding:.38rem .62rem; border-radius:4px; border:1px solid var(--border); background:var(--surface-hover); color:var(--text); cursor:pointer; }
  button:hover:not(:disabled) { border-color:var(--accent); }
  button:disabled { opacity:.45; cursor:default; }
  button.primary { color:var(--accent-contrast); background:var(--accent); border-color:var(--accent); }
  button.ghost,button.close { background:transparent; }
  button.close { font-size:1.25rem; line-height:1; padding:.15rem .45rem; }
  .list { min-height:5rem; overflow-y:auto; display:flex; flex-direction:column; gap:.55rem; padding-right:.2rem; }
  .connection { padding:.7rem; border:1px solid var(--border); border-radius:5px; background:var(--surface); }
  .connection-head { align-items:flex-start; flex-wrap:wrap; }
  .identity { min-width:12rem; flex:1 1 14rem; display:flex; flex-direction:column; gap:.15rem; }
  .identity strong { font-size:.82rem; }
  .detail,.route { color:var(--muted); font-size:.7rem; overflow-wrap:anywhere; }
  .route { color:var(--accent); }
  .status { flex:0 0 auto; padding:.16rem .38rem; border-radius:3px; border:1px solid var(--border); color:var(--text-secondary); font-size:.65rem; text-transform:uppercase; letter-spacing:.03em; }
  .status.active { color:#78bfa5; border-color:#397b65; }
  .status.failed { color:var(--danger); border-color:var(--danger); background:var(--danger-bg); }
  .status.starting,.status.stopping { color:var(--warning,#d6a85f); }
  .actions { margin-left:auto; flex-wrap:wrap; }
  .forwards { display:flex; flex-direction:column; gap:.25rem; margin-top:.55rem; padding-top:.45rem; border-top:1px solid var(--border); }
  .forward { display:grid; grid-template-columns:7rem minmax(0,1fr) auto; gap:.3rem .55rem; align-items:center; font-size:.7rem; }
  .kind { color:var(--muted); }
  code { min-width:0; color:var(--text); overflow-wrap:anywhere; font-size:.68rem; }
  .forward-status { color:var(--muted); font-size:.64rem; }
  .forward-status.active { color:#78bfa5; }
  .forward-status.failed,.forward-error,.tunnel-error { color:var(--danger); }
  .forward-error { grid-column:2 / -1; overflow-wrap:anywhere; font-size:.67rem; }
  .tunnel-error { margin:0; padding-top:.2rem; overflow-wrap:anywhere; font-size:.7rem; }
  .error { margin:0; padding:.5rem .6rem; color:var(--danger); background:var(--danger-bg); border-radius:4px; font-size:.75rem; white-space:pre-wrap; overflow-wrap:anywhere; }
  .foot { justify-content:space-between; padding-top:.45rem; border-top:1px solid var(--border); }
  .foot .hint { flex:1; }
  @media (max-width: 700px) {
    .toolbar { flex-wrap:wrap; }
    .toolbar input { flex-basis:100%; }
    .forward { grid-template-columns:1fr auto; }
    .forward code,.forward-error { grid-column:1 / -1; }
    .actions { margin-left:0; width:100%; }
  }
</style>
