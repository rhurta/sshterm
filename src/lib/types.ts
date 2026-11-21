/** Shared TypeScript shapes. The Rust counterparts live in vault/model.rs. */

export type { AppTheme } from "./theme";

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

export interface AppBehaviorSettings {
  stop_tunnels_on_lock: boolean;
  close_to_tray: boolean;
}

export const DEFAULT_APP_BEHAVIOR_SETTINGS: AppBehaviorSettings = {
  stop_tunnels_on_lock: true,
  close_to_tray: false,
}

export interface CommandConsoleSettings {
  visible: boolean;
  height: number;
}

export const DEFAULT_COMMAND_CONSOLE_SETTINGS: CommandConsoleSettings = {
  visible: true,
  height: 180,
}

export interface WorkspacePanelSettings {
  tree_visible: boolean;
  properties_visible: boolean;
}

export const DEFAULT_WORKSPACE_PANEL_SETTINGS: WorkspacePanelSettings = {
  tree_visible: true,
  properties_visible: true,
}

export type Auth =
  | { type: "password"; password: string }
  | { type: "key"; path: string; passphrase?: string | null }
  | { type: "agent" };

export type Tunnel =
  | { kind: "local"; bind: string; dest: string; enabled: boolean }
  | { kind: "remote"; bind: string; dest: string; enabled: boolean }
  | { kind: "dynamic"; bind: string; enabled: boolean };

/** Runtime-only tunnel status returned by tunnel_list and tunnel:status. */
export type TunnelLifecycle = "starting" | "active" | "stopping" | "stopped" | "failed";
export type TunnelForwardLifecycle = "starting" | "active" | "stopping" | "stopped" | "failed";

export interface TunnelForwardInfo {
  index: number;
  kind: "local" | "remote" | "dynamic";
  bind: string;
  destination: string | null;
  status: TunnelForwardLifecycle;
  error: string | null;
}

export interface TunnelInfo {
  connection_id: string;
  connection_name: string;
  host: string;
  port: number;
  user: string;
  jump_names: string[];
  status: TunnelLifecycle;
  forwards: TunnelForwardInfo[];
  error: string | null;
  started_at: number;
}

export interface Colors {
  fg: string;
  bg: string;
}

export interface SftpOptions {
  /** Optional target SSH port override; null/omitted inherits Connection.port. */
  port?: number | null;
  start_dir: string;
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
  sftp?: SftpOptions | null;
  highlight: string[];
}

/** What `conn_list` returns: never contains credentials. */
export interface HighlightRuleSetStatus {
  name: string;
  file: string;
  rule_count: number;
  valid: boolean;
  error: string | null;
}

export interface HighlightTestMatch {
  line: number;
  rule_name: string;
  scope: string;
  start: number;
  end: number;
  fg: string | null;
  bg: string | null;
  bold: boolean;
}

export interface HighlightTestResult {
  set_name: string;
  prompt_matches: boolean;
  matches: HighlightTestMatch[];
}

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

/** Background image and transparency (config.toml `appearance`). */
export interface AppearanceSettings {
  /** Path of the background image; empty means none. */
  background_image: string;
  /** Interface opacity over the image, in percent (20-100). */
  panel_opacity: number;
  /** Terminal background opacity over the image, in percent (20-100). */
  terminal_opacity: number;
}

export const DEFAULT_APPEARANCE_SETTINGS: AppearanceSettings = {
  background_image: "",
  panel_opacity: 85,
  terminal_opacity: 85,
};


/** Runtime-only SFTP session information; never contains credentials. */
export interface SftpSessionInfo {
  session_id: string;
  connection_id: string;
  connection_name: string;
  host: string;
  port: number;
  user: string;
  jump_names: string[];
  current_path: string;
  agent_binding: "not_used" | "bound" | "legacy_unbound";
}

export interface SftpEntry {
  name: string;
  path: string;
  kind: "directory" | "file" | "symlink" | "other";
  size: number | null;
  /** Unix timestamp in seconds from the remote server. */
  modified: number | null;
  permissions: number | null;
}

export interface SftpDirectory {
  current_path: string;
  parent_path: string | null;
  entries: SftpEntry[];
}
