import test from "node:test";
import assert from "node:assert/strict";
import {
  addSessionToPane,
  createLayoutTree,
  getLayoutGeometry,
  leaf,
  removeSession,
  removeEmptyPane,
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

test("single preset puts the active session first and keeps other sessions as tabs", () => {
  const tree = createLayoutTree(["a", "b"], "single", "b");
  assert.deepEqual(sessionsInTree(tree), ["b", "a"]);
  assert.deepEqual(getLayoutGeometry(tree).leaves.map((pane) => pane.sessionIds), [["b", "a"]]);
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


test("removeEmptyPane removes the selected empty leaf and collapses its parent", () => {
  const tree = splitPaneWithEmptyLeaf(leaf("a"), "a", "empty-pane", "horizontal");
  const updated = removeEmptyPane(tree, "empty-pane");
  assert.deepEqual(sessionsInTree(updated), ["a"]);
  assert.equal(getLayoutGeometry(updated).leaves.length, 1);
});

test("removeEmptyPane does not remove a pane that still contains sessions", () => {
  const tree = splitPaneWithEmptyLeaf(leaf("a"), "a", "empty-pane", "horizontal");
  const updated = removeEmptyPane(tree, "a");
  assert.deepEqual(sessionsInTree(updated), ["a"]);
  assert.equal(getLayoutGeometry(updated).leaves.length, 2);
});

import { moveSession, moveSessionToNewPane, neighborPane, paneOfSession } from "../src/lib/splitTree.ts";

function twoPanes() {
  // pane "a" holds a1,a2 ; pane "b" holds b1
  let tree = leaf("a");
  tree = addSessionToPane(tree, "a", "a2");
  tree = splitSession(tree, "a", "b", "horizontal");
  tree = { ...tree, second: { type: "leaf", sessionId: "b", sessionIds: ["b1"] } };
  return tree;
}

test("moveSession moves a tab to another pane and keeps the empty source pane", () => {
  const tree = twoPanes();
  assert.equal(paneOfSession(tree, "a2"), "a");
  const moved = moveSession(tree, "a2", "b");
  assert.equal(paneOfSession(moved, "a2"), "b");
  assert.deepEqual(sessionsInTree(moved).sort(), ["a", "a2", "b1"].sort());
  const geometry = getLayoutGeometry(moved);
  assert.deepEqual(geometry.leaves.find((p) => p.sessionId === "b").sessionIds, ["b1", "a2"]);
  const emptied = moveSession(moved, "a", "b");
  assert.equal(getLayoutGeometry(emptied).leaves.length, 2);
  assert.deepEqual(getLayoutGeometry(emptied).leaves.find((p) => p.sessionId === "a").sessionIds, []);
});

test("moveSession reorders inside a pane and ignores unknown ids", () => {
  const tree = twoPanes();
  const reordered = moveSession(tree, "a2", "a", 0);
  assert.deepEqual(getLayoutGeometry(reordered).leaves.find((p) => p.sessionId === "a").sessionIds, ["a2", "a"]);
  assert.equal(moveSession(tree, "nope", "b"), tree);
  assert.equal(moveSession(tree, "a2", "nope"), tree);
});

test("moveSessionToNewPane splits beside the target with a unique pane id", () => {
  const tree = twoPanes();
  const split = moveSessionToNewPane(tree, "a2", "b", "p-new", "vertical", true);
  assert.deepEqual(sessionsInTree(split).sort(), ["a", "a2", "b1"].sort());
  const leaves = getLayoutGeometry(split).leaves;
  assert.equal(leaves.length, 3);
  assert.deepEqual(leaves.find((p) => p.sessionId === "p-new").sessionIds, ["a2"]);
  assert.equal(moveSessionToNewPane(tree, "a2", "b", "a", "vertical", true), tree); // id already used
  assert.equal(moveSessionToNewPane(tree, "b1", "b", "x", "vertical", false), tree); // lone tab onto its own pane
});

test("neighborPane finds the adjacent pane in each direction", () => {
  const grid = createLayoutTree(["a", "b", "c", "d"], "grid", "a");
  const leaves = getLayoutGeometry(grid).leaves;
  const id = (s) => leaves.find((p) => p.sessionIds.includes(s)).sessionId;
  const right = neighborPane(leaves, id("a"), "right");
  assert.ok(right && right !== id("a"));
  assert.equal(neighborPane(leaves, right, "left"), id("a"));
  assert.equal(neighborPane(leaves, id("a"), "left"), null);
  assert.equal(neighborPane(leaves, id("a"), "up"), null);
  assert.ok(neighborPane(leaves, id("a"), "down"));
});
