import type { LayoutLeaf } from "./splitTree";

export type Direction = "left" | "right" | "up" | "down";

const EPSILON = 1e-6;

function overlap(aStart: number, aEnd: number, bStart: number, bEnd: number): number {
  return Math.min(aEnd, bEnd) - Math.max(aStart, bStart);
}

/**
 * Finds the pane next to `fromPaneId` in a direction, from the normalized pane
 * rectangles. A candidate must lie fully on that side and share part of the
 * perpendicular edge; the closest one wins, then the one with the larger shared
 * edge. There is no wrap-around. Panes without sessions are skipped.
 */
export function neighborPane(
  leaves: LayoutLeaf[],
  fromPaneId: string,
  direction: Direction,
): LayoutLeaf | null {
  const from = leaves.find((leaf) => leaf.sessionId === fromPaneId);
  if (!from) return null;
  const f = from.rect;

  let best: { leaf: LayoutLeaf; gap: number; shared: number } | null = null;
  for (const leaf of leaves) {
    if (leaf === from || leaf.sessionIds.length === 0) continue;
    const r = leaf.rect;
    let gap: number;
    let shared: number;
    if (direction === "right") {
      gap = r.x - (f.x + f.width);
      shared = overlap(f.y, f.y + f.height, r.y, r.y + r.height);
    } else if (direction === "left") {
      gap = f.x - (r.x + r.width);
      shared = overlap(f.y, f.y + f.height, r.y, r.y + r.height);
    } else if (direction === "down") {
      gap = r.y - (f.y + f.height);
      shared = overlap(f.x, f.x + f.width, r.x, r.x + r.width);
    } else {
      gap = f.y - (r.y + r.height);
      shared = overlap(f.x, f.x + f.width, r.x, r.x + r.width);
    }
    if (gap < -EPSILON || shared <= EPSILON) continue;
    if (
      !best
      || gap < best.gap - EPSILON
      || (Math.abs(gap - best.gap) <= EPSILON && shared > best.shared + EPSILON)
    ) {
      best = { leaf, gap, shared };
    }
  }
  return best?.leaf ?? null;
}
