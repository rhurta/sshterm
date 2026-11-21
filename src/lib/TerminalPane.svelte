<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import "@xterm/xterm/css/xterm.css";
  import { toBase64 } from "$lib/encoding";
  import { LogCapture } from "$lib/logCapture";
  import {
    subscribeClosed,
    subscribeData,
    releaseSession,
    takeOpenSize,
    setLastPaneSize,
  } from "$lib/sessionBus";
  import type { AppTheme, TerminalSettings } from "$lib/types";
  import { getTerminalTheme } from "$lib/theme";

  let {
    sessionId,
    container = null,
    active,
    focused,
    onactivate,
    onclosed,
    settings,
    theme,
    onmultilinepaste,
    highlightSets = [],
  }: {
    sessionId: string;
    /** Pane element this terminal is attached to. The terminal is created once
     * per session and its DOM is moved here, so moving a tab between panes keeps
     * scrollback and screen state (no remount). Null while no pane exists yet. */
    container?: HTMLElement | null;
    active: boolean;
    focused: boolean;
    onactivate: (id: string) => void;
    onclosed: (id: string) => void;
    settings: TerminalSettings;
    theme: AppTheme;
    onmultilinepaste: (text: string) => Promise<boolean>;
    highlightSets?: string[];
  } = $props();

  // Workspace opens each session with the last fitted size (80x24 for the very
  // first one); a resize is only sent when the real fit differs from that.
  // sessionId never changes for a mounted pane, so reading it once is intended.
  const PROVISIONAL = untrack(() => takeOpenSize(sessionId)) ?? { cols: 80, rows: 24 };

  // session_write calls are chained so keystrokes and pastes reach ssh in the
  // order they were produced; independent invokes may be scheduled out of order.
  let writeChain: Promise<unknown> = Promise.resolve();
  function sendInput(text: string) {
    const data = toBase64(new TextEncoder().encode(text));
    writeChain = writeChain
      .then(() => invoke("session_write", { id: sessionId, data }))
      .catch(() => {});
  }

  let host: HTMLDivElement;
  let root = $state<HTMLDivElement | null>(null);
  let term: Terminal | undefined;
  let fit: FitAddon | undefined;
  let lastCols = PROVISIONAL.cols;
  let lastRows = PROVISIONAL.rows;
  // Set once xterm is mounted, so the focus effect runs after `open()`.
  let mounted = $state(false);
  let disconnected = $state(false);

  /** Fits the terminal to its box and tells the PTY, but only when the size really
   * changed: every resize makes ConPTY repaint the whole screen. */
  function syncSize() {
    if (!active || !term || !fit) return; // a hidden tab has no valid dimensions
    // Not attached to a pane (parked, or its pane is being rebuilt): the box has
    // no meaningful size, and fitting it would shrink the PTY.
    if (!container || root?.parentElement !== container) return;
    try {
      fit.fit();
    } catch {
      return; // no size yet
    }
    const { cols, rows } = term;
    if (cols === lastCols && rows === lastRows) return;
    lastCols = cols;
    lastRows = rows;
    setLastPaneSize(cols, rows);
    if (!disconnected) void invoke("session_resize", { id: sessionId, cols, rows });
  }

  onMount(() => {
    // The canvas renderer is used deliberately: the WebGL addon can fail to
    // paint in WebView2, which shows a cursor but no text at all.
    const t = new Terminal({
      fontFamily: 'Consolas, "Cascadia Mono", monospace',
      fontSize: 14,
      cursorBlink: true,
      scrollback: 5000,
      allowProposedApi: true,
      theme: getTerminalTheme(theme),
    });
    term = t;
    const f = new FitAddon();
    fit = f;
    t.loadAddon(f);
    t.open(host);
    mounted = true;


    // Highlight only text already parsed into xterm's normal buffer. Matching
    // is performed by Rust's linear-time regex engine; returned ranges become
    // visual cell decorations and never enter the PTY input/output stream.
    const highlightDecorations: { dispose: () => void }[] = [];
    let highlightTimer: ReturnType<typeof setTimeout> | undefined;
    let highlightRequest = 0;
    let highlightDisposed = false;
    const clearHighlightDecorations = () => {
      while (highlightDecorations.length) highlightDecorations.pop()?.dispose();
    };
    const renderHighlights = async () => {
      if (highlightDisposed || !highlightSets.length) {
        clearHighlightDecorations();
        return;
      }
      const buffer = t.buffer.active;
      if (buffer.type !== "normal") {
        clearHighlightDecorations();
        return;
      }
      const requestId = highlightRequest;
      const firstLine = buffer.viewportY;
      const logicalLines: { text: string; segments: { row: number; start: number; end: number }[] }[] = [];
      for (let row = 0; row < t.rows; row++) {
        const bufferRow = firstLine + row;
        const sourceLine = buffer.getLine(bufferRow);
        const text = sourceLine?.translateToString(true) ?? "";
        if (sourceLine?.isWrapped && logicalLines.length) {
          const logical = logicalLines[logicalLines.length - 1];
          const start = logical.text.length;
          logical.text += text;
          logical.segments.push({ row: bufferRow, start: Array.from(logical.text.slice(0, start)).length, end: Array.from(logical.text).length });
        } else {
          logicalLines.push({
            text,
            segments: [{ row: bufferRow, start: 0, end: Array.from(text).length }],
          });
        }
      }
      const lines = logicalLines.map((line) => line.text);
      if (!lines.some((line) => line.length > 0)) {
        clearHighlightDecorations();
        return;
      }
      try {
        const matches = await invoke<{
          line: number; start: number; end: number; rule_name: string;
          scope: string; fg: string | null; bg: string | null; bold: boolean;
        }[]>("highlight_match", { ruleSets: highlightSets, lines });
        if (highlightDisposed || requestId !== highlightRequest || t.buffer.active.type !== "normal") return;
        clearHighlightDecorations();
        for (const match of matches) {
          if (match.line < 0 || match.line >= logicalLines.length || match.end <= match.start) continue;
          const logical = logicalLines[match.line];
          for (const segment of logical.segments) {
            const segmentStart = Math.max(match.start, segment.start);
            const segmentEnd = Math.min(match.end, segment.end);
            if (segmentEnd <= segmentStart) continue;
            const bufferLine = buffer.getLine(segment.row);
            if (!bufferLine) continue;
            const toCellColumn = (characterOffset: number) => {
              if (characterOffset <= 0) return 0;
              let characterCount = 0;
              for (let column = 0; column < t.cols; column++) {
                const cell = bufferLine.getCell(column);
                if (!cell) break;
                const cellCharacters = Array.from(cell.getChars()).length;
                if (characterOffset < characterCount + cellCharacters) return column;
                characterCount += cellCharacters;
                const cellWidth = cell.getWidth();
                if (cellWidth === 2) column++;
                if (characterCount >= characterOffset) return Math.min(t.cols, column + 1);
              }
              return t.cols;
            };
            const localStart = segmentStart - segment.start;
            const localEnd = segmentEnd - segment.start;
            const startColumn = toCellColumn(localStart);
            const endColumn = toCellColumn(localEnd);
            if (endColumn <= startColumn) continue;
            const cursorAbsoluteLine = buffer.baseY + buffer.cursorY;
            const marker = t.registerMarker(segment.row - cursorAbsoluteLine);
            if (!marker) continue;
            highlightDecorations.push(marker);
            const decoration = t.registerDecoration({
              marker,
              x: startColumn,
              width: endColumn - startColumn,
              ...(match.fg ? { foregroundColor: match.fg } : {}),
              ...(match.bg ? { backgroundColor: match.bg } : {}),
              layer: "bottom",
            });
            if (decoration) {
              highlightDecorations.push(decoration);
              if (match.bold) {
                highlightDecorations.push(decoration.onRender((element) => {
                  element.style.fontWeight = "bold";
                }));
              }
            }
          }
        }
      } catch {
        // Missing/invalid rule files must never interrupt terminal rendering.
      }
    };
    const scheduleHighlights = () => {
      if (!highlightSets.length || highlightDisposed) return;
      clearTimeout(highlightTimer);
      // Invalidate any outstanding IPC result immediately, so a scroll or
      // buffer switch cannot briefly repaint decorations for an older viewport.
      highlightRequest += 1;
      highlightTimer = setTimeout(() => void renderHighlights(), 70);
    };
    const highlightListeners = [
      t.onWriteParsed(scheduleHighlights),
      t.onScroll(scheduleHighlights),
      t.buffer.onBufferChange(() => {
        if (t.buffer.active.type !== "normal") clearHighlightDecorations();
        else scheduleHighlights();
      }),
    ];

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
      if (disconnected || !navigator.clipboard?.readText) return;
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
      if (disconnected) {
        // Keep copy available for inspecting a failed session, but never send
        // keyboard input to a PTY that has already exited.
        if (event.type === "keydown"
          && (event.ctrlKey || event.metaKey)
          && event.key.toLowerCase() === "c"
          && settings.ctrl_c_copy
          && t.hasSelection()) {
          event.preventDefault();
          copySelection(true);
        }
        return false;
      }
      if (event.type !== "keydown") return true;
      // Ctrl+Alt+Arrows focus a neighboring pane and, with Shift, move the tab
      // there (Workspace): never let xterm turn them into modified-arrow
      // sequences for the shell.
      if (event.ctrlKey && event.altKey && event.key.startsWith("Arrow")) return false;
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
          sendInput("\u0003");
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
      if (disconnected) return;
      sendInput(data);
    });

    const cleanups = [
      subscribeData(sessionId, (bytes) => t.write(bytes)),
      subscribeClosed(sessionId, () => {
        // xterm parses writes asynchronously: wait until the last output is in the
        // buffer before locking input and reporting that the SSH process has exited.
        t.write("", () => {
          finishLog();
          disconnected = true;
          t.options.disableStdin = true;
          onclosed(sessionId);
        });
      }),
    ];

    return () => {
      cleanups.forEach((fn) => fn());
      highlightDisposed = true;
      highlightRequest += 1;
      clearTimeout(highlightTimer);
      highlightListeners.forEach((listener) => listener.dispose());
      clearHighlightDecorations();
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

  // Attach the terminal to its pane. Re-runs when the session moves to another
  // pane (or its pane element is recreated): the node is re-parented, not rebuilt.
  $effect(() => {
    const target = container;
    const node = root;
    if (!target || !node || node.parentElement === target) return;
    target.appendChild(node);
    untrack(() => {
      if (!mounted || !term) return;
      syncSize();
      term.refresh(0, term.rows - 1);
      // Re-parenting drops DOM focus, so give it back to the focused terminal.
      if (focused && active) term.focus();
    });
  });
</script>

<div class="pane" class:hidden={!active} bind:this={root} onfocusin={() => onactivate(sessionId)}>
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

  /* xterm's viewport is outside the tree's scoped CSS; mirror its scrollbar. */
  .terminal-host :global(.xterm-viewport) {
    scrollbar-width: thin;
    scrollbar-color: var(--muted) transparent;
  }
  .terminal-host :global(.xterm-viewport::-webkit-scrollbar) { width: 8px; }
  .terminal-host :global(.xterm-viewport::-webkit-scrollbar-track) { background: transparent; }
  .terminal-host :global(.xterm-viewport::-webkit-scrollbar-thumb) {
    background: var(--muted);
    border-radius: 6px;
  }
  .hidden {
    display: none;
  }
</style>
