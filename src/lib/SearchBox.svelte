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
    padding: 0.5rem;
    background: var(--surface);
  }
  input {
    flex: 1;
    min-width: 0;
    min-height: 28px;
    box-sizing: border-box;
    font: inherit;
    font-size: var(--ui-font-control, 12px);
    color: var(--text);
    background: var(--surface-deep);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.3rem 0.5rem;
  }
  input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
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
