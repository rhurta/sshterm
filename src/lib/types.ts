/** Shared TypeScript shapes. The Rust counterparts live in vault/model.rs. */

export type AppTheme = "dark" | "light" | "nord";

export interface TerminalSettings {
  copy_on_selection: boolean;
  right_click_copy: boolean;
  middle_click_copy: boolean;
  right_click_paste: boolean;
  middle_click_paste: boolean;
  ctrl_c_copy: boolean;
  confirm_multiline_paste: boolean;
  keep_disconnected_sessions: boolean;
}

export const DEFAULT_TERMINAL_SETTINGS: TerminalSettings = {
  copy_on_selection: true,
  right_click_copy: true,
  middle_click_copy: true,
  right_click_paste: true,
  middle_click_paste: true,
  ctrl_c_copy: true,
  confirm_multiline_paste: true,
  keep_disconnected_sessions: false,
}

export interface CommandConsoleSettings {
  visible: boolean;
  height: number;
}

export const DEFAULT_COMMAND_CONSOLE_SETTINGS: CommandConsoleSettings = {
  visible: true,
  height: 180,
}

export type Auth =
  | { type: "password"; password: string }
  | { type: "key"; path: string; passphrase?: string | null }
  | { type: "agent" };

export type Tunnel =
  | { kind: "local"; bind: string; dest: string; enabled: boolean }
  | { kind: "remote"; bind: string; dest: string; enabled: boolean }
  | { kind: "dynamic"; bind: string; enabled: boolean };

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
  jump_disabled: string[];
  tunnels: Tunnel[];
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
  jump_disabled: string[];
  tunnel_count: number;
  /** Whether the saved connection is protected from broadcast by default. */
  protected: boolean;
}

export type TreeNode =
  | { type: "folder"; id: string; name: string; children: TreeNode[]; highlight?: string[] }
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
  /** Configured local/reverse forwards owned by this SSH session. */
  tunnelCount: number;
  protected: boolean;
  /** Whether the SSH process is still alive and can receive terminal input. */
  connected: boolean;
  /** Effective inherited/overridden syntax highlighting rule sets. */
  highlightSets: string[];
}
