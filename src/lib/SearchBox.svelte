<script lang="ts">
  import { onMount, tick } from "svelte";
  import { t } from "$lib/i18n/index.svelte";

  let {
    value,
    count,
    onchange,
    autofocus = false,
  }: {
    value: string;
    count: number | null;
    onchange: (value: string) => void;
    autofocus?: boolean;
  } = $props();
  let input = $state<HTMLInputElement | null>(null);
  onMount(() => { if (autofocus) void tick().then(() => { input?.focus(); input?.select(); }); });
</script>

<div class="box">
  <input
    bind:this={input}
    type="search"
    placeholder={t("tree.search")}
    {value}
    oninput={(event) => onchange(event.currentTarget.value)}
    autocomplete="off"
    spellcheck="false"
  />
  {#if value}
    <button class="clear" onclick={() => onchange("")} aria-label={t("editor.cancel")}>×</button>
    <span class="count">{t("tree.matches", { count: count ?? 0 })}</span>
  {/if}
</div>

<style>
  .box {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0 0.65rem 0.5rem;
  }
  input {
    flex: 1;
    min-width: 0;
    font: inherit;
    font-size: var(--ui-font-body);
    color: var(--text);
    background: var(--app-bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.4rem 0.5rem;
  }
  input:focus {
    outline: none;
    border-color: var(--accent);
  }
  .clear {
    font: inherit;
    font-size: var(--ui-font-section);
    color: var(--muted);
    background: transparent;
    border: 0;
    cursor: pointer;
    padding: 0 0.15rem;
  }
  .count {
    font-size: var(--ui-font-meta);
    color: var(--muted);
    white-space: nowrap;
  }
</style>
