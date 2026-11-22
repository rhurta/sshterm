import test from "node:test";
import assert from "node:assert/strict";
import { findScriptNode, folderOptions, parentFolderId, suggestScriptName, targetFolderId, visibleRows } from "../src/lib/scriptsTree.ts";

const script = (id, name) => ({ type: "script", id, name, updated_at: 0, size: 1 });
const folder = (id, name, children = []) => ({ type: "folder", id, name, children });

const tree = [
  folder("ops", "Ops", [folder("net", "Network", [script("s1", "Check routes")]), script("s2", "Reboot host")]),
  folder("db", "Database", [script("s3", "Backup")]),
  script("s4", "Top level"),
];

test("rows follow the expanded folders", () => {
  assert.deepEqual(visibleRows(tree, new Set()).map((r) => r.node.id), ["ops", "db", "s4"]);
  const rows = visibleRows(tree, new Set(["ops", "net"]));
  assert.deepEqual(rows.map((r) => [r.node.id, r.depth]), [["ops", 0], ["net", 1], ["s1", 2], ["s2", 1], ["db", 0], ["s4", 0]]);
  assert.equal(rows[0].open, true);
  assert.equal(rows[4].open, false);
});

test("a query shows matches with their ancestors, opened", () => {
  const byBody = visibleRows(tree, new Set(), "ip route", new Set(["s1"]));
  assert.deepEqual(byBody.map((r) => r.node.id), ["ops", "net", "s1"]);
  const byName = visibleRows(tree, new Set(), "backup", new Set(["s3"]));
  assert.deepEqual(byName.map((r) => r.node.id), ["db", "s3"]);
  assert.deepEqual(visibleRows(tree, new Set(), "zzz").map((r) => r.node.id), []);
});

test("a matching folder shows its whole contents", () => {
  const rows = visibleRows(tree, new Set(), "ops");
  assert.deepEqual(rows.map((r) => r.node.id), ["ops", "net", "s1", "s2"]);
});

test("lookup helpers", () => {
  assert.equal(findScriptNode(tree, "s1").name, "Check routes");
  assert.equal(findScriptNode(tree, "nope"), null);
  assert.equal(parentFolderId(tree, "s1"), "net");
  assert.equal(parentFolderId(tree, "s4"), null);
  assert.equal(parentFolderId(tree, "nope"), undefined);
  assert.equal(targetFolderId(tree, null), null);
  assert.equal(targetFolderId(tree, "db"), "db");
  assert.equal(targetFolderId(tree, "s1"), "net");
  assert.equal(targetFolderId(tree, "s4"), null);
});

test("folder options skip an excluded subtree", () => {
  assert.deepEqual(folderOptions(tree).map((o) => [o.id, o.depth]), [["ops", 0], ["net", 1], ["db", 0]]);
  assert.deepEqual(folderOptions(tree, "ops").map((o) => o.id), ["db"]);
});

test("suggested names come from the first meaningful line", () => {
  assert.equal(suggestScriptName("#!/bin/bash\n\n# Restart nginx\nsudo systemctl restart nginx"), "Restart nginx");
  assert.equal(suggestScriptName("\n  \n// Clean logs\nrm -f *.log"), "Clean logs");
  assert.equal(suggestScriptName("   "), "");
  const long = suggestScriptName("x".repeat(100));
  assert.equal(long.length, 60);
  assert.ok(long.endsWith("…"));
});
