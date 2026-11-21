<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { hasMessage, t } from "$lib/i18n/index.svelte";
  import type { ImportPreview, ImportReport } from "$lib/types";

  let {
    onclose,
    ondone,
  }: {
    onclose: () => void;
    ondone: (report: ImportReport) => void;
  } = $props();

  const sources = ["ssh_config", "putty", "winsshterm"] as const;
  let source = $state<string>("ssh_config");
  let path = $state("");
  let copyDuplicates = $state(false);
  let preview = $state<ImportPreview | null>(null);
  let result = $state<ImportReport | null>(null);
  let error = $state("");
  let busy = $state(false);

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
  }

  function lookup(prefix: string, code: string): string {
    const key = `${prefix}${code}`;
    return hasMessage(key) ? t(key) : code;
  }

  async function runPreview() {
    busy = true;
    error = "";
    result = null;
    try {
      preview = await invoke<ImportPreview>("import_preview", {
        source,
        path: path.trim() || null,
      });
    } catch (e) {
      preview = null;
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function runApply() {
    busy = true;
    error = "";
    try {
      result = await invoke<ImportReport>("import_apply", {
        source,
        path: path.trim() || null,
        parentId: null,
        policy: copyDuplicates ? "copy" : "skip",
      });
      ondone(result);
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="backdrop">
  <button
    class="scrim"
    tabindex="-1"
    aria-label={t("import.close")}
    onclick={onclose}
  ></button>
  <div class="dialog" role="dialog" aria-modal="true" aria-label={t("import.title")}>
    <div class="dialog-head">
      <h2>{t("import.title")}</h2>
      <button
        class="close"
        onclick={onclose}
        title={t("import.close")}
        aria-label={t("import.close")}
      >&times;</button>
    </div>

    <label>
      {t("import.source")}
      <select bind:value={source} onchange={() => { preview = null; result = null; }}>
        {#each sources as option}
          <option value={option}>{lookup("import.source.", option)}</option>
        {/each}
      </select>
    </label>

    {#if source !== "putty"}
      <label>
        {t("import.path")}
        <input bind:value={path} placeholder={t("import.pathHint")} autocomplete="off" spellcheck="false" />
      </label>
    {/if}

    <label class="check">
      <input type="checkbox" bind:checked={copyDuplicates} />
      {t("import.copy")}
    </label>

    {#if error}
      <p class="error" role="alert">{error}</p>
    {/if}

    {#if result}
      <p class="result">{t("import.result", { created: result.created, skipped: result.skipped.length })}</p>
      {#if result.folders_created > 0}
        <p class="result">{t("import.folders", { count: result.folders_created })}</p>
      {/if}
    {/if}

    {#if preview}
      <div class="preview">
        <h3>{t("import.candidates")} ({preview.candidates.length})</h3>
        {#if preview.candidates.length === 0}
          <p class="muted">{t("import.empty")}</p>
        {:else}
          <ul>
            {#each preview.candidates as candidate}
              <li>
                {#if candidate.folder_path.length > 0}
                  <span class="folder">{candidate.folder_path.join(" / ")} /</span>
                {/if}
                <span class="name">{candidate.name}</span>
                <span class="detail">{candidate.user}@{candidate.host}:{candidate.port}</span>
                {#each candidate.warnings as warning}
                  <span class="warning">{lookup("import.warning.", warning)}</span>
                {/each}
              </li>
            {/each}
          </ul>
        {/if}
        {#if preview.skipped.length > 0}
          <h3>{t("import.skipped")}</h3>
          <ul>
            {#each preview.skipped as skipped}
              <li><span class="name">{skipped.label}</span> — {lookup("import.reason.", skipped.reason)}</li>
            {/each}
          </ul>
        {/if}
        {#if preview.notices.length > 0}
          <h3>{t("import.warnings")}</h3>
          <ul>
            {#each preview.notices as notice}
              <li>{lookup("import.notice.", notice)}</li>
            {/each}
          </ul>
        {/if}
      </div>
    {/if}

    <div class="buttons">
      <button class="ghost" onclick={runPreview} disabled={busy}>{t("import.preview")}</button>
      <button
        onclick={runApply}
        disabled={busy || !preview || preview.candidates.length === 0}
      >
        {t("import.apply", { count: preview?.candidates.length ?? 0 })}
      </button>
      <button class="ghost" onclick={onclose}>{t("import.close")}</button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(8, 11, 16, 0.7);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 30;
  }
  .scrim {
    position: absolute;
    inset: 0;
    border: 0;
    padding: 0;
    background: transparent;
    cursor: default;
  }
  .dialog {
    position: relative;
    z-index: 1;
    width: min(30rem, 92vw);
    max-height: 88vh;
    overflow-y: auto;
    background: var(--surface-raised);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 1.25rem;
    display: flex;
    flex-direction: column;
    gap: 0.65rem;
    box-sizing: border-box;
  }
  .dialog-head {
    position: sticky;
    top: 0;
    z-index: 2;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    padding-bottom: 0.35rem;
    background: var(--surface-raised);
  }
  h2 {
    margin: 0;
    font-size: var(--ui-font-title);
    font-weight: 500;
  }
  button.close {
    font: inherit;
    font-size: 1.15rem;
    line-height: 1;
    width: 1.65rem;
    height: 1.65rem;
    flex: 0 0 auto;
    padding: 0;
    box-sizing: border-box;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-secondary);
    background: transparent;
    border: 1px solid transparent;
    border-radius: 4px;
    cursor: pointer;
  }
  button.close:hover {
    color: var(--text);
    background: var(--surface-hover);
    border-color: var(--border);
  }
  h3 {
    margin: 0.5rem 0 0.3rem;
    font-size: var(--ui-font-body);
    font-weight: 500;
    color: var(--accent);
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    font-size: var(--ui-font-body);
    color: var(--text-secondary);
  }
  label.check {
    flex-direction: row;
    align-items: center;
    gap: 0.45rem;
  }
  input,
  select {
    font: inherit;
    font-size: var(--ui-font-body);
    color: var(--text);
    background: var(--app-bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.4rem 0.55rem;
  }
  .preview {
    border-top: 1px solid var(--border);
    padding-top: 0.4rem;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    font-size: var(--ui-font-control);
  }
  li {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
    color: var(--text-secondary);
  }
  .name {
    color: var(--text);
  }
  .detail,
  .muted {
    color: var(--muted);
  }
  .folder {
    color: var(--accent);
  }
  .warning {
    color: var(--warning);
  }
  .error {
    margin: 0;
    font-size: var(--ui-font-body);
    color: var(--danger);
  }
  .result {
    margin: 0;
    font-size: var(--ui-font-body);
    color: var(--success);
  }
  .buttons {
    display: flex;
    gap: 0.5rem;
    margin-top: 0.4rem;
  }
  button {
    font: inherit;
    font-size: var(--ui-font-body);
    font-weight: 500;
    color: var(--accent-contrast);
    background: var(--accent);
    border: 0;
    border-radius: 4px;
    padding: 0.45rem 0.8rem;
    cursor: pointer;
  }
  button.ghost {
    background: transparent;
    color: var(--text-secondary);
    border: 1px solid var(--border);
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
