/** Pure split-tree helpers for the M6 workspace layout. */
export type SplitAxis = "horizontal" | "vertical";
export type LayoutPreset = "single" | "left-right" | "top-bottom" | "grid";

export type SplitNode =
  | { type: "leaf"; sessionId: string }
  | { type: "split"; axis: SplitAxis; ratio: number; first: SplitNode; second: SplitNode };

export function leaf(sessionId: string): SplitNode {
  return { type: "leaf", sessionId };
}

export function sessionsInTree(node: SplitNode | null): string[] {
  if (!node) return [];
  if (node.type === "leaf") return [node.sessionId];
  return [...sessionsInTree(node.first), ...sessionsInTree(node.second)];
}

export function removeSession(node: SplitNode | null, sessionId: string): SplitNode | null {
  if (!node) return null;
  if (node.type === "leaf") return node.sessionId === sessionId ? null : node;
  const first = removeSession(node.first, sessionId);
  const second = removeSession(node.second, sessionId);
  if (!first) return second;
  if (!second) return first;
  return { ...node, first, second };
}

function split(axis: SplitAxis, first: SplitNode, second: SplitNode): SplitNode {
  return { type: "split", axis, ratio: 0.5, first, second };
}

/** Builds balanced trees for the four initial workspace presets. */
export function createLayoutTree(sessionIds: string[], preset: LayoutPreset, activeId: string | null): SplitNode | null {
  const unique = [...new Set(sessionIds)];
  if (!unique.length) return null;
  const limit = preset === "grid" ? 4 : preset === "single" ? 1 : 2;
  const ordered = unique.slice(0, limit);
  if (activeId && unique.includes(activeId) && !ordered.includes(activeId)) {
    ordered[ordered.length - 1] = activeId;
  }
  if (preset === "single" && activeId && unique.includes(activeId)) ordered[0] = activeId;
  const leaves = ordered.map(leaf);
  if (leaves.length === 1) return leaves[0];
  if (preset === "top-bottom") return split("vertical", leaves[0], leaves[1]);
  if (preset === "grid" && leaves.length > 2) {
    return split("horizontal", split("vertical", leaves[0], leaves[1]), leaves[2]);
  }
  return split("horizontal", leaves[0], leaves[1]);
}
