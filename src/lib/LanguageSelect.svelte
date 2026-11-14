<script lang="ts">
  import { i18n, locales, setLocale, t } from "$lib/i18n/index.svelte";
</script>

<label class="lang">
  {t("language.label")}
  <select value={i18n.locale} onchange={(e) => setLocale(e.currentTarget.value)}>
    {#each locales as l (l.code)}
      <option value={l.code}>{l.name}</option>
    {/each}
  </select>
</label>

<style>
  .lang {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.85rem;
    color: #8d97a8;
    white-space: nowrap;
  }
  select {
    font: inherit;
    color: #dfe4ec;
    background: #1f2530;
    border: 1px solid #2f3847;
    border-radius: 4px;
    padding: 0.3rem 0.5rem;
  }
  select:focus-visible {
    outline: 2px solid #8fb4e8;
    outline-offset: 2px;
  }
</style>
