<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import "@xterm/xterm/css/xterm.css";
  import { toBase64 } from "$lib/encoding";
  import { subscribeClosed, subscribeData, releaseSession } from "$lib/sessionBus";

  let {
    sessionId,
    active,
    onclosed,
  }: { sessionId: string; active: boolean; onclosed: (id: string) => void } = $props();

  let host: HTMLDivElement;
  let term: Terminal | undefined;
  let fit: FitAddon | undefined;

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

    const resize = () => {
      if (!active) return; // a hidden tab has no valid dimensions
      try {
        f.fit();
        void invoke("session_resize", { id: sessionId, cols: t.cols, rows: t.rows });
      } catch {
        // no size yet
      }
    };
    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(host);

    t.onData((data) => {
      void invoke("session_write", {
        id: sessionId,
        data: toBase64(new TextEncoder().encode(data)),
      });
    });

    const cleanups = [
      subscribeData(sessionId, (bytes) => t.write(bytes)),
      subscribeClosed(sessionId, () => onclosed(sessionId)),
    ];

    return () => {
      cleanups.forEach((fn) => fn());
      releaseSession(sessionId);
      observer.disconnect();
      t.dispose();
    };
  });

  // When a tab becomes active the pane has dimensions again: refit.
  $effect(() => {
    if (active && fit && term) {
      try {
        fit.fit();
        void invoke("session_resize", { id: sessionId, cols: term.cols, rows: term.rows });
      } catch {
        // ignore: the observer will retry
      }
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
