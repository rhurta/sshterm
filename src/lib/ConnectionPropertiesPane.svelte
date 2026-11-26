<script lang="ts">
  import { tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { hasMessage, t } from "$lib/i18n/index.svelte";
  import ConnectionEditor from "./ConnectionEditor.svelte";
  import type { Connection, ConnectionSummary, HighlightRuleSetStatus, TreeNode, Tunnel } from "$lib/types";
  import { IDENTITY_COLOR_PRESETS } from "$lib/identityColors";

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

  let { selectedNode, focusToken, width, onsave, oncolorschange }: {
    selectedNode: TreeNode | null;
    focusToken: number;
    width: number;
    onsave: (connection: Connection) => Promise<string | null>;
    oncolorschange: (connections: ConnectionSummary[]) => void;
  } = $props();

  let connection = $state<Connection | null>(null);
  let summaries = $state<ConnectionSummary[]>([]);
  let treeNodes = $state<TreeNode[]>([]);
  let highlightRuleSets = $state<HighlightRuleSetStatus[]>([]);
  let highlightMenuOpen = $state(false);
  let colorSaving = $state(false);
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
  let passwordVisible = $state(false);
  let version = 0;
  let lastFocusToken = 0;

  async function loadSelection(node: TreeNode | null = selectedNode) {
    const current = ++version;
    connection = null;
    summaries = [];
    treeNodes = [];
    highlightMenuOpen = false;
    if (!node) {
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
      if (node.type === "folder") {
        const [nodes, sets] = await Promise.all([invoke<TreeNode[]>("tree_get"), invoke<HighlightRuleSetStatus[]>("highlight_list")]);
        if (current !== version) return;
        treeNodes = nodes;
        highlightRuleSets = sets;
      } else {
        const [full, all, nodes, sets] = await Promise.all([
          invoke<Connection | null>("conn_get", { id: node.id }),
          invoke<ConnectionSummary[]>("conn_list"),
          invoke<TreeNode[]>("tree_get"),
          invoke<HighlightRuleSetStatus[]>("highlight_list"),
        ]);
        if (current !== version) return;
        connection = full;
        summaries = all;
        treeNodes = nodes;
        highlightRuleSets = sets;
        if (!full) error = t("properties.missing");
      }
    } catch (e) {
      if (current === version) error = String(e);
    } finally {
      if (current === version) loading = false;
    }
  }

  type FolderContext = { folder: Extract<TreeNode, { type: "folder" }>; inheritedColor: string | null };
  function findFolderContext(nodes: TreeNode[], id: string, inheritedColor: string | null = null): FolderContext | null {
    for (const node of nodes) {
      if (node.type !== "folder") continue;
      if (node.id === id) return { folder: node, inheritedColor };
      const nested = findFolderContext(node.children, id, node.color ?? inheritedColor);
      if (nested) return nested;
    }
    return null;
  }

  const selectedFolderContext = $derived.by(() =>
    selectedNode?.type === "folder" ? findFolderContext(treeNodes, selectedNode.id) : null,
  );
  const selectedSummary = $derived(
    connection ? summaries.find((item) => item.id === connection!.id) ?? null : null,
  );
  const explicitIdentityColor = $derived(
    selectedNode?.type === "folder" ? selectedFolderContext?.folder.color ?? null : connection?.identity_color ?? null,
  );
  const effectiveIdentityColor = $derived(
    selectedNode?.type === "folder"
      ? selectedFolderContext?.folder.color ?? selectedFolderContext?.inheritedColor ?? null
      : connection?.identity_color ?? selectedSummary?.identity_color ?? null,
  );

  async function setIdentityColor(color: string | null) {
    const node = selectedNode;
    if (!node || colorSaving) return;
    colorSaving = true;
    inlineError = "";
    try {
      await invoke(node.type === "folder" ? "folder_set_color" : "conn_set_identity_color", { id: node.id, color });
      const all = await invoke<ConnectionSummary[]>("conn_list");
      oncolorschange(all);
      await loadSelection(node);
    } catch (cause) {
      inlineError = String(cause);
    } finally {
      colorSaving = false;
    }
  }

  $effect(() => {
    const node = selectedNode;
    editing = false;
    activeField = null;
    inlineError = "";
    passwordVisible = false;
    query = "";
    void loadSelection(node);
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
    if (!current || selectedNode?.type !== "connection") return [];
    const values: PropertyRow[] = [];
    const byId = new Map(summaries.map((item) => [item.id, item]));

    values.push({ key: "id", label: t("properties.id"), value: current.id });
    values.push({ key: "name", label: t("properties.name"), value: current.name, field: "name", inputType: "text", editValue: current.name });
    values.push({ key: "host", label: t("properties.host"), value: current.host, field: "host", inputType: "text", editValue: current.host });
    values.push({ key: "port", label: t("properties.port"), value: String(current.port), field: "port", inputType: "number", editValue: String(current.port) });
    values.push({ key: "user", label: t("properties.username"), value: current.user, field: "user", inputType: "text", editValue: current.user });

    if (current.auth.type === "password") {
      values.push({ key: "authMethod", label: t("properties.authMethod"), value: t("properties.auth.password") });
      values.push({
        key: "password",
        label: t("properties.passwordState"),
        value: current.auth.password ? "••••••••" : t("properties.notSaved"),
      });
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

    const needle = query.trim().toLocaleLowerCase();
    return values.filter(({ label, value }) =>
      !needle || label.toLocaleLowerCase().includes(needle) || value.toLocaleLowerCase().includes(needle),
    );
  });

  const selectedHighlights = $derived(
    selectedNode?.type === "folder"
      ? selectedFolderContext?.folder.highlight ?? []
      : connection?.highlight ?? [],
  );

  async function setHighlightSelected(name: string, checked: boolean) {
    const node = selectedNode;
    if (!node) return;
    const next = checked
      ? [...selectedHighlights, name]
      : selectedHighlights.filter((item) => item !== name);
    highlightMenuOpen = true;
    inlineError = "";
    try {
      if (node.type === "folder") {
        await invoke("folder_set_highlight", { id: node.id, highlight: next });
        const all = await invoke<ConnectionSummary[]>("conn_list");
        oncolorschange(all);
        await loadSelection(node);
        highlightMenuOpen = true;
      } else if (connection) {
        await persistDirect({ ...connection, highlight: next });
      }
    } catch (cause) {
      inlineError = String(cause);
    }
  }

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
      await loadSelection();
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
      await loadSelection();
    }
    return failure;
  }
</script>

{#snippet highlightEditor()}
  <div class="property-value highlight-combobox">
    <button
      type="button"
      class="highlight-trigger"
      aria-haspopup="listbox"
      aria-expanded={highlightMenuOpen}
      disabled={loading || savingInline}
      onclick={() => (highlightMenuOpen = !highlightMenuOpen)}
    >
      <span>{selectedHighlights.length ? selectedHighlights.join(", ") : t("properties.inherited")}</span>
      <span class="highlight-chevron">▾</span>
    </button>
    {#if highlightMenuOpen}
      <div class="highlight-options" role="group" aria-label={t("properties.highlight")}>
        {#each highlightRuleSets as item (item.file)}
          <label class="highlight-option">
            <input
              type="checkbox"
              checked={selectedHighlights.includes(item.name)}
              disabled={!item.valid || savingInline}
              onchange={(event) => void setHighlightSelected(item.name, event.currentTarget.checked)}
            />
            <span>{item.name}</span>
            <small>{item.rule_count}</small>
          </label>
        {:else}
          <span class="highlight-empty">{t("editor.highlightNone")}</span>
        {/each}
      </div>
    {/if}
  </div>
{/snippet}

{#snippet colorEditor()}
  <div class="property-value identity-color-value">
    <div class="color-swatches">
      {#each IDENTITY_COLOR_PRESETS as color}
        <button
          class="color-swatch"
          class:selected={effectiveIdentityColor?.toLowerCase() === color.toLowerCase()}
          style={`--swatch: ${color}`}
          title={color}
          aria-label={`${t("properties.identityColor")}: ${color}`}
          disabled={colorSaving}
          onclick={() => void setIdentityColor(color)}
        ></button>
      {/each}
    </div>
    <div class="custom-color-row">
      <input
        type="color"
        aria-label={t("properties.identityColor")}
        value={effectiveIdentityColor ?? "#61afef"}
        disabled={colorSaving}
        onchange={(event) => void setIdentityColor(event.currentTarget.value)}
      />
      <button type="button" class="clear-color" disabled={colorSaving || !explicitIdentityColor} onclick={() => void setIdentityColor(null)}>{t("tree.identityColorClear")}</button>
    </div>
    <small>{explicitIdentityColor ? t("properties.colorCustom") : effectiveIdentityColor ? t("properties.colorInherited") : t("properties.colorNone")}</small>
  </div>
{/snippet}

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
    {:else if !connection && selectedNode?.type !== "folder"}
      <p class="state">{t("properties.selectConnection")}</p>
    {:else if selectedNode?.type === "folder" && selectedFolderContext}
      <div class="property-grid">
        <div class="property-label">{t("properties.id")}</div><div class="property-value">{selectedFolderContext.folder.id}</div>
        <div class="property-label">{t("properties.name")}</div><div class="property-value">{selectedFolderContext.folder.name}</div>
        <div class="property-label">{t("properties.folderChildren")}</div><div class="property-value">{selectedFolderContext.folder.children.length}</div>
        <div class="property-label">{t("properties.highlight")}</div>{@render highlightEditor()}
        <div class="property-label">{t("properties.highlight")}</div>{@render highlightEditor()}
        <div class="property-label">{t("properties.identityColor")}</div>{@render colorEditor()}
      </div>
    {:else if !rows.length}
      <p class="state">{t("properties.noMatches", { query })}</p>
    {:else}
      <div class="property-grid">
        {#each rows as row (row.key)}
          <div class="property-label">{row.label}</div>
          {#if row.key === "password" && connection?.auth.type === "password" && connection?.auth.password}
            <div class="property-value password-value">
              <span class="password-text">{passwordVisible ? (connection?.auth.type === "password" ? connection.auth.password : "") : "••••••••"}</span>
              <button
                class="password-reveal"
                type="button"
                aria-pressed={passwordVisible}
                aria-label={passwordVisible ? t("password.hide") : t("password.show")}
                title={passwordVisible ? t("password.hide") : t("password.show")}
                onclick={() => (passwordVisible = !passwordVisible)}
              >
                {#if passwordVisible}
                  <svg viewBox="0 0 24 24" aria-hidden="true">
                    <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94" />
                    <path d="M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19" />
                    <path d="M14.12 14.12a3 3 0 1 1-4.24-4.24" />
                    <path d="M1 1l22 22" />
                  </svg>
                {:else}
                  <svg viewBox="0 0 24 24" aria-hidden="true">
                    <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z" />
                    <circle cx="12" cy="12" r="3" />
                  </svg>
                {/if}
              </button>
            </div>
          {:else if row.key === "password"}
            <div class="property-value">{row.value}</div>
          {:else if row.field === "protected"}
            <label class="property-value boolean-value">
              <input type="checkbox" checked={connection?.protected ?? false} onchange={(event) => void setProtected(event.currentTarget.checked)} disabled={savingInline} />
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
        <div class="property-label">{t("properties.identityColor")}</div>{@render colorEditor()}
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
  .highlight-combobox { position: relative; padding: 0; }
  .highlight-trigger { width: 100%; min-width: 0; display: flex; align-items: center; justify-content: space-between; gap: .35rem; padding: .38rem .45rem; color: var(--text); background: transparent; border: 0; font: inherit; text-align: left; cursor: pointer; }
  .highlight-trigger span:first-child { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .highlight-chevron { color: var(--muted); flex: 0 0 auto; }
  .highlight-options { position: absolute; z-index: 20; top: calc(100% + 3px); left: 0; right: 0; max-height: 12rem; overflow-y: auto; padding: .3rem; display: flex; flex-direction: column; gap: 2px; background: var(--surface-raised); border: 1px solid var(--border); border-radius: 5px; box-shadow: 0 5px 16px rgba(0,0,0,.25); }
  .highlight-option { display: flex; align-items: center; gap: .4rem; padding: .3rem .35rem; border-radius: 3px; cursor: pointer; }
  .highlight-option:hover { background: var(--surface-hover); }
  .highlight-option input { accent-color: var(--accent); }
  .highlight-option small { margin-left: auto; color: var(--muted); }
  .highlight-empty { padding: .4rem; color: var(--muted); }
  .identity-color-value { display: flex; flex-direction: column; gap: 0.4rem; }
  .color-swatches { display: grid; grid-template-columns: repeat(4, minmax(18px, 1fr)); gap: 6px; }
  .color-swatch { width: 20px; height: 20px; justify-self: center; border-radius: 50%; border: 1px solid color-mix(in srgb, var(--text) 35%, transparent); background: var(--swatch); cursor: pointer; padding: 0; }
  .color-swatch:hover, .color-swatch.selected { outline: 2px solid var(--text); outline-offset: 1px; }
  .color-swatch:disabled { opacity: 0.5; cursor: default; }
  .custom-color-row { display: flex; align-items: center; gap: 0.4rem; }
  .custom-color-row input { width: 32px; height: 26px; padding: 1px; border: 1px solid var(--border); border-radius: 4px; background: var(--surface-deep); cursor: pointer; }
  .clear-color { flex: 1; min-width: 0; padding: 0.25rem 0.35rem; color: var(--text-secondary); background: var(--surface-hover); border: 1px solid var(--border); border-radius: 3px; font: inherit; font-size: var(--ui-font-meta); cursor: pointer; }
  .clear-color:hover:not(:disabled) { border-color: var(--accent); color: var(--text); }
  .clear-color:disabled { opacity: 0.45; cursor: default; }
  .identity-color-value small { color: var(--muted); font-size: var(--ui-font-meta); }
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
  .password-value {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.35rem;
  }
  .password-text {
    min-width: 0;
    overflow-wrap: anywhere;
    user-select: text;
  }
  .password-reveal {
    flex: 0 0 24px;
    width: 24px;
    height: 24px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    color: var(--muted);
    background: transparent;
    border: 1px solid transparent;
    border-radius: 3px;
    cursor: pointer;
  }
  .password-reveal:hover {
    color: var(--accent);
    background: var(--surface-hover);
    border-color: var(--border);
  }
  .password-reveal:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .password-reveal svg {
    width: 16px;
    height: 16px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
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
