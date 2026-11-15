import type { IMarker, Terminal } from "@xterm/xterm";

/**
 * Builds a plain-text session log from what the terminal DISPLAYS, not from the
 * bytes ConPTY sends.
 *
 * Why: ConPTY does not forward the remote bytes. It keeps its own copy of the
 * screen and sends redraws (cursor jumps, partial-line updates, full repaints on
 * resize and scroll). Stripping the escape codes from that stream leaves repeated
 * screens and text fragments. xterm.js interprets the redraws correctly, so its
 * buffer holds each line once. A line is written only when the cursor has moved
 * below it, so redraws and `\r` progress bars never reach the file.
 */
export class LogCapture {
  /** First line not written yet. xterm keeps it valid while the buffer scrolls and
   * reflows, but disposes it when that line is erased (ESC[2J) or trimmed. */
  private marker: IMarker | undefined;
  private out: string[] = [];
  private blankRun = 0;
  /** Last lines written, used to recognise a screen repaint after a marker loss. */
  private recent: string[] = [];
  /** Where new text starts after a screen erase (absolute line of the screen top). */
  private anchor: number | undefined;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private listeners: { dispose(): void }[];

  constructor(
    private term: Terminal,
    private send: (text: string) => void,
    private flushMs = 250,
  ) {
    this.marker = term.registerMarker(0);
    this.listeners = [
      term.onWriteParsed(() => this.capture(false)),
      // Erasing the screen (ED 1/2/3, RIS) disposes the marker and may erase lines
      // that were not written yet. Write everything complete first, and remember
      // where the new text will start. Returning false lets xterm run the erase.
      term.parser.registerCsiHandler({ final: "J" }, (params) => {
        if (((params[0] as number) ?? 0) !== 0) this.beforeErase();
        return false;
      }),
      term.parser.registerEscHandler({ final: "c" }, () => {
        this.beforeErase();
        return false;
      }),
    ];
  }

  private beforeErase(): void {
    if (this.term.buffer.active.type === "alternate") return;
    this.capture(false);
    this.anchor = this.term.buffer.active.baseY;
  }

  /** Writes the remaining text (including the unfinished last line) and stops. */
  finish(): void {
    this.listeners.forEach((l) => l.dispose());
    this.capture(true);
    while (this.out.length > 0 && this.out[this.out.length - 1] === "") this.out.pop();
    this.flush();
    this.marker?.dispose();
  }

  private capture(final: boolean): void {
    const buf = this.term.buffer.active;
    // vim, top, less... draw on the alternate screen: nothing useful to log.
    if (buf.type === "alternate" && !final) return;

    const lost = !this.marker || this.marker.isDisposed;
    // Marker lost (screen erased): start where the new text began after the erase.
    // A repaint of the old screen is recognised and skipped below.
    const start = lost ? (this.anchor ?? buf.baseY) : this.marker!.line;
    const cursorAbs = buf.baseY + buf.cursorY;
    let end = cursorAbs; // lines above the cursor are complete
    if (final) {
      end = cursorAbs + 1;
      for (let i = buf.length - 1; i > cursorAbs; i--) {
        const l = buf.getLine(i);
        if (l && l.translateToString(true) !== "") {
          end = i + 1;
          break;
        }
      }
    } else {
      // a wrapped line is still being written: keep all of its rows pending
      while (end > start && buf.getLine(end)?.isWrapped) end--;
    }

    if (end > start) {
      const lines: string[] = [];
      let text = "";
      for (let i = start; i < end; i++) {
        const line = buf.getLine(i);
        if (!line) continue;
        const continues = i + 1 < end && !!buf.getLine(i + 1)?.isWrapped;
        text += line.translateToString(!continues);
        if (!continues) {
          lines.push(text.replace(/\s+$/, ""));
          text = "";
        }
      }
      if (text) lines.push(text.replace(/\s+$/, ""));
      for (const l of lost ? lines.slice(this.overlap(lines)) : lines) this.addLine(l);
    }

    if (!final && (lost || end > start)) {
      if (lost) this.anchor = undefined;
      this.marker?.dispose();
      this.marker = this.term.registerMarker(Math.max(end, start) - cursorAbs);
      this.schedule();
    }
  }

  /** How many leading lines repeat the tail of what was already written. */
  private overlap(lines: string[]): number {
    const max = Math.min(lines.length, this.recent.length);
    for (let m = max; m > 0; m--) {
      let same = true;
      for (let i = 0; i < m; i++) {
        if (lines[i] !== this.recent[this.recent.length - m + i]) {
          same = false;
          break;
        }
      }
      if (same) return m;
    }
    return 0;
  }

  /** At most one blank line in a row; no blank lines at the very top. */
  private addLine(line: string): void {
    this.recent.push(line);
    if (this.recent.length > 100) this.recent.shift();
    if (line === "") {
      if (this.blankRun >= 1 || this.out.length === 0) return;
      this.blankRun++;
    } else {
      this.blankRun = 0;
    }
    this.out.push(line);
  }

  private schedule(): void {
    if (this.timer === undefined) this.timer = setTimeout(() => this.flush(), this.flushMs);
  }

  private flush(): void {
    if (this.timer !== undefined) clearTimeout(this.timer);
    this.timer = undefined;
    if (this.out.length === 0) return;
    const text = this.out.join("\n") + "\n";
    this.out = [];
    this.send(text);
  }
}
