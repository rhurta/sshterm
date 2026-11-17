import test from "node:test";
import assert from "node:assert/strict";
import {
  createLayoutTree,
  leaf,
  removeSession,
  sessionsInTree,
  setSplitRatioAtPath,
  splitSession,
} from "./splitTree.ts";

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
