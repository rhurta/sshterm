import assert from "node:assert/strict";
import test from "node:test";
import { neighborPane } from "../src/lib/paneNav.ts";
import { createLayoutTree, getLayoutGeometry } from "../src/lib/splitTree.ts";

function leavesFor(preset, ids = ["a", "b", "c", "d"]) {
  return getLayoutGeometry(createLayoutTree(ids, preset, ids[0])).leaves;
}

test("left-right layout moves horizontally only", () => {
  const leaves = leavesFor("left-right", ["a", "b"]);
  assert.equal(neighborPane(leaves, "a", "right")?.sessionId, "b");
  assert.equal(neighborPane(leaves, "b", "left")?.sessionId, "a");
  assert.equal(neighborPane(leaves, "a", "left"), null);
  assert.equal(neighborPane(leaves, "a", "down"), null);
});

test("top-bottom layout moves vertically only", () => {
  const leaves = leavesFor("top-bottom", ["a", "b"]);
  assert.equal(neighborPane(leaves, "a", "down")?.sessionId, "b");
  assert.equal(neighborPane(leaves, "b", "up")?.sessionId, "a");
  assert.equal(neighborPane(leaves, "b", "down"), null);
  assert.equal(neighborPane(leaves, "a", "right"), null);
});

test("2x2 grid reaches every neighbor and never wraps", () => {
  // createLayoutTree fills columns first: a top-left, b bottom-left, c top-right, d bottom-right.
  const leaves = leavesFor("grid");
  const id = (session) => leaves.find((l) => l.sessionIds.includes(session)).sessionId;
  const [a, b, c, d] = ["a", "b", "c", "d"].map(id);
  assert.equal(neighborPane(leaves, a, "right")?.sessionId, c);
  assert.equal(neighborPane(leaves, a, "down")?.sessionId, b);
  assert.equal(neighborPane(leaves, d, "left")?.sessionId, b);
  assert.equal(neighborPane(leaves, d, "up")?.sessionId, c);
  assert.equal(neighborPane(leaves, c, "right"), null);
  assert.equal(neighborPane(leaves, b, "down"), null);
  assert.equal(neighborPane(leaves, a, "up"), null);
});

test("prefers the pane sharing the larger edge and skips empty panes", () => {
  const leaves = [
    { sessionId: "L", sessionIds: ["l"], rect: { x: 0, y: 0, width: 0.5, height: 1 } },
    { sessionId: "RT", sessionIds: ["rt"], rect: { x: 0.5, y: 0, width: 0.5, height: 0.3 } },
    { sessionId: "RB", sessionIds: ["rb"], rect: { x: 0.5, y: 0.3, width: 0.5, height: 0.7 } },
  ];
  assert.equal(neighborPane(leaves, "L", "right")?.sessionId, "RB");
  leaves[2].sessionIds = [];
  assert.equal(neighborPane(leaves, "L", "right")?.sessionId, "RT");
  assert.equal(neighborPane(leaves, "missing", "right"), null);
});
