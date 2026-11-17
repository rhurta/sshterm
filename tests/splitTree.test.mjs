import test from "node:test";
import assert from "node:assert/strict";
import {
  addSessionToPane,\n  createLayoutTree,
  getLayoutGeometry,
  leaf,
  removeSession,
  sessionsInTree,
  setSplitRatioAtPath,
  splitSession,
  splitPaneWithEmptyLeaf,
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
  assert.deepEqual(geometry.leaves.map((item) => item.sessionIds), [["a"], ["b"], ["c"], ["d"]]);
  assert.equal(geometry.dividers.length, 3);
  assert.deepEqual(geometry.leaves[0].rect, { x: 0, y: 0, width: 0.5, height: 0.5 });
  assert.deepEqual(geometry.leaves[1].rect, { x: 0, y: 0.5, width: 0.5, height: 0.5 });
  assert.deepEqual(geometry.leaves[2].rect, { x: 0.5, y: 0, width: 0.5, height: 0.5 });
  assert.deepEqual(geometry.leaves[3].rect, { x: 0.5, y: 0.5, width: 0.5, height: 0.5 });
});

test("addSessionToPane appends a tab without creating another tile", () => {
  const tree = createLayoutTree(["a", "b"], "left-right", "a");
  const updated = addSessionToPane(tree, "a", "c");
  assert.deepEqual(sessionsInTree(updated), ["a", "c", "b"]);
  const geometry = getLayoutGeometry(updated);
  assert.deepEqual(geometry.leaves[0].sessionIds, ["a", "c"]);
  assert.equal(geometry.leaves.length, 2);
});

test("removing one tab keeps its pane until the final tab closes", () => {
  const tree = addSessionToPane(leaf("a"), "a", "b");
  const oneLeft = removeSession(tree, "a");
  assert.deepEqual(sessionsInTree(oneLeft), ["b"]);
  assert.equal(getLayoutGeometry(oneLeft).leaves[0].sessionId, "a");
  assert.equal(removeSession(oneLeft, "b"), null);
});

test("splitPaneWithEmptyLeaf creates an empty sibling pane", () => {
  const tree = splitPaneWithEmptyLeaf(leaf("a"), "a", "empty-pane", "vertical");
  const geometry = getLayoutGeometry(tree);
  assert.deepEqual(geometry.leaves.map((item) => item.sessionIds), [["a"], []]);
  assert.equal(geometry.leaves[1].sessionId, "empty-pane");
  assert.deepEqual(sessionsInTree(tree), ["a"]);
});

test("removeSession can preserve an empty pane for the workspace", () => {
  const tree = removeSession(leaf("a"), "a", true);
  assert.deepEqual(getLayoutGeometry(tree).leaves.map((item) => item.sessionIds), [[]]);
  assert.deepEqual(sessionsInTree(tree), []);
});

test("preset pane groups keep sessions beyond the visible tile count reachable", () => {
  const sessions = ["a", "b", "c", "d", "e", "f"];
  const single = createLayoutTree(sessions, "single", "e");
  assert.deepEqual(sessionsInTree(single), ["e", "a", "b", "c", "d", "f"]);
  assert.equal(getLayoutGeometry(single).leaves.length, 1);
  const two = createLayoutTree(sessions, "left-right", "e");
  assert.deepEqual(sessionsInTree(two), ["e", "a", "b", "c", "d", "f"]);
  assert.deepEqual(getLayoutGeometry(two).leaves.map((item) => item.sessionIds.length), [1, 5]);
  const grid = createLayoutTree(sessions, "grid", "e");
  assert.deepEqual(sessionsInTree(grid), ["e", "a", "b", "c", "d", "f"]);
  assert.deepEqual(getLayoutGeometry(grid).leaves.map((item) => item.sessionIds.length), [1, 1, 1, 3]);
});

test("top-bottom preset with one session still creates one pane", () => {
  assert.deepEqual(sessionsInTree(createLayoutTree(["only"], "top-bottom", "only")), ["only"]);
});
