<script lang="ts">
  import { tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "$lib/i18n/index.svelte";
  import ConnectionEditor from "./ConnectionEditor.svelte";
  import type { Connection, ConnectionSummary, Tunnel } from "$lib/types";

  let { connectionId, focusToken, width, onsave }: {
    connectionId: string | null;
    focusToken: number;
    width: number;
    onsave: (connection: Connection) => Promise<string | null>;
  } = $props();

  let connection = $state<Connection | null>(null);
  let summaries = $state<ConnectionSummary[]>([]);
  let query = $state("");
  let loading = $state(false);
  let error = $state("");
  let editing = $state(false);
  let searchInput = $state<HTMLInputElement | null>(null);
  let version = 0;
  let lastFocusToken = 0;

  async function load(id: string | null = connectionId) {
    const current = ++version;
    if (!id) {
      connection = null;
      summaries = [];
      loading = false;
      error = "";
      editing = false;
      query = "";
      return;
    }
    loading = true;
    error = "";
    try {
      const [full, all] = await Promise.all([
        invoke<Connection | null>("conn_get", { id }),
        invoke<ConnectionSummary[]>("conn_list"),
      ]);
      if (current !== version) return;
      connection = full;
      summaries = all;
      if (!full) error = t("properties.missing");
    } catch (e) {
      if (current === version) error = String(e);
    } finally {
      if (current === version) loading = false;
    }
  }

  $effect(() => {
    const id = connectionId;
    editing = false;
    query = "";
    void load(id);
  });

  $effect(() => {
    if (focusToken !== lastFocusToken) {
      lastFocusToken = focusToken;
      void tick().then(() => {
        searchInput?.focus();
        searchInput?.select();
      });
    }
  });

  const rows = $derived.by((): Array<[string, string]> => {
    const current = connection;
    if (!current) return [];
    const values: Array<[string, string]> = [];
    const byId = new Map(summaries.map((item) => [item.id, item]));

    values.push([t("properties.id"), current.id]);
    values.push([t("properties.name"), current.name]);
    values.push([t("properties.host"), current.host]);
    values.push([t("properties.port"), String(current.port)]);
    values.push([t("properties.username"), current.user]);

    if (current.auth.type === "password") {
      values.push([t("properties.authMethod"), t("properties.auth.password")]);
      values.push([t("properties.passwordState"), current.auth.password ? t("properties.saved") : t("properties.notSaved")]);
    } else if (current.auth.type === "key") {
      values.push([t("properties.authMethod"), t("properties.auth.key")]);
      values.push([t("properties.sshKey"), current.auth.path || t("properties.none")]);
      values.push([t("properties.keyPassphrase"), current.auth.passphrase ? t("properties.saved") : t("properties.notSaved")]);
    } else {
      values.push([t("properties.authMethod"), t("properties.auth.agent")]);
    }

    const jumpHosts = current.jump.map((id) => {
      const hop = byId.get(id);
      const name = hop?.name ?? id;
      return current.jump_disabled.includes(id) ? name + " (" + t("editor.disabled") + ")" : name;
    });
    values.push([t("properties.jumpHosts"), jumpHosts.join(" → ") || t("properties.none")]);
    values.push([t("properties.keepalive"), String(current.keepalive_secs) + " s"]);
    values.push([t("properties.protected"), current.protected ? t("common.yes") : t("common.no")]);

    const forwards: Array<[string, string]> = current.tunnels.map((tunnel: Tunnel, index) => {
      const inactive = tunnel.enabled ? "" : " · " + t("editor.disabled");
      if (tunnel.kind === "dynamic") {
        return [t("properties.forward", { index: index + 1 }), "SOCKS " + tunnel.bind + inactive];
      }
      const kind = tunnel.kind === "local" ? "Local" : "Remote";
      return [t("properties.forward", { index: index + 1 }), kind + " " + tunnel.bind + " → " + tunnel.dest + inactive];
    });
    values.push(...forwards);

    values.push([t("properties.colorForeground"), current.colors?.fg || t("properties.default")]);
    values.push([t("properties.colorBackground"), current.colors?.bg || t("properties.default")]);
    values.push([
      t("properties.sftp"),
      current.sftp
        ? t("properties.sftpPort") + ": " + current.sftp.port + "; " + t("properties.sftpStartDir") + ": " + (current.sftp.start_dir || t("properties.default"))
        : t("properties.default"),
    ]);
    values.push([t("properties.highlight"), current.highlight.join(", ") || t("properties.inherited")]);

    const needle = query.trim().toLocaleLowerCase();
    return values.filter(([label, value]) =>
      !needle || label.toLocaleLowerCase().includes(needle) || value.toLocaleLowerCase().includes(needle),
    );
  });

  function beginEdit() { editing = true; }
  function cancelEdit() { editing = false; }

  async function saveInline(next: Connection): Promise<string | null> {
    const failure = await onsave(next);
    if (!failure) {
      editing = false;
      await load(connectionId);
    }
    return failure;
  }
</script>

<aside class="properties-pane" aria-label={t("properties.title")} style:width={width + "px"}>
  <div class="properties-head workspace-panel-header">
    <strong class="panel-title">{t("properties.title")}</strong>
    {#if connection && !editing}
      <button class="edit-button" title={t("properties.edit")} aria-label={t("properties.edit")} onclick={beginEdit}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 20h4l11-11a2.1 2.1 0 0 0-4-4L4 16v4Z"/><path d="m13.5 6.5 4 4"/></svg>
      </button>
    {/if}
  </div>

  {#if editing && connection}
    <ConnectionEditor inline conn={connection} isNew={false} onsave={saveInline} oncancel={cancelEdit} />
  {:else}
    <div class="properties-search-wrap">
      <input bind:this={searchInput} bind:value={query} type="search" placeholder={t("properties.search")} aria-label={t("properties.searchLabel")} />
    </div>
    {#if loading}
      <p class="state">{t("properties.loading")}</p>
    {:else if error}
      <p class="state error" role="alert">{error}</p>
    {:else if !connection}
      <p class="state">{t("properties.selectConnection")}</p>
    {:else if !rows.length}
      <p class="state">{t("properties.noMatches", { query })}</p>
    {:else}
      <div class="property-grid">
        {#each rows as row, index (index)}
          <div class="property-label">{row[0]}</div>
          <div class="property-value" title={row[1]}>{row[1]}</div>
        {/each}
      </div>
    {/if}
  {/if}
</aside>

<style>
  .properties-pane {
    flex: 0 0 auto;
    min-width: 250px;
    max-width: 450px;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--surface);
    border-right: 1px solid var(--border);
    box-sizing: border-box;
    font-family: var(--ui-font-family, "Segoe UI", system-ui, sans-serif);
    font-size: var(--ui-font-body, 13px);
  }
  .properties-head { justify-content: space-between; }
  .edit-button {
    width: 28px;
    height: 28px;
    flex: 0 0 28px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    color: var(--muted);
    background: transparent;
    border: 1px solid transparent;
    border-radius: 4px;
    cursor: pointer;
  }
  .edit-button:hover { color: var(--accent); background: var(--surface-hover); border-color: var(--border); }
  .edit-button svg { width: 16px; height: 16px; fill: none; stroke: currentColor; stroke-width: 1.7; stroke-linecap: round; stroke-linejoin: round; }
  .properties-search-wrap { flex: 0 0 auto; padding: 0.5rem; background: var(--surface); }
  .properties-search-wrap input {
    width: 100%;
    min-height: 28px;
    box-sizing: border-box;
    padding: 0.3rem 0.5rem;
    color: var(--text);
    background: var(--surface-deep);
    border: 1px solid var(--border);
    border-radius: 4px;
    font: inherit;
    font-size: var(--ui-font-control, 12px);
  }
  .properties-search-wrap input:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .property-grid {
    min-height: 0;
    overflow: auto;
    display: grid;
    grid-template-columns: minmax(96px, 100px) minmax(0, 1fr);
    align-content: start;
    gap: 3px 4px;
    padding: 0.5rem;
    font-size: var(--ui-font-dense, 12px);
    line-height: 1.4;
  }
  .property-label {
    min-width: 0;
    padding: 0.38rem 0.4rem;
    display: flex;
    align-items: center;
    color: var(--muted);
    background: color-mix(in srgb, var(--surface-raised) 45%, transparent);
    border-radius: 3px 0 0 3px;
    overflow-wrap: anywhere;
  }
  .property-value {
    min-width: 0;
    padding: 0.38rem 0.45rem;
    color: var(--text);
    background: var(--surface-deep);
    border: 1px solid var(--border);
    border-radius: 0 3px 3px 0;
    overflow-wrap: anywhere;
    user-select: text;
  }
  .state { margin: 0; padding: 0.75rem; color: var(--muted); font-size: var(--ui-font-meta, 11px); }
  .state.error { color: var(--danger); }
</style>
