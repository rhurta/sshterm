<script lang="ts">
  import { t } from "$lib/i18n/index.svelte";

  let {
    value,
    count,
    onchange,
  }: {
    value: string;
    count: number | null;
    onchange: (value: string) => void;
  } = $props();
</script>

<div class="box">
  <input
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
    font-size: 0.8rem;
    color: #dfe4ec;
    background: #161a22;
    border: 1px solid #2f3847;
    border-radius: 4px;
    padding: 0.4rem 0.5rem;
  }
  input:focus {
    outline: none;
    border-color: #8fb4e8;
  }
  .clear {
    font: inherit;
    font-size: 0.9rem;
    color: #8d97a8;
    background: transparent;
    border: 0;
    cursor: pointer;
    padding: 0 0.15rem;
  }
  .count {
    font-size: 0.68rem;
    color: #8d97a8;
    white-space: nowrap;
  }
</style>
