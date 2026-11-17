import test from "node:test";
import assert from "node:assert/strict";
import {
  createLayoutTree,
  getLayoutGeometry,
  leaf,
  removeSession,
  sessionsInTree,
  setSplitRatioAtPath,
  splitSession,
} from "../src/lib/splitTree.ts";

test("layout presets include each session once and grid balances four panes", () => {
  const tree = createLayoutTree(["a", "b", "c", "d"], "grid", "a");
  assert.deepEqual(sessionsInTree(tree), ["a", "b", "c", "d"]);
  assert.equal(tree?.type, "split");
  if (tree?.type === "split") {
    assert.equal(tree.first.type, "split");
    assert.equal(tree.second.type, "split");
  }
});

test("single preset follows active session", () => {
  assert.deepEqual(sessionsInTree(createLayoutTree(["a", "b"], "single", "b")), ["b"]);
});

test("splitSession inserts beside a target without duplicating sessions", () => {
  const tree = splitSession(leaf("a"), "a", "b", "vertical");
  assert.deepEqual(sessionsInTree(tree), ["a", "b"]);
  assert.deepEqual(sessionsInTree(splitSession(tree, "a", "b", "horizontal")), ["a", "b"]);
  assert.deepEqual(sessionsInTree(splitSession(tree, "missing", "c", "horizontal")), ["a", "b"]);
});

test("removeSession collapses empty branches", () => {
  const tree = createLayoutTree(["a", "b", "c", "d"], "grid", "a");
  assert.deepEqual(sessionsInTree(removeSession(tree, "b")), ["a", "c", "d"]);
  assert.deepEqual(sessionsInTree(removeSession(removeSession(tree, "b"), "a")), ["c", "d"]);
  assert.equal(removeSession(leaf("a"), "a"), null);
});

test("split ratio updates only the addressed split and clamps bounds", () => {
  const tree = createLayoutTree(["a", "b", "c", "d"], "grid", "a");
  const updated = setSplitRatioAtPath(tree, ["first"], 0.95);
  assert.equal(updated?.type, "split");
  if (updated?.type === "split") {
    assert.equal(updated.ratio, 0.5);
    assert.equal(updated.first.type === "split" ? updated.first.ratio : 0, 0.85);
    assert.equal(updated.second.type === "split" ? updated.second.ratio : 0, 0.5);
  }
});

test("layout geometry positions nested leaves and dividers within normalized bounds", () => {
  const tree = createLayoutTree(["a", "b", "c", "d"], "grid", "a");
  const geometry = getLayoutGeometry(tree);
  assert.deepEqual(geometry.leaves.map((item) => item.sessionId), ["a", "b", "c", "d"]);
  assert.equal(geometry.dividers.length, 3);
  assert.deepEqual(geometry.leaves[0].rect, { x: 0, y: 0, width: 0.5, height: 0.5 });
  assert.deepEqual(geometry.leaves[1].rect, { x: 0, y: 0.5, width: 0.5, height: 0.5 });
  assert.deepEqual(geometry.leaves[2].rect, { x: 0.5, y: 0, width: 0.5, height: 0.5 });
  assert.deepEqual(geometry.leaves[3].rect, { x: 0.5, y: 0.5, width: 0.5, height: 0.5 });
});
