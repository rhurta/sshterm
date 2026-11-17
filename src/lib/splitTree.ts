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

/** Insert a new session beside a target leaf. Existing session IDs are never duplicated. */
export function splitSession(node: SplitNode | null, targetId: string, sessionId: string, axis: SplitAxis): SplitNode | null {
  if (!node || sessionsInTree(node).includes(sessionId)) return node;
  if (node.type === "leaf") {
    return node.sessionId === targetId
      ? { type: "split", axis, ratio: 0.5, first: node, second: leaf(sessionId) }
      : node;
  }
  return {
    ...node,
    first: splitSession(node.first, targetId, sessionId, axis) ?? node.first,
    second: splitSession(node.second, targetId, sessionId, axis) ?? node.second,
  };
}

/** Update the ratio at the split whose two branches contain the requested target. */
export function setSplitRatio(node: SplitNode | null, targetId: string, ratio: number): SplitNode | null {
  if (!node || node.type === "leaf") return node;
  const bounded = Math.max(0.15, Math.min(0.85, ratio));
  if (sessionsInTree(node.first).includes(targetId)) {
    return { ...node, ratio: bounded, first: setSplitRatio(node.first, targetId, ratio) ?? node.first };
  }
  return { ...node, second: setSplitRatio(node.second, targetId, ratio) ?? node.second };
}
