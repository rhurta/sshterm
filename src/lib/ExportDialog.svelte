<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { homeDir, join } from "@tauri-apps/api/path";
  import { save } from "@tauri-apps/plugin-dialog";
  import { hasMessage, t } from "$lib/i18n/index.svelte";

  let { onclose }: { onclose: () => void } = $props();

  let format = $state<"json" | "ssh_config">("json");
  let busy = $state(false);
  let error = $state("");
  let exportedCount = $state<number | null>(null);
  let skippedCount = $state(0);
  let completedFormat = $state<"json" | "ssh_config" | null>(null);

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape" && !busy) {
      event.preventDefault();
      onclose();
    }
  }

  function errorText(value: string): string {
    const key = "error." + value;
    return hasMessage(key) ? t(key) : t("lock.error.unexpected", { detail: value });
  }

  async function runExport() {
    busy = true;
    error = "";
    try {
      const defaultPath = format === "json"
        ? "sshterm-connections.json"
        : await join(await homeDir(), ".ssh", "config");
      const path = await save({
        title: t("export.saveDialogTitle"),
        defaultPath,
        filters: format === "json"
          ? [{ name: t("export.json"), extensions: ["json"] }]
          : [{ name: t("export.sshConfig"), extensions: ["config"] }],
      });
      if (!path) return;

      const command = format === "json" ? "conn_export" : "conn_export_ssh_config";
      const report = await invoke<{ connections: number; skipped: number }>(command, { path });
      exportedCount = report.connections;
      skippedCount = report.skipped;
      completedFormat = format;
    } catch (cause) {
      error = errorText(String(cause));
    } finally {
      busy = false;
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="backdrop">
  <button class="scrim" tabindex="-1" aria-label={t("export.cancel")} onclick={() => !busy && onclose()}></button>
  <section class="dialog" role="alertdialog" aria-modal="true" aria-labelledby="export-title">
    <header>
      <h2 id="export-title">{t("export.title")}</h2>
    </header>

    {#if exportedCount === null}
      <label class="format">
        <span>{t("export.format")}</span>
        <select bind:value={format} disabled={busy}>
          <option value="json">{t("export.json")}</option>
          <option value="ssh_config">{t("export.sshConfig")}</option>
        </select>
      </label>
      <p class="warning">{format === "json" ? t("export.warning") : t("export.sshConfigWarning")}</p>
      {#if error}
        <p class="error" role="alert">{error}</p>
      {/if}
      <div class="actions">
        <button class="secondary" onclick={onclose} disabled={busy}>{t("export.cancel")}</button>
        <button onclick={runExport} disabled={busy}>{busy ? t("console.sending") : t("export.save")}</button>
      </div>
    {:else}
      <p class="success" role="status">
        {completedFormat === "json"
          ? t("export.complete", { count: exportedCount })
          : t("export.sshConfigComplete", { count: exportedCount, skipped: skippedCount })}
      </p>
      <div class="actions">
        <button onclick={onclose}>{t("export.done")}</button>
      </div>
    {/if}
  </section>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 31;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(8, 11, 16, 0.7);
  }
  .scrim {
    position: absolute;
    inset: 0;
    padding: 0;
    border: 0;
    background: transparent;
    cursor: default;
  }
  .dialog {
    position: relative;
    z-index: 1;
    display: flex;
    flex-direction: column;
    gap: 0.8rem;
    width: min(34rem, 92vw);
    padding: 1.25rem;
    box-sizing: border-box;
    color: var(--text);
    background: var(--surface-raised);
    border: 1px solid var(--border-strong);
    border-radius: 8px;
    box-shadow: 0 12px 42px color-mix(in srgb, var(--c-app-bg) 45%, transparent);
  }
  h2 {
    margin: 0;
    font-size: var(--ui-font-title);
    font-weight: 600;
  }
  p {
    margin: 0;
    font-size: var(--ui-font-body);
    line-height: 1.5;
  }
  .format {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    color: var(--text-secondary);
    font-size: var(--ui-font-body);
  }
  select {
    width: 100%;
    padding: 0.45rem 0.55rem;
    font: inherit;
    color: var(--text);
    background: var(--app-bg);
    border: 1px solid var(--border);
    border-radius: 4px;
  }
  .warning {
    padding: 0.75rem;
    color: var(--warning);
    background: color-mix(in srgb, var(--warning) 10%, var(--surface-raised));
    border: 1px solid color-mix(in srgb, var(--warning) 40%, var(--border));
    border-radius: 5px;
  }
  .error { color: var(--danger); }
  .success { color: var(--success); }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }
  button {
    padding: 0.45rem 0.8rem;
    font: inherit;
    font-size: var(--ui-font-body);
    font-weight: 500;
    color: var(--accent-contrast);
    background: var(--accent);
    border: 0;
    border-radius: 4px;
    cursor: pointer;
  }
  button.secondary {
    color: var(--text-secondary);
    background: transparent;
    border: 1px solid var(--border);
  }
  button:disabled { opacity: 0.5; cursor: default; }
</style>
