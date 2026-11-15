<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import "@xterm/xterm/css/xterm.css";
  import { toBase64 } from "$lib/encoding";
  import { LogCapture } from "$lib/logCapture";
  import { subscribeClosed, subscribeData, releaseSession } from "$lib/sessionBus";

  let {
    sessionId,
    active,
    onclosed,
  }: { sessionId: string; active: boolean; onclosed: (id: string) => void } = $props();

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
      theme: { background: "#161a22", foreground: "#dfe4ec" },
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
      releaseSession(sessionId);
      clearTimeout(timer);
      observer.disconnect();
      finishLog();
      t.dispose();
    };
  });

  // M4: a newly opened session (or a tab switch) must take keyboard focus so
  // typing goes straight into the terminal, not into the sidebar or tab bar.
  // The pane is only focusable once it is visible and xterm has been opened.
  $effect(() => {
    if (active && mounted) {
      syncSize();
      term?.focus();
    }
  });
</script>

<div class="pane" class:hidden={!active} bind:this={host}></div>

<style>
  .pane {
    width: 100%;
    height: 100%;
    padding: 0.5rem;
    box-sizing: border-box;
    overflow: hidden;
  }
  .hidden {
    display: none;
  }
</style>
