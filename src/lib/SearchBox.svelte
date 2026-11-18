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

  // Down moves into the results; Escape clears the filter.
  function onKeydown(event: KeyboardEvent) {
    if (event.key === "ArrowDown") {
      const row = document.querySelector<HTMLElement>('[data-nav="connection-tree"] button.row');
      if (row) {
        event.preventDefault();
        row.focus();
      }
    } else if (event.key === "Escape" && value) {
      event.preventDefault();
      onchange("");
    }
  }
</script>

<div class="box">
  <input
    type="search"
    placeholder={t("tree.search")}
    {value}
    oninput={(event) => onchange(event.currentTarget.value)}
    onkeydown={onKeydown}
    data-nav="connection-search"
    aria-label={t("tree.search")}
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
    font-size: 0.9rem;
    color: var(--muted);
    background: transparent;
    border: 0;
    cursor: pointer;
    padding: 0 0.15rem;
  }
  .count {
    font-size: 0.68rem;
    color: var(--muted);
    white-space: nowrap;
  }
</style>
