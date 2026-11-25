<script lang="ts">
  import { tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { hasMessage, t } from "$lib/i18n/index.svelte";
  import ConnectionEditor from "./ConnectionEditor.svelte";
  import type { Connection, ConnectionSummary, Tunnel } from "$lib/types";

  type EditableField =
    | "name"
    | "host"
    | "port"
    | "user"
    | "authPath"
    | "keepalive"
    | "protected"
    | "colorFg"
    | "colorBg"
    | "sftpPort"
    | "sftpStartDir";

  type PropertyRow = {
    key: string;
    label: string;
    value: string;
    field?: EditableField;
    inputType?: "text" | "number";
    editValue?: string;
  };

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
  let editInput = $state<HTMLInputElement | null>(null);
  let activeField = $state<EditableField | null>(null);
  let editValue = $state("");
  let savingInline = $state(false);
  let inlineError = $state("");
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
      activeField = null;
      inlineError = "";
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
    activeField = null;
    inlineError = "";
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

  const rows = $derived.by((): PropertyRow[] => {
    const current = connection;
    if (!current) return [];
    const values: PropertyRow[] = [];
    const byId = new Map(summaries.map((item) => [item.id, item]));

    values.push({ key: "id", label: t("properties.id"), value: current.id });
    values.push({ key: "name", label: t("properties.name"), value: current.name, field: "name", inputType: "text", editValue: current.name });
    values.push({ key: "host", label: t("properties.host"), value: current.host, field: "host", inputType: "text", editValue: current.host });
    values.push({ key: "port", label: t("properties.port"), value: String(current.port), field: "port", inputType: "number", editValue: String(current.port) });
    values.push({ key: "user", label: t("properties.username"), value: current.user, field: "user", inputType: "text", editValue: current.user });

    if (current.auth.type === "password") {
      values.push({ key: "authMethod", label: t("properties.authMethod"), value: t("properties.auth.password") });
      values.push({ key: "passwordState", label: t("properties.passwordState"), value: current.auth.password ? t("properties.saved") : t("properties.notSaved") });
    } else if (current.auth.type === "key") {
      values.push({ key: "authMethod", label: t("properties.authMethod"), value: t("properties.auth.key") });
      values.push({
        key: "sshKey",
        label: t("properties.sshKey"),
        value: current.auth.path || t("properties.none"),
        field: "authPath",
        inputType: "text",
        editValue: current.auth.path,
      });
      values.push({ key: "keyPassphrase", label: t("properties.keyPassphrase"), value: current.auth.passphrase ? t("properties.saved") : t("properties.notSaved") });
    } else {
      values.push({ key: "authMethod", label: t("properties.authMethod"), value: t("properties.auth.agent") });
    }

    const jumpHosts = current.jump.map((id) => {
      const hop = byId.get(id);
      const name = hop?.name ?? id;
      return current.jump_disabled.includes(id) ? name + " (" + t("editor.disabled") + ")" : name;
    });
    values.push({ key: "jumpHosts", label: t("properties.jumpHosts"), value: jumpHosts.join(" → ") || t("properties.none") });
    values.push({
      key: "keepalive",
      label: t("properties.keepalive"),
      value: String(current.keepalive_secs) + " s",
      field: "keepalive",
      inputType: "number",
      editValue: String(current.keepalive_secs),
    });
    values.push({
      key: "protected",
      label: t("properties.protected"),
      value: current.protected ? t("common.yes") : t("common.no"),
      field: "protected",
    });

    const forwards: PropertyRow[] = current.tunnels.map((tunnel: Tunnel, index) => {
      const inactive = tunnel.enabled ? "" : " · " + t("editor.disabled");
      if (tunnel.kind === "dynamic") {
        return { key: "forward-" + index, label: t("properties.forward", { index: index + 1 }), value: "SOCKS " + tunnel.bind + inactive };
      }
      const kind = tunnel.kind === "local" ? "Local" : "Remote";
      return { key: "forward-" + index, label: t("properties.forward", { index: index + 1 }), value: kind + " " + tunnel.bind + " → " + tunnel.dest + inactive };
    });
    values.push(...forwards);

    values.push({
      key: "colorFg",
      label: t("properties.colorForeground"),
      value: current.colors?.fg || t("properties.default"),
      field: "colorFg",
      inputType: "text",
      editValue: current.colors?.fg ?? "",
    });
    values.push({
      key: "colorBg",
      label: t("properties.colorBackground"),
      value: current.colors?.bg || t("properties.default"),
      field: "colorBg",
      inputType: "text",
      editValue: current.colors?.bg ?? "",
    });

    if (current.sftp) {
      values.push({
        key: "sftpPort",
        label: t("properties.sftpPort"),
        value: current.sftp.port == null ? t("properties.inheritSshPort") : String(current.sftp.port),
        field: "sftpPort",
        inputType: "number",
        editValue: current.sftp.port == null ? "" : String(current.sftp.port),
      });
      values.push({
        key: "sftpStartDir",
        label: t("properties.sftpStartDir"),
        value: current.sftp.start_dir || t("properties.default"),
        field: "sftpStartDir",
        inputType: "text",
        editValue: current.sftp.start_dir,
      });
    } else {
      values.push({ key: "sftp", label: t("properties.sftp"), value: t("properties.default") });
    }
    values.push({ key: "highlight", label: t("properties.highlight"), value: current.highlight.join(", ") || t("properties.inherited") });

    const needle = query.trim().toLocaleLowerCase();
    return values.filter(({ label, value }) =>
      !needle || label.toLocaleLowerCase().includes(needle) || value.toLocaleLowerCase().includes(needle),
    );
  });

  function errorText(code: string): string {
    const key = "error." + code;
    return hasMessage(key) ? t(key) : t("lock.error.unexpected", { detail: code });
  }

  function beginEdit() { editing = true; }
  function cancelEdit() { editing = false; }

  async function startCellEdit(row: PropertyRow) {
    if (!row.field || !row.inputType || savingInline) return;
    activeField = row.field;
    editValue = row.editValue ?? "";
    inlineError = "";
    await tick();
    editInput?.focus();
    editInput?.select();
  }

  function cancelCellEdit() {
    activeField = null;
    inlineError = "";
  }

  function onCellKeydown(event: KeyboardEvent) {
    if (event.key === "Enter") {
      event.preventDefault();
      event.stopPropagation();
      void commitCellEdit();
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      cancelCellEdit();
    }
  }

  async function persistDirect(next: Connection): Promise<boolean> {
    savingInline = true;
    inlineError = "";
    try {
      const failure = await onsave(next);
      if (failure) {
        inlineError = errorText(failure);
        return false;
      }
      activeField = null;
      await load(connectionId);
      return true;
    } catch (cause) {
      inlineError = String(cause);
      return false;
    } finally {
      savingInline = false;
    }
  }

  async function commitCellEdit() {
    const field = activeField;
    const current = connection;
    if (!field || !current || savingInline) return;

    const raw = editValue.trim();
    const next: Connection = { ...current };

    switch (field) {
      case "name":
        if (!raw) { inlineError = t("editor.validation"); return; }
        next.name = raw;
        break;
      case "host":
        if (!raw) { inlineError = t("editor.validation"); return; }
        next.host = raw;
        break;
      case "port": {
        const value = Number(raw);
        if (!Number.isInteger(value) || value < 1 || value > 65535) { inlineError = t("editor.validation"); return; }
        next.port = value;
        break;
      }
      case "user":
        if (!raw) { inlineError = t("editor.validation"); return; }
        next.user = raw;
        break;
      case "authPath":
        if (current.auth.type !== "key") return;
        next.auth = raw ? { ...current.auth, path: raw } : { type: "agent" };
        break;
      case "keepalive": {
        const value = Number(raw);
        if (!raw || !Number.isInteger(value) || value < 0 || value > 4294967295) { inlineError = t("editor.validation"); return; }
        next.keepalive_secs = value;
        break;
      }
      case "colorFg":
      case "colorBg": {
        if (raw && !/^#[0-9a-fA-F]{6}$/.test(raw)) { inlineError = "#RRGGBB"; return; }
        const fg = field === "colorFg" ? raw : (current.colors?.fg ?? "");
        const bg = field === "colorBg" ? raw : (current.colors?.bg ?? "");
        next.colors = fg || bg ? { fg, bg } : null;
        break;
      }
      case "sftpPort": {
        const value = raw ? Number(raw) : null;
        if (value !== null && (!Number.isInteger(value) || value < 1 || value > 65535)) { inlineError = t("editor.validationSftpPort"); return; }
        next.sftp = { ...(current.sftp ?? { port: null, start_dir: "" }), port: value };
        break;
      }
      case "sftpStartDir":
        next.sftp = { ...(current.sftp ?? { port: null, start_dir: "" }), start_dir: raw };
        break;
      case "protected":
        return;
    }

    await persistDirect(next);
  }

  async function setProtected(value: boolean) {
    if (!connection || savingInline) return;
    await persistDirect({ ...connection, protected: value });
  }

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
        {#each rows as row (row.key)}
          <div class="property-label">{row.label}</div>
          {#if row.field === "protected"}
            <label class="property-value boolean-value">
              <input type="checkbox" checked={connection.protected} onchange={(event) => void setProtected(event.currentTarget.checked)} disabled={savingInline} />
              <span>{row.value}</span>
            </label>
          {:else if row.field && activeField === row.field}
            <div class="property-value cell-editing">
              <input
                bind:this={editInput}
                bind:value={editValue}
                type={row.inputType ?? "text"}
                aria-label={row.label}
                onkeydown={onCellKeydown}
                disabled={savingInline}
              />
              <button class="cell-action" type="button" title={t("editor.save")} aria-label={t("editor.save")} onclick={() => void commitCellEdit()} disabled={savingInline}>✓</button>
              <button class="cell-action" type="button" title={t("editor.cancel")} aria-label={t("editor.cancel")} onclick={cancelCellEdit} disabled={savingInline}>×</button>
            </div>
          {:else if row.field}
            <button class="property-value editable" type="button" title={t("properties.edit")} onclick={() => void startCellEdit(row)} disabled={savingInline}>
              <span>{row.value}</span>
              <span class="edit-hint" aria-hidden="true">✎</span>
            </button>
          {:else}
            <div class="property-value" title={row.value}>{row.value}</div>
          {/if}
        {/each}
      </div>
      {#if inlineError}
        <p class="state error" role="alert">{inlineError}</p>
      {/if}
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
    width: 100%;
    padding: 0.38rem 0.45rem;
    box-sizing: border-box;
    color: var(--text);
    background: var(--surface-deep);
    border: 1px solid var(--border);
    border-radius: 0 3px 3px 0;
    overflow-wrap: anywhere;
    user-select: text;
    text-align: left;
    font: inherit;
    font-size: inherit;
  }
  button.property-value {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.3rem;
    cursor: text;
  }
  button.property-value:hover { border-color: var(--accent); background: var(--surface-hover); }
  .edit-hint { flex: 0 0 auto; color: var(--muted); opacity: 0; }
  button.property-value:hover .edit-hint,
  button.property-value:focus-visible .edit-hint { opacity: 1; }
  .cell-editing {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 2px;
  }
  .cell-editing input {
    width: 100%;
    min-width: 0;
    padding: 0.22rem 0.28rem;
    color: var(--text);
    background: var(--app-bg, var(--surface));
    border: 1px solid var(--accent);
    border-radius: 3px;
    font: inherit;
  }
  .cell-action {
    flex: 0 0 20px;
    width: 20px;
    height: 24px;
    padding: 0;
    color: var(--text-secondary);
    background: transparent;
    border: 1px solid transparent;
    border-radius: 3px;
    cursor: pointer;
  }
  .cell-action:hover { color: var(--accent); background: var(--surface-hover); border-color: var(--border); }
  .boolean-value {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    cursor: pointer;
  }
  .boolean-value input { accent-color: var(--accent); }
  .state { margin: 0; padding: 0.75rem; color: var(--muted); font-size: var(--ui-font-meta, 11px); }
  .state.error { color: var(--danger); }
</style>
