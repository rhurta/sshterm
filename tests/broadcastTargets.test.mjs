import test from "node:test";
import assert from "node:assert/strict";
import { resolveBroadcastTargets } from "../src/lib/broadcastTargets.ts";

const sessions = [
  { sessionId: "s1", name: "Production", protected: false, connected: true },
  { sessionId: "s2", name: "Staging", protected: true, connected: true },
  { sessionId: "s3", name: "Database", protected: false, connected: true },
  { sessionId: "s4", name: "Disconnected", protected: false, connected: false },
];

test("active mode resolves only the active open session, including protected", () => {
  assert.deepEqual(
    resolveBroadcastTargets(sessions, "s2", "active", [], false),
    { targets: [sessions[1]], excludedProtected: 0 },
  );
});

test("selected mode includes only selected open sessions and excludes protected by default", () => {
  assert.deepEqual(
    resolveBroadcastTargets(sessions, "s1", "selected", ["s1", "s2", "closed"], false),
    { targets: [sessions[0]], excludedProtected: 1 },
  );
});

test("selected mode includes protected sessions only after explicit opt-in", () => {
  assert.deepEqual(
    resolveBroadcastTargets(sessions, "s1", "selected", ["s1", "s2"], true),
    { targets: [sessions[0], sessions[1]], excludedProtected: 0 },
  );
});

test("all mode excludes protected sessions by default", () => {
  assert.deepEqual(
    resolveBroadcastTargets(sessions, "s1", "all", [], false),
    { targets: [sessions[0], sessions[2]], excludedProtected: 1 },
  );
});

test("all mode includes every open session after explicit opt-in", () => {
  assert.deepEqual(
    resolveBroadcastTargets(sessions, "s1", "all", [], true),
    { targets: sessions.slice(0, 3), excludedProtected: 0 },
  );
});

test("disconnected sessions are never targetable, including when explicitly selected", () => {
  assert.deepEqual(
    resolveBroadcastTargets(sessions, "s4", "active", [], false),
    { targets: [], excludedProtected: 0 },
  );
  assert.deepEqual(
    resolveBroadcastTargets(sessions, "s1", "selected", ["s1", "s4"], false),
    { targets: [sessions[0]], excludedProtected: 0 },
  );
  assert.deepEqual(
    resolveBroadcastTargets(sessions, "s1", "all", [], true),
    { targets: sessions.slice(0, 3), excludedProtected: 0 },
  );
});

test("stale active IDs and duplicate session IDs do not create targets", () => {
  assert.deepEqual(
    resolveBroadcastTargets([sessions[0], sessions[0]], "closed", "active", [], false),
    { targets: [], excludedProtected: 0 },
  );
});
