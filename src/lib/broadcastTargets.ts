/** Safe target resolution for M6.3 command broadcast. */
export type BroadcastTargetMode = "active" | "selected" | "all";

export interface BroadcastSession {
  sessionId: string;
  name: string;
  protected: boolean;
  connected: boolean;
}

export interface BroadcastTargetResolution {
  targets: BroadcastSession[];
  excludedProtected: number;
}

/**
 * Resolve against the currently open sessions on every send attempt.
 * Active mode is a deliberate single-session action; protected exclusion
 * applies to selected/all broadcast modes unless the user opts in.
 */
export function resolveBroadcastTargets(
  sessions: BroadcastSession[],
  activeSessionId: string | null,
  mode: BroadcastTargetMode,
  selectedSessionIds: string[],
  includeProtected: boolean,
): BroadcastTargetResolution {
  const unique = [...new Map(sessions.map((session) => [session.sessionId, session])).values()];
  const connected = unique.filter((session) => session.connected);
  if (mode === "active") {
    const active = connected.find((session) => session.sessionId === activeSessionId);
    return { targets: active ? [active] : [], excludedProtected: 0 };
  }

  const selected = mode === "all"
    ? connected
    : connected.filter((session) => selectedSessionIds.includes(session.sessionId));
  const excludedProtected = includeProtected ? 0 : selected.filter((session) => session.protected).length;
  return {
    targets: includeProtected ? selected : selected.filter((session) => !session.protected),
    excludedProtected,
  };
}
