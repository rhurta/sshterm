/** Shared TypeScript shapes. The Rust counterparts live in vault/model.rs. */

export type Auth =
  | { type: "password"; password: string }
  | { type: "key"; path: string; passphrase?: string | null }
  | { type: "agent" };

export interface Colors {
  fg: string;
  bg: string;
}

export interface Connection {
  id: string;
  name: string;
  host: string;
  port: number;
  user: string;
  auth: Auth;
  keepalive_secs: number;
  colors?: Colors | null;
  protected: boolean;
  /** Ordered saved connection IDs used as ProxyJump hops. */
  jump: string[];
  tunnels: unknown[];
  sftp?: unknown | null;
  highlight: string[];
}

/** What `conn_list` returns: never contains credentials. */
export interface ConnectionSummary {
  id: string;
  name: string;
  host: string;
  port: number;
  user: string;
  /** Ordered jump-host connection IDs; contains no credentials. */
  jump: string[];
}

export type TreeNode =
  | { type: "folder"; id: string; name: string; children: TreeNode[] }
  | { type: "connection"; id: string };

export interface SearchHit {
  id: string;
  name: string;
  host: string;
  port: number;
  user: string;
  path: string[];
}

export interface ImportCandidate {
  name: string;
  host: string;
  port: number;
  user: string;
  auth: Auth;
  label: string;
  /** Folder names from the source hierarchy, outermost first. */
  folder_path: string[];
  warnings: string[];
}

export interface Skipped {
  label: string;
  reason: string;
}

export interface ImportPreview {
  source: string;
  candidates: ImportCandidate[];
  skipped: Skipped[];
  notices: string[];
}

export interface ImportReport {
  created: number;
  folders_created: number;
  skipped: Skipped[];
}

export interface Tab {
  sessionId: string;
  connId: string;
  name: string;
  /** Resolved display names for the visual jump route. */
  jumpNames: string[];
}
