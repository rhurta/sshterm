<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { hasMessage, t, type MessageKey } from "$lib/i18n/index.svelte";
  import LanguageSelect from "./LanguageSelect.svelte";
  import { THEMES, type AppTheme } from "$lib/theme";
  import type { AppBehaviorSettings, AppearanceSettings, TerminalSettings } from "$lib/types";

  let {
    settings,
    appBehavior,
    theme,
    appearance,
    onterminal,
    onbehavior,
    onhighlight,
    onthemechange,
    onappearance,
    onclose,
  }: {
    settings: TerminalSettings;
    appBehavior: AppBehaviorSettings;
    theme: AppTheme;
    appearance: AppearanceSettings;
    onterminal: <K extends keyof TerminalSettings>(key: K, value: TerminalSettings[K]) => void;
    onbehavior: <K extends keyof AppBehaviorSettings>(key: K, value: AppBehaviorSettings[K]) => void;
    onhighlight: () => void;
    onthemechange: (theme: AppTheme) => void;
    /** Saves appearance settings; returns an error code or null. */
    onappearance: (next: AppearanceSettings) => Promise<string | null>;
    onclose: () => void;
  } = $props();

  // Every section and the labels it contains. Search matches the section title
  // (shows the whole section) or an individual label (shows only that row).
  const SECTIONS: Record<string, { title: MessageKey; items: MessageKey[] }> = {
    language: { title: "settings.language", items: ["settings.language"] },
    appearance: {
      title: "settings.appearance",
      items: ["theme.label", "settings.backgroundImage", "settings.backgroundImageHint", "settings.panelOpacity", "settings.terminalOpacity", "settings.opacityHint"],
    },
    shortcuts: {
      title: "workspace.keyboardShortcuts",
      items: ["workspace.shortcutConsoleHelp", "workspace.shortcutLayoutHelp", "workspace.shortcutPaneHelp", "console.shortcutSendHelp"],
    },
    behavior: {
      title: "settings.applicationBehavior",
      items: ["settings.stopTunnelsOnLock", "settings.keepTunnelsOnLockWarning", "settings.closeToTray", "settings.closeToTrayHint"],
    },
    terminal: {
      title: "settings.terminal",
      items: [
        "settings.highlightRules", "settings.copyOnSelection", "settings.rightClickCopy", "settings.middleClickCopy",
        "settings.rightClickPaste", "settings.middleClickPaste", "settings.ctrlCCopy", "settings.confirmMultilinePaste",
        "settings.keepDisconnectedSessions",
      ],
    },
  };

  let query = $state("");
  let searchEl: HTMLInputElement | undefined = $state();
  const q = $derived(query.trim().toLowerCase());

  const matches = (key: MessageKey) => t(key).toLowerCase().includes(q);
  /** Is this row visible? Always when there is no query or the section title matches. */
  const show = (section: string, item: MessageKey) => !q || matches(SECTIONS[section].title) || matches(item);
  const showSection = (section: string) =>
    !q || matches(SECTIONS[section].title) || SECTIONS[section].items.some(matches);
  const anyVisible = $derived(Object.keys(SECTIONS).some(showSection));

  onMount(() => searchEl?.focus());

  function onSearchKeydown(event: KeyboardEvent) {
    // Escape clears the search first and only closes the dialog when it is empty.
    if (event.key === "Escape" && query) {
      event.preventDefault();
      event.stopPropagation();
      query = "";
    }
  }

  // --- appearance -----------------------------------------------------------
  // Local edit buffers: start from the saved values, then follow the user's input.
  let imagePath = $state(untrack(() => appearance.background_image));
  let imageError = $state("");
  let busy = $state(false);
  let panelOpacity = $state(untrack(() => appearance.panel_opacity));
  let terminalOpacity = $state(untrack(() => appearance.terminal_opacity));

  async function saveAppearance(next: AppearanceSettings, reportImage: boolean) {
    busy = true;
    const failure = await onappearance(next);
    busy = false;
    if (reportImage) {
      const key = `error.${failure}`;
      imageError = failure
        ? t("settings.backgroundError", { detail: failure && hasMessage(key) ? t(key) : failure })
        : "";
    }
    if (failure) {
      panelOpacity = appearance.panel_opacity;
      terminalOpacity = appearance.terminal_opacity;
    }
  }

  const applyImage = (path: string) => {
    imagePath = path.trim();
    return saveAppearance({ ...appearance, background_image: imagePath }, true);
  };
</script>

<div class="backdrop">
  <button class="scrim" tabindex="-1" aria-label={t("workspace.close")} onclick={onclose}></button>
  <div class="dialog" role="dialog" aria-modal="true" aria-label={t("settings.title")}>
    <div class="dialog-head">
      <h2>{t("settings.title")}</h2>
      <button class="close" onclick={onclose} title={t("workspace.close")} aria-label={t("workspace.close")}>&times;</button>
    </div>

    <input
      class="search"
      type="search"
      bind:this={searchEl}
      bind:value={query}
      onkeydown={onSearchKeydown}
      placeholder={t("settings.search")}
      aria-label={t("settings.searchLabel")}
      autocomplete="off"
      spellcheck="false"
    />

    <div class="body">
      {#if showSection("language")}
        <h3>{t("settings.language")}</h3>
        <LanguageSelect />
      {/if}

      {#if showSection("appearance")}
        <h3>{t("settings.appearance")}</h3>
        {#if show("appearance", "theme.label")}
          <label class="field">
            {t("theme.label")}
            <select value={theme} onchange={(event) => onthemechange(event.currentTarget.value as AppTheme)}>
              {#each THEMES as id (id)}
                <option value={id}>{t(`theme.${id}` as MessageKey)}</option>
              {/each}
            </select>
          </label>
        {/if}
        {#if show("appearance", "settings.backgroundImage") || show("appearance", "settings.backgroundImageHint")}
          <div class="field">
            <label for="bg-path">{t("settings.backgroundImage")}</label>
            <div class="row">
              <input
                id="bg-path"
                type="text"
                bind:value={imagePath}
                onkeydown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    void applyImage(imagePath);
                  }
                }}
                placeholder="C:\Pictures\wallpaper.jpg"
                autocomplete="off"
                spellcheck="false"
                disabled={busy}
              />
              <button class="ghost" disabled={busy} onclick={() => applyImage(imagePath)}>{t("settings.backgroundApply")}</button>
              <button class="ghost" disabled={busy || !appearance.background_image} onclick={() => applyImage("")}>{t("settings.backgroundClear")}</button>
            </div>
            {#if imageError}<p class="error" role="alert">{imageError}</p>{/if}
            <p class="hint">{t("settings.backgroundImageHint")}</p>
          </div>
        {/if}
        {#if show("appearance", "settings.panelOpacity")}
          <label class="field slider">
            <span>{t("settings.panelOpacity")} <output>{panelOpacity}%</output></span>
            <input
              type="range"
              min="20"
              max="100"
              step="5"
              bind:value={panelOpacity}
              disabled={busy}
              onchange={() => saveAppearance({ ...appearance, panel_opacity: panelOpacity }, false)}
            />
          </label>
        {/if}
        {#if show("appearance", "settings.terminalOpacity")}
          <label class="field slider">
            <span>{t("settings.terminalOpacity")} <output>{terminalOpacity}%</output></span>
            <input
              type="range"
              min="20"
              max="100"
              step="5"
              bind:value={terminalOpacity}
              disabled={busy}
              onchange={() => saveAppearance({ ...appearance, terminal_opacity: terminalOpacity }, false)}
            />
          </label>
        {/if}
        {#if show("appearance", "settings.opacityHint")}
          <p class="hint">{t("settings.opacityHint")}</p>
        {/if}
      {/if}

      {#if showSection("shortcuts")}
        <h3>{t("workspace.keyboardShortcuts")}</h3>
        {#each SECTIONS.shortcuts.items as key (key)}
          {#if show("shortcuts", key)}<p class="hint">{t(key)}</p>{/if}
        {/each}
      {/if}

      {#if showSection("behavior")}
        <h3>{t("settings.applicationBehavior")}</h3>
        {#if show("behavior", "settings.stopTunnelsOnLock") || show("behavior", "settings.keepTunnelsOnLockWarning")}
          <label class="setting"><input type="checkbox" checked={appBehavior.stop_tunnels_on_lock} onchange={(e) => onbehavior("stop_tunnels_on_lock", e.currentTarget.checked)} />{t("settings.stopTunnelsOnLock")}</label>
          <p class="hint">{t("settings.keepTunnelsOnLockWarning")}</p>
        {/if}
        {#if show("behavior", "settings.closeToTray") || show("behavior", "settings.closeToTrayHint")}
          <label class="setting"><input type="checkbox" checked={appBehavior.close_to_tray} onchange={(e) => onbehavior("close_to_tray", e.currentTarget.checked)} />{t("settings.closeToTray")}</label>
          <p class="hint">{t("settings.closeToTrayHint")}</p>
        {/if}
      {/if}

      {#if showSection("terminal")}
        <h3>{t("settings.terminal")}</h3>
        {#if show("terminal", "settings.highlightRules")}
          <button class="ghost" onclick={onhighlight}>{t("settings.highlightRules")}</button>
        {/if}
        {#if show("terminal", "settings.copyOnSelection")}
          <label class="setting"><input type="checkbox" checked={settings.copy_on_selection} onchange={(e) => onterminal("copy_on_selection", e.currentTarget.checked)} />{t("settings.copyOnSelection")}</label>
        {/if}
        {#if show("terminal", "settings.rightClickCopy")}
          <label class="setting"><input type="checkbox" checked={settings.right_click_copy} onchange={(e) => onterminal("right_click_copy", e.currentTarget.checked)} />{t("settings.rightClickCopy")}</label>
        {/if}
        {#if show("terminal", "settings.middleClickCopy")}
          <label class="setting"><input type="checkbox" checked={settings.middle_click_copy} onchange={(e) => onterminal("middle_click_copy", e.currentTarget.checked)} />{t("settings.middleClickCopy")}</label>
        {/if}
        {#if show("terminal", "settings.rightClickPaste")}
          <label class="setting"><input type="checkbox" checked={settings.right_click_paste} onchange={(e) => onterminal("right_click_paste", e.currentTarget.checked)} />{t("settings.rightClickPaste")}</label>
        {/if}
        {#if show("terminal", "settings.middleClickPaste")}
          <label class="setting"><input type="checkbox" checked={settings.middle_click_paste} onchange={(e) => onterminal("middle_click_paste", e.currentTarget.checked)} />{t("settings.middleClickPaste")}</label>
        {/if}
        {#if show("terminal", "settings.ctrlCCopy")}
          <label class="setting"><input type="checkbox" checked={settings.ctrl_c_copy} onchange={(e) => onterminal("ctrl_c_copy", e.currentTarget.checked)} />{t("settings.ctrlCCopy")}</label>
        {/if}
        {#if show("terminal", "settings.confirmMultilinePaste")}
          <label class="setting"><input type="checkbox" checked={settings.confirm_multiline_paste} onchange={(e) => onterminal("confirm_multiline_paste", e.currentTarget.checked)} />{t("settings.confirmMultilinePaste")}</label>
        {/if}
        {#if show("terminal", "settings.keepDisconnectedSessions")}
          <label class="setting"><input type="checkbox" checked={settings.keep_disconnected_sessions} onchange={(e) => onterminal("keep_disconnected_sessions", e.currentTarget.checked)} />{t("settings.keepDisconnectedSessions")}</label>
        {/if}
      {/if}

      {#if !anyVisible}
        <p class="empty" role="status">{t("settings.noResults", { query: query.trim() })}</p>
      {/if}
    </div>

    <button class="ghost close-button" onclick={onclose}>{t("workspace.close")}</button>
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
    z-index: 25;
  }
  .scrim {
    position: absolute;
    inset: 0;
    border: 0;
    padding: 0;
    background: transparent;
    cursor: default;
  }
  /* Fixed size, proportional to the window: filtering or switching content never
     resizes it. Only the body scrolls; the title and search stay at the top. */
  .dialog {
    position: relative;
    z-index: 1;
    width: clamp(380px, 46vw, 760px);
    height: clamp(360px, 76vh, 820px);
    max-width: 96vw;
    max-height: 96vh;
    overflow: hidden;
    background: var(--surface-raised);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 1.1rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    box-sizing: border-box;
  }
  .dialog-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    flex: 0 0 auto;
  }
  h2 {
    margin: 0;
    font-size: 1rem;
    font-weight: 500;
  }
  h3 {
    margin: 0.5rem 0 0.1rem;
    font-size: 0.85rem;
    color: var(--accent);
    font-weight: 600;
  }
  .search {
    flex: 0 0 auto;
    font: inherit;
    font-size: 0.85rem;
    color: var(--text);
    background: var(--app-bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.45rem 0.6rem;
  }
  .body {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
    padding-right: 0.3rem;
  }
  .body > h3:first-child {
    margin-top: 0;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    font-size: 0.82rem;
    color: var(--text-secondary);
  }
  .field select,
  .field input[type="text"] {
    font: inherit;
    font-size: 0.82rem;
    color: var(--text);
    background: var(--app-bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.4rem 0.55rem;
  }
  .row {
    display: flex;
    gap: 0.4rem;
  }
  .row input {
    flex: 1;
    min-width: 0;
  }
  .slider span {
    display: flex;
    justify-content: space-between;
  }
  .slider input {
    accent-color: var(--accent);
  }
  output {
    color: var(--muted);
  }
  .setting {
    display: flex;
    align-items: center;
    gap: 0.65rem;
    padding: 0.35rem 0;
    color: var(--text);
    font-size: 0.82rem;
  }
  .setting input {
    accent-color: var(--accent);
  }
  .hint {
    margin: 0;
    font-size: 0.75rem;
    color: var(--muted);
  }
  .error {
    margin: 0;
    font-size: 0.78rem;
    color: var(--danger);
  }
  .empty {
    margin: 1rem 0;
    text-align: center;
    color: var(--muted);
    font-size: 0.85rem;
  }
  button.close {
    font: inherit;
    font-size: 1.15rem;
    line-height: 1;
    width: 1.65rem;
    height: 1.65rem;
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-secondary);
    background: transparent;
    border: 1px solid transparent;
    border-radius: 4px;
    cursor: pointer;
  }
  button.ghost {
    font: inherit;
    font-size: 0.8rem;
    align-self: flex-start;
    color: var(--text-secondary);
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.35rem 0.7rem;
    cursor: pointer;
  }
  .row button.ghost {
    align-self: auto;
  }
  button.close-button {
    flex: 0 0 auto;
  }
  button:disabled {
    opacity: 0.55;
    cursor: default;
  }
  button:focus-visible,
  input:focus-visible,
  select:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
