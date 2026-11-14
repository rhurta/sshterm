/** Tauri emits PTY bytes as soon as ssh.exe starts, which is before a
 * TerminalPane component has mounted and registered its listener. A single
 * app-wide listener started at page load captures those events and buffers
 * them per session until the pane subscribes. Without this, early output such
 * as the host-key confirmation prompt is silently lost and the terminal stays
 * blank while ssh waits for an answer. */

import { listen } from "@tauri-apps/api/event";
import { fromBase64 } from "$lib/encoding";

type DataHandler = (bytes: Uint8Array) => void;
type ClosedHandler = () => void;

const dataHandlers = new Map<string, Set<DataHandler>>();
const closedHandlers = new Map<string, Set<ClosedHandler>>();
const buffered = new Map<string, Uint8Array[]>();
const MAX_BUFFERED_BYTES = 256 * 1024;

let started = false;

export function startSessionBus(): void {
  if (started) return;
  started = true;

  void listen<{ id: string; data: string }>("session:data", (event) => {
    const { id, data } = event.payload;
    const bytes = fromBase64(data);
    const handlers = dataHandlers.get(id);
    if (handlers && handlers.size > 0) {
      for (const handler of handlers) handler(bytes);
      return;
    }
    const queue = buffered.get(id) ?? [];
    const size = queue.reduce((n, chunk) => n + chunk.length, 0) + bytes.length;
    if (size <= MAX_BUFFERED_BYTES) queue.push(bytes);
    buffered.set(id, queue);
  });

  void listen<{ id: string }>("session:closed", (event) => {
    const id = event.payload.id;
    for (const handler of closedHandlers.get(id) ?? []) handler();
  });
}

export function subscribeData(id: string, handler: DataHandler): () => void {
  let handlers = dataHandlers.get(id);
  if (!handlers) {
    handlers = new Set();
    dataHandlers.set(id, handlers);
  }
  handlers.add(handler);

  // Replay anything that arrived before the pane mounted.
  const queue = buffered.get(id);
  if (queue) {
    for (const chunk of queue) handler(chunk);
    buffered.delete(id);
  }
  return () => handlers!.delete(handler);
}

export function subscribeClosed(id: string, handler: ClosedHandler): () => void {
  let handlers = closedHandlers.get(id);
  if (!handlers) {
    handlers = new Set();
    closedHandlers.set(id, handlers);
  }
  handlers.add(handler);
  return () => handlers!.delete(handler);
}

export function releaseSession(id: string): void {
  dataHandlers.delete(id);
  closedHandlers.delete(id);
  buffered.delete(id);
}
