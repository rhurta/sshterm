<script lang="ts">
  import { t } from "$lib/i18n/index.svelte";

  let {
    value = $bindable(""),
    autocomplete = "off",
    disabled = false,
  }: {
    value?: string;
    autocomplete?: "off" | "new-password" | "current-password";
    disabled?: boolean;
  } = $props();

  let shown = $state(false);
  let input: HTMLInputElement | undefined = $state();

  export function focus() {
    input?.focus();
  }
</script>

<!-- Password field with a small eye button at its right end that reveals the text.
     Style hooks from the parent: --field-bg, --field-pad, --field-font-size. -->
<span class="password-field">
  <input
    bind:this={input}
    type={shown ? "text" : "password"}
    bind:value
    {autocomplete}
    {disabled}
    spellcheck="false"
    autocapitalize="off"
  />
  <button
    type="button"
    class="reveal"
    aria-pressed={shown}
    aria-label={shown ? t("password.hide") : t("password.show")}
    title={shown ? t("password.hide") : t("password.show")}
    {disabled}
    onmousedown={(event) => event.preventDefault()}
    onclick={() => (shown = !shown)}
  >
    {#if shown}
      <!-- eye-off -->
      <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94" />
        <path d="M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19" />
        <path d="M14.12 14.12a3 3 0 1 1-4.24-4.24" />
        <line x1="1" y1="1" x2="23" y2="23" />
      </svg>
    {:else}
      <!-- eye -->
      <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z" />
        <circle cx="12" cy="12" r="3" />
      </svg>
    {/if}
  </button>
</span>

<style>
  .password-field {
    position: relative;
    display: block;
  }
  input {
    box-sizing: border-box;
    width: 100%;
    font: inherit;
    font-size: var(--field-font-size, inherit);
    color: var(--text);
    background: var(--field-bg, var(--app-bg));
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: var(--field-pad, 0.45rem 0.55rem);
    padding-right: 2.2rem; /* room for the eye button */
  }
  input:focus-visible,
  .reveal:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .reveal {
    position: absolute;
    top: 50%;
    right: 0.3rem;
    transform: translateY(-50%);
    display: flex;
    align-items: center;
    justify-content: center;
    width: 1.7rem;
    height: 1.7rem;
    padding: 0;
    color: var(--muted);
    background: transparent;
    border: 0;
    border-radius: 4px;
    cursor: pointer;
  }
  .reveal:hover:not(:disabled) {
    color: var(--text);
    background: var(--surface-hover);
  }
  .reveal[aria-pressed="true"] {
    color: var(--accent);
  }
  .reveal:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
