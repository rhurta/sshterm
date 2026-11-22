import type { ScriptSummary } from "./types";

export interface ScriptRow {
  node: ScriptSummary;
  depth: number;
  /** Folders only: shown open. */
  open: boolean;
}

export interface FolderOption {
  /** null = top level */
  id: string | null;
  depth: number;
  name: string;
}

export function findScriptNode(nodes: ScriptSummary[], id: string): ScriptSummary | null {
  for (const node of nodes) {
    if (node.id === id) return node;
    if (node.type === "folder") {
      const found = findScriptNode(node.children, id);
      if (found) return found;
    }
  }
  return null;
}

/** ID of the folder that contains `id` (null = top level), or undefined if not found. */
export function parentFolderId(nodes: ScriptSummary[], id: string, parent: string | null = null): string | null | undefined {
  for (const node of nodes) {
    if (node.id === id) return parent;
    if (node.type === "folder") {
      const found = parentFolderId(node.children, id, node.id);
      if (found !== undefined) return found;
    }
  }
  return undefined;
}

/** Folder to act in for a selection: the folder itself, the folder of a script, or the top level. */
export function targetFolderId(nodes: ScriptSummary[], selectedId: string | null): string | null {
  if (!selectedId) return null;
  const node = findScriptNode(nodes, selectedId);
  if (!node) return null;
  if (node.type === "folder") return node.id;
  return parentFolderId(nodes, selectedId) ?? null;
}

function subtreeHasMatch(node: ScriptSummary, query: string, matchIds: Set<string>): boolean {
  if (node.name.toLowerCase().includes(query)) return true;
  if (node.type === "script") return matchIds.has(node.id);
  return node.children.some((child) => subtreeHasMatch(child, query, matchIds));
}

/**
 * The rows to show, in order. Without a query: folders follow `expanded`. With a query:
 * scripts whose name or body matched (`matchIds`) and folders whose name matched are shown
 * with their ancestors, all open; a matching folder shows its whole contents.
 */
export function visibleRows(
  nodes: ScriptSummary[],
  expanded: ReadonlySet<string>,
  query = "",
  matchIds: ReadonlySet<string> = new Set(),
): ScriptRow[] {
  const q = query.trim().toLowerCase();
  const rows: ScriptRow[] = [];
  const walk = (list: ScriptSummary[], depth: number, forceAll: boolean) => {
    for (const node of list) {
      if (q && !forceAll && !subtreeHasMatch(node, q, matchIds as Set<string>)) continue;
      if (node.type === "script") {
        rows.push({ node, depth, open: false });
        continue;
      }
      const folderMatches = node.name.toLowerCase().includes(q);
      const open = q ? true : expanded.has(node.id);
      rows.push({ node, depth, open });
      if (open) walk(node.children, depth + 1, forceAll || (!!q && folderMatches));
    }
  };
  walk(nodes, 0, false);
  return rows;
}

/** Every folder with its depth, for "move to" and folder pickers. `exclude` hides a folder and its subtree. */
export function folderOptions(nodes: ScriptSummary[], exclude: string | null = null): FolderOption[] {
  const options: FolderOption[] = [];
  const walk = (list: ScriptSummary[], depth: number) => {
    for (const node of list) {
      if (node.type !== "folder" || node.id === exclude) continue;
      options.push({ id: node.id, depth, name: node.name });
      walk(node.children, depth + 1);
    }
  };
  walk(nodes, 0);
  return options;
}

/** A default script name from the first meaningful line of the text. */
export function suggestScriptName(body: string, max = 60): string {
  for (const raw of body.split(/\r?\n/)) {
    if (raw.startsWith("#!")) continue; // shebang
    const line = raw.replace(/^[\s#/*;:!-]+/, "").trim();
    if (line) return line.length > max ? `${line.slice(0, max - 1).trimEnd()}…` : line;
  }
  return "";
}
