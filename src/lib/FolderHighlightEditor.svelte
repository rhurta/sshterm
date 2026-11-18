<script lang="ts">
  import { onMount, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "$lib/i18n/index.svelte";
  import type { HighlightRuleSetStatus, TreeNode } from "$lib/types";

  let {
    folder,
    oncancel,
    onsave,
  }: {
    folder: Extract<TreeNode, { type: "folder" }>;
    oncancel: () => void;
    onsave: (id: string, highlight: string[]) => Promise<string | null>;
  } = $props();

  let ruleSets = $state<HighlightRuleSetStatus[]>([]);
  let selected = $state<string[]>([]);
  let query = $state("");
  let searchInput = $state<HTMLInputElement | null>(null);
  let error = $state("");
  let saving = $state(false);
  let filtered = $derived.by(() => {
    const needle = query.trim().toLocaleLowerCase();
    return ruleSets.filter((item) => !needle || item.name.toLocaleLowerCase().includes(needle) || item.file.toLocaleLowerCase().includes(needle));
  });

  onMount(async () => {
    selected = [...(folder.highlight ?? [])];
    try { ruleSets = await invoke<HighlightRuleSetStatus[]>("highlight_list"); }
    catch (e) { error = String(e); }
    await tick();
    searchInput?.focus();
  });

  function toggle(name: string) {
    selected = selected.includes(name) ? selected.filter((item) => item !== name) : [...selected, name];
  }

  async function save() {
    if (saving) return;
    saving = true;
    error = "";
    try {
      const failure = await onsave(folder.id, selected);
      if (failure) error = failure;
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }
</script>

<div class="backdrop">
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="folder-highlight-title" tabindex="-1" onkeydown={(event) => { if (event.key === "Escape") oncancel(); }}>
    <div class="head">
      <h2 id="folder-highlight-title">{t("folder.highlightTitle", { name: folder.name })}</h2>
      <button class="close" aria-label={t("workspace.close")} onclick={oncancel}>×</button>
    </div>
    <p class="hint">{t("folder.highlightHint")}</p>
    <input bind:this={searchInput} bind:value={query} type="search" placeholder={t("editor.highlightSearch")} aria-label={t("editor.highlightSearch")} autocomplete="off" />
    <div class="list">
      {#each filtered as item (item.file)}
        <label class="choice">
          <input type="checkbox" checked={selected.includes(item.name)} disabled={!item.valid} onchange={() => toggle(item.name)} />
          <span class="rule-name">{item.name} <small>({item.rule_count})</small></span>
          {#if !item.valid}<small class="error">{item.error}</small>{/if}
        </label>
      {:else}
        <p class="hint">{query.trim() ? t("editor.highlightNoMatches") : t("editor.highlightNone")}</p>
      {/each}
    </div>
    <p class="hint">{t("folder.highlightInheritance")}</p>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <div class="actions">
      <button class="primary" disabled={saving} onclick={save}>{t("editor.save")}</button>
      <button class="ghost" onclick={oncancel}>{t("editor.cancel")}</button>
    </div>
  </div>
</div>

<style>
  .backdrop { position:fixed; inset:0; z-index:40; display:grid; place-items:center; padding:1rem; background:rgba(8,11,16,.72); }
  .dialog { width:min(28rem,94vw); max-height:82vh; display:flex; flex-direction:column; gap:.65rem; padding:1.1rem; box-sizing:border-box; overflow:hidden; background:var(--surface-raised); border:1px solid var(--border); border-radius:8px; color:var(--text); }
  .head,.actions { display:flex; align-items:center; justify-content:space-between; gap:.5rem; }
  h2 { margin:0; font-size:1rem; font-weight:550; }
  .hint { margin:0; color:var(--muted); font-size:.76rem; line-height:1.45; }
  input[type="search"] { width:100%; box-sizing:border-box; padding:.45rem .55rem; color:var(--text); background:var(--app-bg); border:1px solid var(--border); border-radius:4px; font:inherit; font-size:.8rem; }
  input[type="search"]:focus { outline:none; border-color:var(--accent); }
  .list { min-height:4rem; max-height:42vh; overflow-y:auto; display:flex; flex-direction:column; gap:.4rem; }
  .choice { display:grid; grid-template-columns:auto 1fr; gap:.35rem .55rem; align-items:center; font-size:.8rem; }
  .choice input { accent-color:var(--accent); }
  .rule-name small { color:var(--muted); }
  .choice .error { grid-column:2; }
  .error { margin:0; color:var(--danger); font-size:.75rem; }
  button { font:inherit; font-size:.78rem; padding:.4rem .65rem; border-radius:4px; border:1px solid var(--border); background:var(--surface-hover); color:var(--text); cursor:pointer; }
  button:hover:not(:disabled) { border-color:var(--accent); }
  button:disabled { opacity:.55; cursor:default; }
  button.primary { color:var(--accent-contrast); background:var(--accent); border-color:var(--accent); }
  button.ghost,button.close { background:transparent; }
  button.close { border:0; font-size:1.15rem; padding:.1rem .35rem; }
</style>
