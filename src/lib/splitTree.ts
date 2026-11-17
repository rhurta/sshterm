/** Pure split-tree helpers for the M6 workspace layout. */
export type SplitAxis = "horizontal" | "vertical";
export type LayoutPreset = "single" | "left-right" | "top-bottom" | "grid";
export type SplitPath = Array<"first" | "second">;

export type SplitNode =
  | { type: "leaf"; sessionId: string; sessionIds: string[] }
  | { type: "split"; axis: SplitAxis; ratio: number; first: SplitNode; second: SplitNode };

export function leaf(sessionId: string): SplitNode {
  return { type: "leaf", sessionId, sessionIds: [sessionId] };
}

export function sessionsInTree(node: SplitNode | null): string[] {
  if (!node) return [];
  if (node.type === "leaf") return [...node.sessionIds];
  return [...sessionsInTree(node.first), ...sessionsInTree(node.second)];
}

export function removeSession(node: SplitNode | null, sessionId: string, preserveEmptyPane = false): SplitNode | null {
  if (!node) return null;
  if (node.type === "leaf") {
    if (!node.sessionIds.includes(sessionId)) return node;
    const remaining = node.sessionIds.filter((id) => id !== sessionId);
    return remaining.length || preserveEmptyPane ? { ...node, sessionIds: remaining } : null;
  }
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
  if (preset === "grid" && leaves.length === 3) {
    return split("horizontal", split("vertical", leaves[0], leaves[1]), leaves[2]);
  }
  if (preset === "grid" && leaves.length === 4) {
    return split("horizontal", split("vertical", leaves[0], leaves[1]), split("vertical", leaves[2], leaves[3]));
  }
  return split("horizontal", leaves[0], leaves[1]);
}

/** Insert a new session beside a target leaf. Existing session IDs are never duplicated. */
export function splitSession(node: SplitNode | null, targetId: string, sessionId: string, axis: SplitAxis): SplitNode | null {
  if (!node || !targetId || !sessionId || sessionsInTree(node).includes(sessionId)) return node;
  if (node.type === "leaf") {
    return node.sessionId === targetId
      ? { type: "split", axis, ratio: 0.5, first: node, second: leaf(sessionId) }
      : node;
  }
  if (sessionsInTree(node.first).includes(targetId)) {
    return { ...node, first: splitSession(node.first, targetId, sessionId, axis) ?? node.first };
  }
  if (sessionsInTree(node.second).includes(targetId)) {
    return { ...node, second: splitSession(node.second, targetId, sessionId, axis) ?? node.second };
  }
  return node;
}



/** Split the pane containing targetSessionId and create a stable empty sibling pane. */
export function splitPaneWithEmptyLeaf(
  node: SplitNode | null,
  targetSessionId: string,
  newPaneId: string,
  axis: SplitAxis,
): SplitNode | null {
  if (!node || !targetSessionId || !newPaneId || sessionsInTree(node).includes(newPaneId)) return node;
  if (node.type === "leaf") {
    return node.sessionIds.includes(targetSessionId)
      ? { type: "split", axis, ratio: 0.5, first: node, second: { type: "leaf", sessionId: newPaneId, sessionIds: [] } }
      : node;
  }
  if (sessionsInTree(node.first).includes(targetSessionId)) {
    return { ...node, first: splitPaneWithEmptyLeaf(node.first, targetSessionId, newPaneId, axis) ?? node.first };
  }
  if (sessionsInTree(node.second).includes(targetSessionId)) {
    return { ...node, second: splitPaneWithEmptyLeaf(node.second, targetSessionId, newPaneId, axis) ?? node.second };
  }
  return node;
}
\n/** Add a session as a tab in the pane containing the stable pane ID. */
export function addSessionToPane(node: SplitNode | null, paneId: string, sessionId: string): SplitNode | null {
  if (!node || !paneId || !sessionId || sessionsInTree(node).includes(sessionId)) return node;
  if (node.type === "leaf") {
    return node.sessionId === paneId
      ? { ...node, sessionIds: [...node.sessionIds, sessionId] }
      : node;
  }
  if (findPane(node.first, paneId)) {
    return { ...node, first: addSessionToPane(node.first, paneId, sessionId) ?? node.first };
  }
  if (findPane(node.second, paneId)) {
    return { ...node, second: addSessionToPane(node.second, paneId, sessionId) ?? node.second };
  }
  return node;
}

function findPane(node: SplitNode, paneId: string): boolean {
  return node.type === "leaf" ? node.sessionId === paneId : findPane(node.first, paneId) || findPane(node.second, paneId);
}
\n/** Update exactly one split, identified by its path from the root. */
export function setSplitRatioAtPath(node: SplitNode | null, path: SplitPath, ratio: number): SplitNode | null {
  if (!node || node.type === "leaf") return node;
  const bounded = Math.max(0.15, Math.min(0.85, ratio));
  if (path.length === 0) return { ...node, ratio: bounded };
  const [branch, ...rest] = path;
  if (branch === "first") {
    return { ...node, first: setSplitRatioAtPath(node.first, rest, ratio) ?? node.first };
  }
  return { ...node, second: setSplitRatioAtPath(node.second, rest, ratio) ?? node.second };
}


export type LayoutRect = { x: number; y: number; width: number; height: number };
export type LayoutLeaf = { sessionId: string; sessionIds: string[]; rect: LayoutRect };
export type LayoutDivider = {
  path: SplitPath;
  axis: SplitAxis;
  rect: LayoutRect;
  region: LayoutRect;
  ratio: number;
};
export type LayoutGeometry = { leaves: LayoutLeaf[]; dividers: LayoutDivider[] };

/** Calculates normalized leaf and divider bounds for a recursive split tree. */
export function getLayoutGeometry(node: SplitNode | null): LayoutGeometry {
  const leaves: LayoutLeaf[] = [];
  const dividers: LayoutDivider[] = [];

  function visit(current: SplitNode, rect: LayoutRect, path: SplitPath) {
    if (current.type === "leaf") {
      leaves.push({ sessionId: current.sessionId, sessionIds: [...current.sessionIds], rect });
      return;
    }

    const divider: LayoutRect = current.axis === "horizontal"
      ? {
          x: rect.x + rect.width * current.ratio,
          y: rect.y,
          width: 0,
          height: rect.height,
        }
      : {
          x: rect.x,
          y: rect.y + rect.height * current.ratio,
          width: rect.width,
          height: 0,
        };
    dividers.push({ path, axis: current.axis, rect: divider, region: rect, ratio: current.ratio });

    if (current.axis === "horizontal") {
      const firstWidth = rect.width * current.ratio;
      visit(current.first, { ...rect, width: firstWidth }, [...path, "first"]);
      visit(current.second, {
        x: rect.x + firstWidth,
        y: rect.y,
        width: rect.width - firstWidth,
        height: rect.height,
      }, [...path, "second"]);
    } else {
      const firstHeight = rect.height * current.ratio;
      visit(current.first, { ...rect, height: firstHeight }, [...path, "first"]);
      visit(current.second, {
        x: rect.x,
        y: rect.y + firstHeight,
        width: rect.width,
        height: rect.height - firstHeight,
      }, [...path, "second"]);
    }
  }

  if (node) visit(node, { x: 0, y: 0, width: 1, height: 1 }, []);
  return { leaves, dividers };
}

/** Compatibility helper: adjust the nearest split enclosing a target leaf. */
export function setSplitRatio(node: SplitNode | null, targetId: string, ratio: number): SplitNode | null {
  if (!node || node.type === "leaf") return node;
  if (sessionsInTree(node.first).includes(targetId)) {
    if (node.first.type === "leaf" && node.first.sessionIds.includes(targetId)) {
      return { ...node, ratio: Math.max(0.15, Math.min(0.85, ratio)) };
    }
    const first = setSplitRatio(node.first, targetId, ratio) ?? node.first;
    return { ...node, first };
  }
  if (sessionsInTree(node.second).includes(targetId)) {
    if (node.second.type === "leaf" && node.second.sessionIds.includes(targetId)) {
      return { ...node, ratio: Math.max(0.15, Math.min(0.85, ratio)) };
    }
    const second = setSplitRatio(node.second, targetId, ratio) ?? node.second;
    return { ...node, second };
  }
  return node;
}
