<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import "@xterm/xterm/css/xterm.css";
  import { toBase64 } from "$lib/encoding";
  import { LogCapture } from "$lib/logCapture";
  import { subscribeClosed, subscribeData, releaseSession } from "$lib/sessionBus";
  import type { AppTheme, TerminalSettings } from "$lib/types";
  import { getTerminalTheme } from "$lib/theme";

  let {
    sessionId,
    active,
    focused,
    onactivate,
    onclosed,
    settings,
    theme,
    onmultilinepaste,
  }: {
    sessionId: string;
    active: boolean;
    focused: boolean;
    onactivate: (id: string) => void;
    onclosed: (id: string) => void;
    settings: TerminalSettings;
    theme: AppTheme;
    onmultilinepaste: (text: string) => Promise<boolean>;
  } = $props();

  // Workspace opens every session at 80x24; the first real fit may match it.
  const PROVISIONAL = { cols: 80, rows: 24 };

  let host: HTMLDivElement;
  let term: Terminal | undefined;
  let fit: FitAddon | undefined;
  let lastCols = PROVISIONAL.cols;
  let lastRows = PROVISIONAL.rows;
  // Set once xterm is mounted, so the focus effect runs after `open()`.
  let mounted = $state(false);

  /** Fits the terminal to its box and tells the PTY, but only when the size really
   * changed: every resize makes ConPTY repaint the whole screen. */
  function syncSize() {
    if (!active || !term || !fit) return; // a hidden tab has no valid dimensions
    try {
      fit.fit();
    } catch {
      return; // no size yet
    }
    const { cols, rows } = term;
    if (cols === lastCols && rows === lastRows) return;
    lastCols = cols;
    lastRows = rows;
    void invoke("session_resize", { id: sessionId, cols, rows });
  }

  onMount(() => {
    // The canvas renderer is used deliberately: the WebGL addon can fail to
    // paint in WebView2, which shows a cursor but no text at all.
    const t = new Terminal({
      fontFamily: 'Consolas, "Cascadia Mono", monospace',
      fontSize: 14,
      cursorBlink: true,
      scrollback: 5000,
      theme: getTerminalTheme(theme),
    });
    term = t;
    const f = new FitAddon();
    fit = f;
    t.loadAddon(f);
    t.open(host);
    mounted = true;

    // Session log: plain text built from what this terminal displays (see
    // logCapture.ts). Calls are chained so the text reaches the file in order.
    let logQueue: Promise<unknown> = Promise.resolve();
    const logCall = (cmd: string, args: Record<string, unknown>) => {
      logQueue = logQueue.then(() => invoke(cmd, args)).catch(() => {});
    };
    const capture = new LogCapture(t, (text) => logCall("session_log_append", { id: sessionId, text }));
    let logFinished = false;
    const finishLog = () => {
      if (logFinished) return;
      logFinished = true;
      capture.finish(); // sends the last lines, including the unfinished one
      logCall("session_log_close", { id: sessionId });
    };

    syncSize();
    // Dragging the window fires many events: wait until the size settles.
    let timer: ReturnType<typeof setTimeout> | undefined;
    const observer = new ResizeObserver(() => {
      clearTimeout(timer);
      timer = setTimeout(syncSize, 80);
    });
    observer.observe(host);

    const copyText = (text: string) => {
      if (!text || !navigator.clipboard?.writeText) return;
      void navigator.clipboard.writeText(text).catch(() => {});
    };
    const copySelection = (clearAfterCopy = false) => {
      const selected = t.getSelection();
      if (!selected) return;
      copyText(selected);
      if (clearAfterCopy) t.clearSelection();
    };
    const pasteClipboard = () => {
      if (!navigator.clipboard?.readText) return;
      // Clear before starting the asynchronous clipboard read so a previous
      // terminal selection never remains highlighted after a paste gesture.
      t.clearSelection();
      void navigator.clipboard.readText().then(async (value) => {
        if (!value) return;
        if (settings.confirm_multiline_paste && /[\r\n]/.test(value)) {
          const confirmed = await onmultilinepaste(value);
          if (!confirmed) {
            t.focus();
            return;
          }
        }
        t.paste(value);
        t.focus();
      }).catch(() => {});
    };

    // Automatic copy follows xterm's current selection. This is intentionally
    // separate from the click and keyboard switches in application settings.
    t.onSelectionChange(() => {
      if (settings.copy_on_selection) copySelection();
    });

    // Ctrl+C copies only when a selection exists; without a selection xterm's
    // normal key handling sends ETX (Ctrl+C) to the remote shell.
    // Ctrl+V is handled explicitly so it works consistently in WebView2.
    t.attachCustomKeyEventHandler((event) => {
      if (event.type !== "keydown") return true;
      const mod = event.ctrlKey || event.metaKey;
      if (!mod || event.altKey) return true;
      const key = event.key.toLowerCase();
      if (key === "c") {
        event.preventDefault();
        if (t.hasSelection() && settings.ctrl_c_copy) {
          copySelection(true);
        } else {
          // With no selection (or when copy-on-Ctrl+C is disabled), deliver
          // the interrupt directly to the remote PTY.
          void invoke("session_write", {
            id: sessionId,
            data: toBase64(new TextEncoder().encode("\u0003")),
          });
        }
        return false;
      }
      if (key === "v") {
        // Ctrl+V is intentionally unconditional and is not a user setting.
        // Prevent WebView's native paste path: pasteClipboard() already feeds
        // the clipboard text to xterm, otherwise both paths paste the same text.
        event.preventDefault();
        pasteClipboard();
        return false;
      }
      return true;
    });

    const onContextMenu = (event: MouseEvent) => {
      event.preventDefault();
      if (t.hasSelection() && settings.right_click_copy) {
        copySelection(true);
      } else if (settings.right_click_paste) {
        // If copy is disabled, paste remains available and clears the
        // current selection before sending the clipboard contents.
        pasteClipboard();
      }
    };
    const onMouseDown = (event: MouseEvent) => {
      if (event.button !== 1) return;
      event.preventDefault();
      if (t.hasSelection() && settings.middle_click_copy) {
        copySelection(true);
      } else if (settings.middle_click_paste) {
        pasteClipboard();
      }
    };
    host.addEventListener("contextmenu", onContextMenu);
    host.addEventListener("mousedown", onMouseDown);

    t.onData((data) => {
      void invoke("session_write", {
        id: sessionId,
        data: toBase64(new TextEncoder().encode(data)),
      });
    });

    const cleanups = [
      subscribeData(sessionId, (bytes) => t.write(bytes)),
      subscribeClosed(sessionId, () => {
        // xterm parses writes asynchronously: wait until the last output is in the
        // buffer before the log is closed and the tab is dropped.
        t.write("", () => {
          finishLog();
          onclosed(sessionId);
        });
      }),
    ];

    return () => {
      cleanups.forEach((fn) => fn());
      host.removeEventListener("contextmenu", onContextMenu);
      host.removeEventListener("mousedown", onMouseDown);
      releaseSession(sessionId);
      clearTimeout(timer);
      observer.disconnect();
      finishLog();
      t.dispose();
    };
  });

  // Keep already-open terminals in sync when the application theme changes.
  $effect(() => {
    const palette = getTerminalTheme(theme);
    if (mounted && term) term.options.theme = palette;
  });

  // Visible panes are fitted independently; only the selected pane receives
  // keyboard focus so tiled terminals do not race focus between instances.
  $effect(() => {
    if (active && mounted) syncSize();
  });

  $effect(() => {
    if (focused && active && mounted) term?.focus();
  });
</script>

<div class="pane" class:hidden={!active} onfocusin={() => onactivate(sessionId)} onclick={() => onactivate(sessionId)}>
  <div class="terminal-host" bind:this={host}></div>
</div>

<style>
  .pane {
    width: 100%;
    height: 100%;
    padding: 0.5rem;
    box-sizing: border-box;
    overflow: hidden;
  }
  .terminal-host {
    width: 100%;
    height: 100%;
    min-height: 0;
    min-width: 0;
  }
  .hidden {
    display: none;
  }
</style>
