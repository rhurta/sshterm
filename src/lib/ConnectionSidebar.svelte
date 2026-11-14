<script lang="ts">
  import { t } from "$lib/i18n/index.svelte";
  import type { ConnectionSummary } from "$lib/types";

  let {
    connections,
    onnew,
    onopen,
    onedit,
    ondelete,
  }: {
    connections: ConnectionSummary[];
    onnew: () => void;
    onopen: (c: ConnectionSummary) => void;
    onedit: (c: ConnectionSummary) => void;
    ondelete: (c: ConnectionSummary) => void;
  } = $props();
</script>

<aside>
  <div class="head">
    <span class="title">{t("workspace.connections")}</span>
    <button class="new" onclick={onnew}>+ {t("workspace.new")}</button>
  </div>

  {#if connections.length === 0}
    <p class="empty">{t("workspace.empty")}</p>
  {:else}
    <ul>
      {#each connections as c (c.id)}
        <li>
          <button class="main" onclick={() => onopen(c)} title={t("workspace.connect")}>
            <span class="name">{c.name}</span>
            <span class="detail">{c.user}@{c.host}:{c.port}</span>
          </button>
          <div class="actions">
            <button onclick={() => onedit(c)}>{t("workspace.edit")}</button>
            <button onclick={() => ondelete(c)}>{t("workspace.delete")}</button>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</aside>

<style>
  aside {
    width: 15rem;
    flex: 0 0 15rem;
    border-right: 1px solid #2f3847;
    background: #1a1f29;
    padding: 1rem 0.75rem;
    box-sizing: border-box;
    overflow-y: auto;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    margin-bottom: 1rem;
  }
  .title {
    font-size: 0.75rem;
    font-weight: 600;
    color: #8d97a8;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    white-space: nowrap;
  }
  button {
    font: inherit;
    cursor: pointer;
    border: 1px solid #2f3847;
    background: #222a36;
    color: #dfe4ec;
    border-radius: 4px;
  }
  button:hover {
    border-color: #8fb4e8;
  }
  .new {
    font-size: 0.75rem;
    padding: 0.35rem 0.5rem;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }
  li {
    border: 1px solid #2f3847;
    border-radius: 5px;
    padding: 0.5rem;
    background: #1f2530;
  }
  .main {
    width: 100%;
    text-align: left;
    border: 0;
    background: transparent;
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    padding: 0.15rem;
  }
  .name {
    font-size: 0.9rem;
    font-weight: 600;
  }
  .detail {
    font-size: 0.75rem;
    color: #8d97a8;
  }
  .actions {
    display: flex;
    gap: 0.35rem;
    margin-top: 0.45rem;
  }
  .actions button {
    font-size: 0.7rem;
    padding: 0.2rem 0.4rem;
  }
  .empty {
    color: #8d97a8;
    font-size: 0.85rem;
    line-height: 1.5;
  }
</style>
