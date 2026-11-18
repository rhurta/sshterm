<script lang="ts">
  import { onMount, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "$lib/i18n/index.svelte";
  import type { HighlightRuleSetStatus, HighlightTestResult } from "$lib/types";

  let { onclose }: { onclose: () => void } = $props();
  let ruleSets = $state<HighlightRuleSetStatus[]>([]);
  let selectedSet = $state("");
  let query = $state("");
  let searchInput = $state<HTMLInputElement | null>(null);
  let sample = $state("admin@db-01:~$ sudo systemctl status sshd\n10.20.30.40:22 connection established\nERROR: authentication failed");
  let result = $state<HighlightTestResult | null>(null);
  let error = $state("");
  let loading = $state(false);
  let filtered = $derived.by(() => {
    const needle = query.trim().toLocaleLowerCase();
    return ruleSets.filter((item) => !needle || item.name.toLocaleLowerCase().includes(needle) || item.file.toLocaleLowerCase().includes(needle));
  });

  onMount(async () => {
    try {
      ruleSets = await invoke<HighlightRuleSetStatus[]>("highlight_list");
      selectedSet = ruleSets.find((item) => item.valid)?.name ?? "";
    } catch (e) {
      error = String(e);
    }
    await tick();
    searchInput?.focus();
  });

  async function test() {
    if (!selectedSet || loading) return;
    loading = true;
    error = "";
    result = null;
    try { result = await invoke<HighlightTestResult>("highlight_test", { setName: selectedSet, sample }); }
    catch (e) { error = String(e); }
    finally { loading = false; }
  }
</script>

<div class="backdrop">
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="highlight-panel-title" tabindex="-1" onkeydown={(event) => { if (event.key === "Escape") onclose(); }}>
    <div class="head">
      <h2 id="highlight-panel-title">{t("highlight.panelTitle")}</h2>
      <button class="close" aria-label={t("workspace.close")} onclick={onclose}>×</button>
    </div>
    <p class="hint">{t("highlight.panelHint")}</p>
    <input bind:this={searchInput} bind:value={query} type="search" placeholder={t("editor.highlightSearch")} aria-label={t("editor.highlightSearch")} />
    <div class="sets">
      {#each filtered as item (item.file)}
        <button class:selected={selectedSet === item.name} disabled={!item.valid} aria-pressed={selectedSet === item.name} onclick={() => { selectedSet = item.name; result = null; }}>
          <span>{item.name} ({item.rule_count})</span>
          {#if !item.valid}<small>{item.error}</small>{/if}
        </button>
      {:else}
        <p class="hint">{t("editor.highlightNone")}</p>
      {/each}
    </div>
    <label class="sample-label" for="highlight-sample">{t("highlight.sample")}</label>
    <textarea id="highlight-sample" bind:value={sample} spellcheck="false" rows="6"></textarea>
    <div class="actions">
      <button class="primary" disabled={!selectedSet || loading} onclick={test}>{loading ? t("highlight.testing") : t("highlight.test")}</button>
      <button class="ghost" onclick={onclose}>{t("editor.cancel")}</button>
    </div>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    {#if result}
      <div class="results">
        <strong>{t("highlight.resultTitle")}</strong>
        <p class="hint">{result.prompt_matches ? t("highlight.promptDetected") : t("highlight.promptNotDetected")}</p>
        {#each result.matches as match, index (`${match.rule_name}-${match.start}-${index}`)}
          <div class="match">
            <span class="swatch" style={`background:${match.bg ?? "transparent"};color:${match.fg ?? "var(--text)"}`}>Aa</span>
            <span>{match.rule_name} · {match.scope}</span>
            <code>{Array.from((sample.split(/\r\n|\r|\n/)[match.line] ?? "")).slice(match.start, match.end).join("")}</code>
          </div>
        {:else}
          <p class="hint">{t("highlight.noMatches")}</p>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .backdrop { position:fixed; inset:0; z-index:35; display:grid; place-items:center; padding:1rem; background:rgba(8,11,16,.72); }
  .dialog { width:min(34rem,94vw); max-height:88vh; overflow-y:auto; display:flex; flex-direction:column; gap:.6rem; padding:1.1rem; box-sizing:border-box; background:var(--surface-raised); border:1px solid var(--border); border-radius:8px; color:var(--text); }
  .head,.actions { display:flex; align-items:center; justify-content:space-between; gap:.5rem; }
  h2 { margin:0; font-size:1rem; font-weight:550; }
  .hint { margin:0; color:var(--muted); font-size:.76rem; line-height:1.45; }
  input[type="search"],textarea { width:100%; box-sizing:border-box; padding:.45rem .55rem; color:var(--text); background:var(--app-bg); border:1px solid var(--border); border-radius:4px; font:inherit; font-size:.8rem; }
  input:focus,textarea:focus { outline:none; border-color:var(--accent); }
  .sets { display:flex; flex-wrap:wrap; gap:.35rem; max-height:7rem; overflow-y:auto; }
  button { font:inherit; font-size:.78rem; padding:.4rem .65rem; border-radius:4px; border:1px solid var(--border); background:var(--surface-hover); color:var(--text); cursor:pointer; }
  button:hover:not(:disabled) { border-color:var(--accent); }
  button:disabled { opacity:.55; cursor:default; }
  button.selected { border-color:var(--accent); background:var(--surface-deep); }
  button.primary { color:var(--accent-contrast); background:var(--accent); border-color:var(--accent); }
  button.ghost,button.close { background:transparent; }
  button.close { border:0; font-size:1.15rem; padding:.1rem .35rem; }
  .sets button { display:flex; flex-direction:column; text-align:left; }
  .sets small { color:var(--danger); font-size:.68rem; }
  .sample-label { color:var(--text-secondary); font-size:.78rem; }
  textarea { resize:vertical; font-family:Consolas,monospace; line-height:1.45; }
  .error { margin:0; color:var(--danger); font-size:.75rem; }
  .results { display:flex; flex-direction:column; gap:.4rem; padding-top:.45rem; border-top:1px solid var(--border); }
  .match { display:grid; grid-template-columns:2rem 1fr auto; gap:.5rem; align-items:center; font-size:.75rem; }
  .swatch { display:grid; place-items:center; width:1.8rem; height:1.35rem; border:1px solid var(--border); border-radius:3px; font-weight:700; }
  code { max-width:12rem; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; padding:.15rem .3rem; background:var(--surface-deep); border-radius:3px; }
</style>
