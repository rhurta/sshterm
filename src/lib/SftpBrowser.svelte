<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { t } from "$lib/i18n/index.svelte";
  import type { ConnectionSummary, LocalDirectoryListing, LocalFileEntry, SftpDirectory, SftpEntry, SftpSessionInfo } from "$lib/types";

  let {
    connectionId = null,
    embedded = false,
    visible = true,
    container = null,
    focused = false,
    workspaceItemId = "",
    onactivate,
    onconnectionchange,
    onclose,
  }: {
    connectionId?: string | null;
    embedded?: boolean;
    visible?: boolean;
    /** Pane body this browser is attached to; re-parenting preserves its SFTP sessions. */
    container?: HTMLElement | null;
    focused?: boolean;
    workspaceItemId?: string;
    onactivate?: (id: string) => void;
    onconnectionchange?: (connectionName: string) => void;
    onclose: () => void;
  } = $props();

  let root = $state<HTMLDivElement | null>(null);
  let connections = $state<ConnectionSummary[]>([]);
  let sessions = $state<SftpSessionInfo[]>([]);
  let activeId = $state("");
  let activePane = $state<"local" | "remote">("remote");
  let directories = $state<Record<string, SftpDirectory>>({});
  let selectedPaths = $state<Record<string, string>>({});
  let search = $state("");
  let pathValue = $state("");
  let localDirectory = $state<LocalDirectoryListing | null>(null);
  let localPathValue = $state("");
  let localSearch = $state("");
  let localSelectedPaths = $state<string[]>([]);
  let localFocusedPath = $state("");
  let localSelectionAnchor = $state("");
  let localListing = $state(false);
  let localErrorMessage = $state("");
  let busy = $state(false);
  let listing = $state(false);
  let errorMessage = $state("");
  let statusMessage = $state("");
  let pickerOpen = $state(false);
  let pickerSearch = $state("");
  let nameMode = $state<"mkdir" | "rename" | null>(null);
  let namePane = $state<"local" | "remote">("remote");
  let nameTargetPath = $state("");
  let nameValue = $state("");
  let deleteTarget = $state<{ pane: "local" | "remote"; entries: Array<SftpEntry | LocalFileEntry> } | null>(null);
  let selectedRemotePaths = $state<Record<string, string[]>>({});
  let remoteSelectionAnchors = $state<Record<string, string>>({});
  type TransferProgress = {
    transfer_id: string; session_id: string; direction: "upload" | "download";
    status: "running" | "completed" | "cancelled" | "failed"; current_file: string;
    files_completed: number; files_total: number; bytes_transferred: number; bytes_total: number; error: string | null;
  };
  type TransferStartResult = { transfer_id: string | null; conflicts: string[] };
  type DeleteBatchResult = { deleted: string[]; failed: Array<{ path: string; detail: string }> };
  let transferJobs = $state<TransferProgress[]>([]);
  let transferQueueHeight = $state(168);
  let resizingTransferQueue = $state(false);
  let transferQueueResizeStartY = 0;
  let transferQueueResizeStartHeight = 168;
  let overwriteConfirm = $state<{ sessionId: string; direction: "upload" | "download"; sources: string[]; destination: string; conflicts: string[] } | null>(null);
  let overwriteConfirmButton: HTMLButtonElement | undefined = $state();
  let searchInput: HTMLInputElement | undefined = $state();
  let localSearchInput: HTMLInputElement | undefined = $state();
  let pickerInput: HTMLInputElement | undefined = $state();
  let pathInput: HTMLInputElement | undefined = $state();
  let localPathInput: HTMLInputElement | undefined = $state();
  let nameInput: HTMLInputElement | undefined = $state();
  let rowList: HTMLDivElement | undefined = $state();
  let remoteScrollTop = $state(0);
  const REMOTE_ROW_HEIGHT = 30;
  const REMOTE_WINDOW_ROWS = 120;
  const REMOTE_OVERSCAN_ROWS = 12;
  let localRowList: HTMLDivElement | undefined = $state();
  let browserDialog: HTMLElement | undefined = $state();
  let deleteCancelButton: HTMLButtonElement | undefined = $state();

  const activeSession = $derived(sessions.find((item) => item.session_id === activeId) ?? null);
  const activeDirectory = $derived(activeSession ? directories[activeSession.session_id] ?? null : null);
  const selectedEntry = $derived(
    activeDirectory && selectedPaths[activeId]
      ? activeDirectory.entries.find((entry) => entry.path === selectedPaths[activeId]) ?? null
      : null,
  );
  const selectedRemoteEntries = $derived(
    activeDirectory?.entries.filter((entry) => (selectedRemotePaths[activeId] ?? []).includes(entry.path)) ?? [],
  );
  const selectedLocalEntries = $derived(
    localDirectory?.entries.filter((entry) => localSelectedPaths.includes(entry.path)) ?? [],
  );
  const selectedLocalEntry = $derived(selectedLocalEntries.length === 1 ? selectedLocalEntries[0] : null);
  const localRoot = $derived.by(() => {
    const directory = localDirectory;
    if (!directory) return "";
    return directory.roots.find((root) => directory.current_path.toLocaleLowerCase().startsWith(root.toLocaleLowerCase())) ?? "";
  });
  const filteredLocalEntries = $derived.by(() => {
    const query = localSearch.trim().toLocaleLowerCase();
    const entries = localDirectory?.entries ?? [];
    return entries
      .filter((entry) => !query || entry.name.toLocaleLowerCase().includes(query))
      .sort((left, right) => Number(right.kind === "directory") - Number(left.kind === "directory") || left.name.localeCompare(right.name, undefined, { numeric: true, sensitivity: "base" }));
  });
  const activeTransferJobs = $derived(transferJobs.filter((job) => sessions.some((session) => session.session_id === job.session_id)));
  const filteredEntries = $derived.by(() => {
    const query = search.trim().toLocaleLowerCase();
    const entries = activeDirectory?.entries ?? [];
    if (!query) return entries;
    return entries.filter((entry) => entry.name.toLocaleLowerCase().includes(query));
  });
  const remoteVisibleWindow = $derived.by(() => {
    const entries = filteredEntries;
    const start = Math.max(0, Math.floor(remoteScrollTop / REMOTE_ROW_HEIGHT) - REMOTE_OVERSCAN_ROWS);
    const end = Math.min(entries.length, start + REMOTE_WINDOW_ROWS);
    return {
      entries: entries.slice(start, end),
      start,
      top: start * REMOTE_ROW_HEIGHT,
      bottom: (entries.length - end) * REMOTE_ROW_HEIGHT,
    };
  });
  const filteredConnections = $derived.by(() => {
    const query = pickerSearch.trim().toLocaleLowerCase();
    if (!query) return connections;
    return connections.filter((item) =>
      [item.name, item.host, item.user, item.user + "@" + item.host]
        .some((value) => value.toLocaleLowerCase().includes(query)),
    );
  });
  $effect(() => {
    if (deleteTarget) void tick().then(() => deleteCancelButton?.focus());
    if (overwriteConfirm) void tick().then(() => overwriteConfirmButton?.focus());
  });

  function message(error: unknown): string {
    return error instanceof Error ? error.message : String(error);
  }

  async function refreshConnections() {
    try {
      connections = await invoke<ConnectionSummary[]>("conn_list");
      return true;
    } catch (error) {
      errorMessage = message(error);
      return false;
    }
  }

  async function initialize() {
    const [loaded] = await Promise.all([refreshConnections(), loadLocalDirectory()]);
    if (!loaded) return;
    if (connectionId && connections.some((item) => item.id === connectionId)) {
      await openSession(connectionId);
    } else {
      await openConnectionPicker();
    }
    await tick();
    searchInput?.focus();
    searchInput?.select();
  }

  async function openSession(id: string) {
    if (!id || busy) return;
    const existing = sessions.find((item) => item.connection_id === id);
    if (existing) {
      activeId = existing.session_id;
      search = "";
      await tick();
      searchInput?.focus();
      return;
    }
    const connection = connections.find((item) => item.id === id);
    if (!connection) return;

    busy = true;
    errorMessage = "";
    statusMessage = "";
    try {
      const opened = await invoke<SftpSessionInfo>("sftp_open", { connectionId: id });
      sessions = [...sessions, opened];
      activeId = opened.session_id;
      onconnectionchange?.(opened.connection_name);
      search = "";
      pathValue = opened.current_path;
      selectedPaths = { ...selectedPaths, [opened.session_id]: "" };
      selectedRemotePaths = { ...selectedRemotePaths, [opened.session_id]: [] };
      remoteSelectionAnchors = { ...remoteSelectionAnchors, [opened.session_id]: "" };
      await listDirectory(opened.session_id, opened.current_path);
      if (opened.agent_binding === "legacy_unbound") {
        statusMessage = t("sftp.agentUnboundWarning");
      }
    } catch (error) {
      errorMessage = t("sftp.openFailed", { detail: message(error) });
    } finally {
      busy = false;
    }
    await tick();
    searchInput?.focus();
  }

  function resetRemoteScroll() {
    remoteScrollTop = 0;
    if (rowList) rowList.scrollTop = 0;
  }

  function updateRemoteScroll(event: Event) {
    remoteScrollTop = (event.currentTarget as HTMLDivElement).scrollTop;
  }

  async function listDirectory(sessionId: string, path: string) {
    resetRemoteScroll();
    listing = true;
    errorMessage = "";
    try {
      const result = await invoke<SftpDirectory>("sftp_list", {
        sessionId,
        remotePath: path,
      });
      directories = { ...directories, [sessionId]: result };
      if (activeId === sessionId) pathValue = result.current_path;
      statusMessage = t("sftp.itemCount", { count: result.entries.length });
    } catch (error) {
      errorMessage = t("sftp.listFailed", { detail: message(error) });
    } finally {
      listing = false;
    }
  }

  async function navigate(path: string) {
    if (!activeSession || busy || listing || !path.trim()) return;
    search = "";
    selectedPaths = { ...selectedPaths, [activeId]: "" };
    selectedRemotePaths = { ...selectedRemotePaths, [activeId]: [] };
    remoteSelectionAnchors = { ...remoteSelectionAnchors, [activeId]: "" };
    await listDirectory(activeId, path.trim());
  }

  async function navigateFromInput() {
    if (!pathValue.trim()) return;
    await navigate(pathValue);
  }

  function openParent() {
    if (activeDirectory?.parent_path) void navigate(activeDirectory.parent_path);
  }

  async function closeBrowser() {
    const ids = sessions.map((item) => item.session_id);
    sessions = [];
    activeId = "";
    for (const sessionId of ids) {
      try {
        await invoke("sftp_close", { sessionId });
      } catch {
        // The backend may have closed the sessions as part of vault locking.
      }
    }
    onclose();
  }

  async function openConnectionPicker() {
    if (!connections.length) {
      await refreshConnections();
    }
    pickerSearch = "";
    pickerOpen = true;
    await tick();
    pickerInput?.focus();
    pickerInput?.select();
  }

  function chooseEntry(entry: SftpEntry) {
    selectedPaths = { ...selectedPaths, [activeId]: entry.path };
  }

  function pathsBetween(orderedPaths: string[], anchor: string, target: string): string[] {
    const targetIndex = orderedPaths.indexOf(target);
    if (targetIndex < 0) return [];
    const anchorIndex = orderedPaths.indexOf(anchor);
    const start = anchorIndex < 0 ? targetIndex : Math.min(anchorIndex, targetIndex);
    const end = anchorIndex < 0 ? targetIndex : Math.max(anchorIndex, targetIndex);
    return orderedPaths.slice(start, end + 1);
  }

  function selectRemoteEntry(entry: SftpEntry, event: MouseEvent) {
    chooseEntry(entry);
    const selected = selectedRemotePaths[activeId] ?? [];
    if (event.shiftKey) {
      const visiblePaths = filteredEntries.map((item) => item.path);
      const savedAnchor = remoteSelectionAnchors[activeId];
      const anchor = savedAnchor && visiblePaths.includes(savedAnchor) ? savedAnchor : entry.path;
      const range = pathsBetween(visiblePaths, anchor, entry.path);
      const keep = event.ctrlKey || event.metaKey ? selected : [];
      selectedRemotePaths = { ...selectedRemotePaths, [activeId]: [...new Set([...keep, ...range])] };
      if (anchor !== savedAnchor) remoteSelectionAnchors = { ...remoteSelectionAnchors, [activeId]: anchor };
    } else if (event.ctrlKey || event.metaKey) {
      selectedRemotePaths = { ...selectedRemotePaths, [activeId]: selected.includes(entry.path) ? selected.filter((path) => path !== entry.path) : [...selected, entry.path] };
      remoteSelectionAnchors = { ...remoteSelectionAnchors, [activeId]: entry.path };
    } else {
      selectedRemotePaths = { ...selectedRemotePaths, [activeId]: [entry.path] };
      remoteSelectionAnchors = { ...remoteSelectionAnchors, [activeId]: entry.path };
    }
  }

  function openEntry(entry: SftpEntry) {
    chooseEntry(entry);
    selectedRemotePaths = { ...selectedRemotePaths, [activeId]: [entry.path] };
    remoteSelectionAnchors = { ...remoteSelectionAnchors, [activeId]: entry.path };
    if (entry.kind === "directory") void navigate(entry.path);
  }

  async function refreshDirectory() {
    if (activeDirectory) await navigate(activeDirectory.current_path);
  }

  async function showNameDialog(mode: "mkdir" | "rename", pane: "local" | "remote" = "remote", entry?: SftpEntry | LocalFileEntry) {
    if (busy) return;
    if (pane === "local") {
      if (!localDirectory || localListing) return;
      const selected = entry && entry.kind !== "symlink" && entry.kind !== "other"
        ? entry
        : selectedLocalEntry;
      if (mode === "rename" && !selected) return;
      if (mode === "rename" && selected) localSelectedPaths = [selected.path];
      nameTargetPath = mode === "rename" ? selected?.path ?? "" : "";
      nameValue = mode === "rename" ? selected?.name ?? "" : "";
    } else {
      if (!activeSession || listing) return;
      const selected = entry && "permissions" in entry ? entry : (selectedRemoteEntries.length === 1 ? selectedRemoteEntries[0] : selectedEntry);
      if (mode === "rename" && !selected) return;
      if (mode === "rename" && selected) {
        selectedPaths = { ...selectedPaths, [activeId]: selected.path };
        selectedRemotePaths = { ...selectedRemotePaths, [activeId]: [selected.path] };
      }
      nameTargetPath = mode === "rename" ? selected?.path ?? "" : "";
      nameValue = mode === "rename" ? selected?.name ?? "" : "";
    }
    nameMode = mode;
    namePane = pane;
    await tick();
    nameInput?.focus();
    nameInput?.select();
  }

  async function saveName() {
    if (!nameMode || busy) return;
    const value = nameValue.trim();
    if (!value) return;
    busy = true;
    errorMessage = "";
    try {
      if (namePane === "local") {
        const currentPath = localDirectory?.current_path;
        if (!currentPath) return;
        if (nameMode === "mkdir") {
          await invoke("local_create_directory", { parentPath: currentPath, name: value });
          statusMessage = t("sftp.localFolderCreated", { name: value });
        } else if (nameTargetPath) {
          await invoke("local_rename_entry", { path: nameTargetPath, newName: value });
          statusMessage = t("sftp.localRenamed", { name: value });
        } else {
          return;
        }
        nameMode = null;
        await loadLocalDirectory(currentPath);
      } else {
        const currentPath = activeDirectory?.current_path;
        if (!activeSession || !currentPath) return;
        if (nameMode === "mkdir") {
          await invoke("sftp_mkdir", { sessionId: activeId, remotePath: currentPath, name: value });
          statusMessage = t("sftp.folderCreated", { name: value });
        } else if (nameTargetPath) {
          await invoke("sftp_rename", { sessionId: activeId, remotePath: nameTargetPath, newName: value });
          statusMessage = t("sftp.renamed", { name: value });
        } else {
          return;
        }
        nameMode = null;
        selectedPaths = { ...selectedPaths, [activeId]: "" };
        selectedRemotePaths = { ...selectedRemotePaths, [activeId]: [] };
        await listDirectory(activeId, currentPath);
      }
    } catch (error) {
      errorMessage = t("sftp.operationFailed", { detail: message(error) });
    } finally {
      busy = false;
    }
  }

  function requestDelete(pane: "local" | "remote", entry?: SftpEntry | LocalFileEntry) {
    if (busy) return;
    if (pane === "local") {
      const entries = entry && entry.kind !== "symlink" && entry.kind !== "other"
        ? [entry]
        : selectedLocalEntries.filter((item) => item.kind !== "symlink" && item.kind !== "other");
      if (!entries.length) return;
      deleteTarget = { pane, entries };
    } else {
      const entries = entry
        ? [entry]
        : selectedRemoteEntries;
      if (!entries.length) return;
      deleteTarget = { pane, entries };
    }
  }

  async function dismissDelete() {
    const pane = deleteTarget?.pane ?? activePane;
    deleteTarget = null;
    await tick();
    if (pane === "local") localSearchInput?.focus();
    else searchInput?.focus();
  }

  async function confirmDelete() {
    const target = deleteTarget;
    if (!target || busy) return;
    if (target.pane === "remote" && !activeSession) return;
    const currentRemotePath = activeDirectory?.current_path ?? ".";
    const currentLocalPath = localDirectory?.current_path ?? "";
    busy = true;
    errorMessage = "";
    try {
      let result: DeleteBatchResult;
      if (target.pane === "remote") {
        result = await invoke<DeleteBatchResult>("sftp_delete_many", {
          sessionId: activeId,
          remotePaths: target.entries.map((entry) => entry.path),
        });
        selectedPaths = { ...selectedPaths, [activeId]: "" };
        selectedRemotePaths = { ...selectedRemotePaths, [activeId]: [] };
        if (result.deleted.length) statusMessage = t("sftp.deletedCount", { count: result.deleted.length });
        if (result.failed.length) errorMessage = t("sftp.deletePartial", {
          deleted: result.deleted.length,
          failed: result.failed.length,
          detail: result.failed.slice(0, 3).map((item) => item.path + ": " + item.detail).join("; "),
        });
      } else {
        result = await invoke<DeleteBatchResult>("local_delete_many", {
          paths: target.entries.map((entry) => entry.path),
        });
        localSelectedPaths = [];
        localSelectionAnchor = "";
        if (result.deleted.length) statusMessage = t("sftp.localDeletedCount", { count: result.deleted.length });
        if (result.failed.length) errorMessage = t("sftp.localDeletePartial", {
          deleted: result.deleted.length,
          failed: result.failed.length,
          detail: result.failed.slice(0, 3).map((item) => item.path + ": " + item.detail).join("; "),
        });
      }
      deleteTarget = null;
      if (target.pane === "remote" && result.deleted.length) await listDirectory(activeId, currentRemotePath);
      else if (target.pane === "local" && result.deleted.length) await loadLocalDirectory(currentLocalPath);
      await tick();
      if (target.pane === "local") localSearchInput?.focus();
      else searchInput?.focus();
    } catch (error) {
      errorMessage = t("sftp.deleteFailed", { detail: message(error) });
    } finally {
      busy = false;
    }
  }

  async function loadLocalDirectory(path: string | null = null) {
    if (localListing) return;
    localListing = true;
    localErrorMessage = "";
    try {
      const result = await invoke<LocalDirectoryListing>("local_list_directory", { path });
      localDirectory = result;
      localPathValue = result.current_path;
      localSelectedPaths = [];
      localSelectionAnchor = "";
      localFocusedPath = result.entries.find((entry) => entry.kind !== "symlink" && entry.kind !== "other")?.path ?? "";
    } catch (error) {
      localErrorMessage = message(error);
    } finally {
      localListing = false;
    }
  }

  async function navigateLocalFromInput() {
    if (!localPathValue.trim()) return;
    localSearch = "";
    await loadLocalDirectory(localPathValue.trim());
    await tick();
    localSearchInput?.focus();
  }

  function openLocalParent() {
    if (localDirectory?.parent_path) void loadLocalDirectory(localDirectory.parent_path);
  }

  function refreshLocalDirectory() {
    if (localDirectory) void loadLocalDirectory(localDirectory.current_path);
  }

  function toggleLocalSelection(path: string, checked: boolean) {
    localSelectedPaths = checked
      ? [...new Set([...localSelectedPaths, path])]
      : localSelectedPaths.filter((item) => item !== path);
    localFocusedPath = path;
    localSelectionAnchor = path;
  }

  function selectLocalEntry(entry: LocalDirectoryListing["entries"][number], event: MouseEvent) {
    localFocusedPath = entry.path;
    if (entry.kind === "symlink" || entry.kind === "other") return;
    if (event.shiftKey) {
      const eligiblePaths = filteredLocalEntries.filter((item) => item.kind !== "symlink" && item.kind !== "other").map((item) => item.path);
      const savedAnchor = localSelectionAnchor;
      const anchor = savedAnchor && eligiblePaths.includes(savedAnchor) ? savedAnchor : entry.path;
      const range = pathsBetween(eligiblePaths, anchor, entry.path);
      const keep = event.ctrlKey || event.metaKey ? localSelectedPaths : [];
      localSelectedPaths = [...new Set([...keep, ...range])];
      if (anchor !== savedAnchor) localSelectionAnchor = anchor;
    } else if (event.ctrlKey || event.metaKey) {
      localSelectedPaths = localSelectedPaths.includes(entry.path)
        ? localSelectedPaths.filter((path) => path !== entry.path)
        : [...localSelectedPaths, entry.path];
      localSelectionAnchor = entry.path;
    } else {
      localSelectedPaths = [entry.path];
      localSelectionAnchor = entry.path;
    }
  }

  async function openLocalEntry(entry: LocalDirectoryListing["entries"][number]) {
    if (entry.kind === "directory") {
      localSearch = "";
      await loadLocalDirectory(entry.path);
      await tick();
      localSearchInput?.focus();
    }
  }

  function focusLocalRow(index: number) {
    const rows = localRowList?.querySelectorAll<HTMLButtonElement>("button.local-entry-button");
    if (!rows?.length) return;
    const target = Math.max(0, Math.min(index, rows.length - 1));
    rows[target]?.focus();
  }

  function onLocalSearchKeydown(event: KeyboardEvent) {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      focusLocalRow(0);
    } else if (event.key === "Enter" && filteredLocalEntries.length > 0) {
      event.preventDefault();
      const first = filteredLocalEntries[0];
      if (first) openLocalEntry(first);
    } else if (event.key === "Escape" && localSearch) {
      event.preventDefault();
      event.stopPropagation();
      localSearch = "";
    }
  }

  function onLocalRowKeydown(event: KeyboardEvent, index: number, entry: LocalDirectoryListing["entries"][number]) {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      focusLocalRow(index + 1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      if (index === 0) localSearchInput?.focus();
      else focusLocalRow(index - 1);
    } else if (event.key === "Home") {
      event.preventDefault();
      focusLocalRow(0);
    } else if (event.key === "End") {
      event.preventDefault();
      focusLocalRow(filteredLocalEntries.length - 1);
    } else if (event.key === "Enter") {
      event.preventDefault();
      if (entry.kind === "directory") openLocalEntry(entry);
      else if (entry.kind !== "symlink" && entry.kind !== "other") toggleLocalSelection(entry.path, !localSelectedPaths.includes(entry.path));
    } else if (event.key === " ") {
      event.preventDefault();
      if (entry.kind !== "symlink" && entry.kind !== "other") toggleLocalSelection(entry.path, !localSelectedPaths.includes(entry.path));
    } else if (event.key === "F2") {
      event.preventDefault();
      event.stopPropagation();
      if (entry.kind !== "symlink" && entry.kind !== "other") void showNameDialog("rename", "local", entry);
    } else if (event.key === "Delete") {
      event.preventDefault();
      event.stopPropagation();
      if (localSelectedPaths.includes(entry.path)) requestDelete("local");
      else requestDelete("local", entry);
    } else if (event.key === "Backspace") {
      event.preventDefault();
      openLocalParent();
    } else if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "a") {
      event.preventDefault();
      localSelectedPaths = filteredLocalEntries.filter((item) => item.kind !== "symlink" && item.kind !== "other").map((item) => item.path);
    }
  }

  function onRemoteDrop(event: DragEvent) {
    event.preventDefault();
    const source = event.dataTransfer?.getData("application/x-sshterm-local-path");
    if (source && activeDirectory && activeId) {
      void startTransfer("upload", false, [source], activeDirectory.current_path, activeId);
    }
  }

  function onLocalDrop(event: DragEvent) {
    event.preventDefault();
    const source = event.dataTransfer?.getData("application/x-sshterm-remote-path");
    if (source && activeId && localDirectory) {
      void startTransfer("download", false, [source], localDirectory.current_path, activeId);
    }
  }

  function toggleRemoteSelection(path: string, checked: boolean) {
    const selected = selectedRemotePaths[activeId] ?? [];
    selectedRemotePaths = { ...selectedRemotePaths, [activeId]: checked ? [...new Set([...selected, path])] : selected.filter((item) => item !== path) };
    selectedPaths = { ...selectedPaths, [activeId]: path };
    remoteSelectionAnchors = { ...remoteSelectionAnchors, [activeId]: path };
  }

  function updateTransferProgress(progress: TransferProgress) {
    const index = transferJobs.findIndex((job) => job.transfer_id === progress.transfer_id);
    if (index < 0) {
      const next = [...transferJobs, progress];
      transferJobs = [
        ...next.filter((job) => job.status === "running"),
        ...next.filter((job) => job.status !== "running").slice(-20),
      ];
    } else transferJobs = transferJobs.map((job) => job.transfer_id === progress.transfer_id ? progress : job);
    if (progress.status === "completed") {
      statusMessage = t("sftp.transferCompleted", { count: progress.files_completed });
      if (progress.direction === "download") {
        if (activeId === progress.session_id) selectedRemotePaths = { ...selectedRemotePaths, [activeId]: [] };
        if (localDirectory) void loadLocalDirectory(localDirectory.current_path);
      }
      if (activeId === progress.session_id) void refreshDirectory();
    } else if (progress.status === "failed") {
      errorMessage = progress.error ?? t("sftp.transferFailed");
    } else if (progress.status === "cancelled") {
      statusMessage = t("sftp.transferCancelled");
    }
  }

  async function startTransfer(
    direction: "upload" | "download", overwrite = false,
    suppliedSources?: string[], suppliedDestination?: string, sessionId = activeId,
  ) {
    const session = sessions.find((item) => item.session_id === sessionId);
    if (!session || busy) return;
    const sources = suppliedSources ?? (direction === "upload" ? localSelectedPaths : (selectedRemotePaths[sessionId] ?? []));
    const destination = suppliedDestination ?? (direction === "upload" ? directories[sessionId]?.current_path ?? "" : localDirectory?.current_path ?? "");
    if (!sources.length || !destination) return;
    busy = true;
    errorMessage = "";
    try {
      const result = await invoke<TransferStartResult>("sftp_transfer_start", { sessionId, direction, sources, destination, overwrite });
      if (result.conflicts.length && !overwrite) {
        overwriteConfirm = { sessionId, direction, sources, destination, conflicts: result.conflicts };
      } else if (result.transfer_id) {
        if (!transferJobs.some((job) => job.transfer_id === result.transfer_id)) updateTransferProgress({
          transfer_id: result.transfer_id, session_id: sessionId, direction, status: "running", current_file: "",
          files_completed: 0, files_total: 0, bytes_transferred: 0, bytes_total: 0, error: null,
        });
        statusMessage = t("sftp.transferStarted");
      }
    } catch (error) {
      errorMessage = t("sftp.transferFailed") + ": " + message(error);
    } finally {
      busy = false;
    }
  }

  async function confirmOverwrite() {
    const pending = overwriteConfirm;
    if (!pending) return;
    overwriteConfirm = null;
    await startTransfer(pending.direction, true, pending.sources, pending.destination, pending.sessionId);
  }

  async function cancelTransfer(transferId: string) {
    try { await invoke("sftp_transfer_cancel", { transferId }); }
    catch (error) { errorMessage = t("sftp.transferCancelFailed", { detail: message(error) }); }
  }

  function transferPercent(job: TransferProgress): number {
    if (job.bytes_total > 0) return Math.min(100, Math.round(job.bytes_transferred * 100 / job.bytes_total));
    if (job.files_total > 0) return Math.min(100, Math.round(job.files_completed * 100 / job.files_total));
    return job.status === "completed" ? 100 : 0;
  }

  const MIN_TRANSFER_QUEUE_HEIGHT = 96;
  function maxTransferQueueHeight(): number {
    return Math.max(MIN_TRANSFER_QUEUE_HEIGHT, Math.floor((browserDialog?.clientHeight ?? 780) * 0.55));
  }

  function clampTransferQueueHeight(value: number): number {
    return Math.max(MIN_TRANSFER_QUEUE_HEIGHT, Math.min(maxTransferQueueHeight(), value));
  }

  function startTransferQueueResize(event: PointerEvent) {
    event.preventDefault();
    transferQueueResizeStartY = event.clientY;
    transferQueueResizeStartHeight = transferQueueHeight;
    resizingTransferQueue = true;
    if (event.currentTarget instanceof HTMLElement) event.currentTarget.setPointerCapture(event.pointerId);
  }

  function moveTransferQueueResize(event: PointerEvent) {
    if (!resizingTransferQueue) return;
    transferQueueHeight = clampTransferQueueHeight(transferQueueResizeStartHeight + transferQueueResizeStartY - event.clientY);
  }

  function endTransferQueueResize(event: PointerEvent) {
    if (!resizingTransferQueue) return;
    resizingTransferQueue = false;
    if (event.currentTarget instanceof HTMLElement && event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  }

  function onTransferQueueResizeKeydown(event: KeyboardEvent) {
    const step = event.shiftKey ? 32 : 12;
    if (event.key === "ArrowUp") transferQueueHeight = clampTransferQueueHeight(transferQueueHeight + step);
    else if (event.key === "ArrowDown") transferQueueHeight = clampTransferQueueHeight(transferQueueHeight - step);
    else if (event.key === "Home") transferQueueHeight = MIN_TRANSFER_QUEUE_HEIGHT;
    else if (event.key === "End") transferQueueHeight = maxTransferQueueHeight();
    else return;
    event.preventDefault();
  }

  function formatSize(size: number | null): string {
    if (size === null) return "—";
    if (size < 1024) return size + " B";
    const units = ["KB", "MB", "GB", "TB"];
    let value = size;
    let unit = -1;
    do {
      value /= 1024;
      unit += 1;
    } while (value >= 1024 && unit < units.length - 1);
    return value.toFixed(value >= 10 ? 0 : 1) + " " + units[unit];
  }

  function formatModified(seconds: number | null): string {
    if (seconds === null) return "—";
    const date = new Date(seconds * 1000);
    return Number.isNaN(date.getTime()) ? "—" : date.toLocaleString();
  }

  function formatPermissions(value: number | null): string {
    return value === null ? "—" : (value & 0o7777).toString(8).padStart(4, "0");
  }

  function focusRow(index: number) {
    if (!rowList || !filteredEntries.length) return;
    const target = Math.max(0, Math.min(index, filteredEntries.length - 1));
    const rowTop = target * REMOTE_ROW_HEIGHT;
    const rowBottom = rowTop + REMOTE_ROW_HEIGHT;
    if (rowTop < rowList.scrollTop) rowList.scrollTop = rowTop;
    else if (rowBottom > rowList.scrollTop + rowList.clientHeight) {
      rowList.scrollTop = rowBottom - rowList.clientHeight;
    }
    remoteScrollTop = rowList.scrollTop;
    void tick().then(() => {
      rowList?.querySelector<HTMLButtonElement>(`button[data-entry-index="${target}"]`)?.focus();
    });
  }

  function onSearchKeydown(event: KeyboardEvent) {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      focusRow(0);
    } else if (event.key === "Enter" && filteredEntries.length > 0) {
      event.preventDefault();
      const first = filteredEntries[0];
      if (first) openEntry(first);
    } else if (event.key === "Escape" && search) {
      event.preventDefault();
      event.stopPropagation();
      search = "";
    }
  }

  function onRowKeydown(event: KeyboardEvent, index: number, entry: SftpEntry) {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      focusRow(index + 1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      if (index === 0) searchInput?.focus();
      else focusRow(index - 1);
    } else if (event.key === "Home") {
      event.preventDefault();
      focusRow(0);
    } else if (event.key === "End") {
      event.preventDefault();
      focusRow(filteredEntries.length - 1);
    } else if (event.key === "Enter") {
      event.preventDefault();
      openEntry(entry);
    } else if (event.key === "F2") {
      event.preventDefault();
      event.stopPropagation();
      chooseEntry(entry);
      void showNameDialog("rename", "remote", entry);
    } else if (event.key === "Delete") {
      event.preventDefault();
      event.stopPropagation();
      if ((selectedRemotePaths[activeId] ?? []).includes(entry.path)) requestDelete("remote");
      else requestDelete("remote", entry);
    } else if (event.key === "Backspace") {
      event.preventDefault();
      openParent();
    }
  }

  function onBrowserKeydown(event: KeyboardEvent) {
    if (!visible || !focused || !root?.contains(document.activeElement)) return;
    if (event.key === "Tab" && (!embedded || pickerOpen || nameMode || overwriteConfirm || deleteTarget)) {
      const activeDialog = browserDialog?.querySelector<HTMLElement>(".sftp-small-dialog") ?? browserDialog;
      const focusable = Array.from(
        activeDialog?.querySelectorAll<HTMLElement>(
          'button:not([disabled]), input:not([disabled]), [tabindex]:not([tabindex="-1"])',
        ) ?? [],
      ).filter((element) => element.offsetParent !== null);
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (!first || !last) {
        event.preventDefault();
        activeDialog?.focus();
      } else if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
      return;
    }
    if (pickerOpen || nameMode || deleteTarget || overwriteConfirm) {
      if (event.key === "Escape") {
        event.preventDefault();
        if (pickerOpen) {
          if (pickerSearch) pickerSearch = "";
          else pickerOpen = false;
        } else if (nameMode) {
          nameMode = null;
        } else if (overwriteConfirm) {
          overwriteConfirm = null;
        } else {
          void dismissDelete();
        }
      }
      return;
    }
    const target = event.target;
    const localTarget = target instanceof Element && target.closest(".local-pane") !== null;
    if (event.ctrlKey && !event.altKey && event.key.toLowerCase() === "l") {
      event.preventDefault();
      const input = localTarget ? localPathInput : pathInput;
      input?.focus();
      input?.select();
      return;
    }
    if (localTarget && event.key !== "Escape") return;
    const editingText = target instanceof HTMLInputElement
      || target instanceof HTMLTextAreaElement
      || (target instanceof HTMLElement && target.isContentEditable);
    if (editingText && event.key !== "Escape") return;
    if (event.key === "Escape") {
      event.preventDefault();
      void closeBrowser();
    } else if (event.key === "F2" && activePane === "remote" && selectedRemoteEntries.length === 1) {
      event.preventDefault();
      void showNameDialog("rename", "remote");
    } else if (event.key === "Delete" && activePane === "remote" && selectedRemoteEntries.length > 0) {
      event.preventDefault();
      requestDelete("remote");
    } else if (event.key === "Backspace" && activePane === "remote" && !(event.target instanceof HTMLInputElement)) {
      event.preventDefault();
      openParent();
    }
  }

  $effect(() => {
    if (visible && focused) void tick().then(() => searchInput?.focus());
  });

  // Keep this browser mounted while its workspace tab moves between panes.
  // Re-parenting the root preserves active SFTP sessions and transfer state.
  $effect(() => {
    const target = container;
    const node = root;
    if (!target || !node || node.parentElement === target) return;
    target.appendChild(node);
    untrack(() => { if (visible && focused) void tick().then(() => searchInput?.focus()); });
  });

  onMount(() => {
    let mounted = true;
    let stopProgress: (() => void) | undefined;
    void listen<TransferProgress>("sftp:progress", (event) => {
      if (mounted && sessions.some((session) => session.session_id === event.payload.session_id)) updateTransferProgress(event.payload);
    }).then((stop) => { if (mounted) stopProgress = stop; else stop(); })
      .catch((error) => { errorMessage = t("sftp.transferEventsFailed", { detail: message(error) }); });
    void initialize();
    return () => {
      mounted = false;
      stopProgress?.();
      // This root may have been re-parented into a workspace pane. Detach it
      // explicitly so closing this tab cannot leave an orphaned SFTP overlay.
      root?.remove();
      for (const session of sessions) void invoke("sftp_close", { sessionId: session.session_id }).catch(() => {});
    };
  });
</script>

<svelte:window onkeydown={onBrowserKeydown} />

<div bind:this={root} class="sftp-backdrop" class:embedded class:hidden={!visible}>
  <section bind:this={browserDialog} class="sftp-browser" role="dialog" aria-modal={embedded ? "false" : "true"} aria-labelledby="sftp-title" onpointerdown={() => onactivate?.(workspaceItemId)} onfocusin={() => onactivate?.(workspaceItemId)}>
    <header class="sftp-header">
      <div class="sftp-heading">
        <span class="sftp-mark" aria-hidden="true">
          <svg viewBox="0 0 24 24"><path d="M4 7h6l2 2h8v9H4z"/><path d="M4 7V5h6l2 2"/></svg>
        </span>
        <div class="sftp-title-block">
          <h2 id="sftp-title">{t("sftp.title")}</h2>
          <span class="sftp-subtitle">
            {#if activeSession}
              {activeSession.user}@{activeSession.host}:{activeSession.port}
              {#if activeSession.jump_names.length}
                · {t("sftp.via")} {activeSession.jump_names.join(" → ")}
              {/if}
            {:else}
              {t("sftp.selectConnection")}
            {/if}
          </span>
        </div>
      </div>
    </header>

    {#if activeSession?.agent_binding === "legacy_unbound"}
      <div class="sftp-warning" role="note">{t("sftp.agentUnboundWarning")}</div>
    {/if}

    {#if errorMessage}
      <div class="sftp-error" role="alert">
        <span>{t("sftp.error")}: {errorMessage}</span>
        <button class="dismiss-error" title={t("workspace.close")} aria-label={t("workspace.close")} onclick={() => (errorMessage = "")}>×</button>
      </div>
    {/if}

    <div class="sftp-content">
      <div class="sftp-dual-panes">
        <section class="file-pane local-pane" class:active-pane={activePane === "local"} aria-label={t("sftp.localPane")} onpointerdown={() => (activePane = "local")} onfocusin={() => (activePane = "local")}>
          <header class="pane-heading">
            <div class="pane-heading-title">
              <strong>{t("sftp.localPane")}</strong>
              <span>{localDirectory ? t("sftp.itemCount", { count: localDirectory.entries.length }) : t("sftp.loading")}</span>
            </div>
            <div class="pane-heading-actions">
              <button class="tool-button" title={t("sftp.localParent")} aria-label={t("sftp.localParent")} onclick={openLocalParent} disabled={!localDirectory?.parent_path || localListing}>
                <svg viewBox="0 0 24 24"><path d="M19 12H5m7-7-7 7 7 7"/></svg>
              </button>
              <button class="tool-button" title={t("sftp.localRefresh")} aria-label={t("sftp.localRefresh")} onclick={refreshLocalDirectory} disabled={!localDirectory || localListing}>
                <svg viewBox="0 0 24 24"><path d="M20 7v5h-5M4 17v-5h5"/><path d="M5.6 9a7 7 0 0 1 12-2L20 12M4 12l2.4 5a7 7 0 0 0 12-2"/></svg>
              </button>
              <select class="local-root-select" value={localRoot} aria-label={t("sftp.localDrives")} title={t("sftp.localDrives")} onchange={(event) => { const root = event.currentTarget.value; if (root) void loadLocalDirectory(root); }}>
                <option value="">{t("sftp.localDrives")}</option>
                {#each localDirectory?.roots ?? [] as root (root)}<option value={root}>{root}</option>{/each}
              </select>
            </div>
          </header>
          <div class="pane-actions" aria-label={t("sftp.localOperations")}>
            <button class="tool-button text-tool" title={t("sftp.newFolder")} onclick={() => void showNameDialog("mkdir", "local")} disabled={!localDirectory || localListing || busy}>
              <svg viewBox="0 0 24 24"><path d="M3 6h7l2 2h9v11H3z"/><path d="M12 11v5m-2.5-2.5h5"/></svg><span>{t("sftp.newFolder")}</span>
            </button>
            <button class="tool-button text-tool" title={t("sftp.rename")} onclick={() => void showNameDialog("rename", "local")} disabled={!selectedLocalEntry || busy || localListing}>
              <svg viewBox="0 0 24 24"><path d="m4 16-.8 4 4-.8L19.5 7a2.1 2.1 0 0 0-3-3z"/><path d="m14.5 6.5 3 3"/></svg><span>{t("sftp.rename")}</span>
            </button>
            <button class="tool-button text-tool danger-tool" title={t("sftp.delete")} onclick={() => requestDelete("local")} disabled={!selectedLocalEntries.length || busy || localListing}>
              <svg viewBox="0 0 24 24"><path d="M4 7h16m-10 4v6m4-6v6M6 7l1 13h10l1-13M9 7V4h6v3"/></svg><span>{t("sftp.delete")}{selectedLocalEntries.length ? " (" + selectedLocalEntries.length + ")" : ""}</span>
            </button>
            <button class="tool-button text-tool" title={t("sftp.upload")} onclick={() => void startTransfer("upload")} disabled={!activeSession || !localSelectedPaths.length || !activeDirectory || busy}>
              <svg viewBox="0 0 24 24"><path d="M12 15V3m-4 4 4-4 4 4M4 17v4h16v-4"/></svg><span>{t("sftp.upload")}{localSelectedPaths.length ? " (" + localSelectedPaths.length + ")" : ""}</span>
            </button>
          </div>
          <form class="pane-path-form" onsubmit={(event) => { event.preventDefault(); void navigateLocalFromInput(); }}>
            <input bind:this={localPathInput} bind:value={localPathValue} aria-label={t("sftp.localPath")} placeholder={t("sftp.localPathPlaceholder")} disabled={localListing} spellcheck="false" />
            <button type="submit" title={t("sftp.go")} aria-label={t("sftp.go")} disabled={localListing}>{t("sftp.go")}</button>
          </form>
          <div class="local-list-head" role="row">
            <span>{t("sftp.name")}</span>
            <span>{t("sftp.size")}</span>
            <span>{t("sftp.modified")}</span>
          </div>
          <div class="local-list" bind:this={localRowList} role="list" aria-label={t("sftp.localEntries")} ondragover={(event) => event.preventDefault()} ondrop={onLocalDrop}>
            {#if localListing}
              <div class="list-loading"><span class="spinner"></span>{t("sftp.loading")}</div>
            {:else if localErrorMessage}
              <div class="sftp-empty-list local-list-error">{localErrorMessage}</div>
            {:else if filteredLocalEntries.length === 0}
              <div class="sftp-empty-list">{localSearch ? t("sftp.noSearchResults") : t("sftp.emptyDirectory")}</div>
            {:else}
              {#each filteredLocalEntries as entry, index (entry.path)}
                <div class="local-entry" class:selected={localSelectedPaths.includes(entry.path)} class:focused={localFocusedPath === entry.path}>
                  <div class="entry-name-cell">
                    <input class="entry-select" type="checkbox" checked={localSelectedPaths.includes(entry.path)} disabled={entry.kind === "symlink" || entry.kind === "other"} aria-label={t("sftp.selectLocalEntry", { name: entry.name })} onchange={(event) => toggleLocalSelection(entry.path, event.currentTarget.checked)} />
                    <button class="entry-name local-entry-button" class:directory-entry={entry.kind === "directory"} class:symlink-entry={entry.kind === "symlink"} tabindex={localFocusedPath === entry.path || (!localFocusedPath && index === 0) ? 0 : -1} draggable={entry.kind === "file" || entry.kind === "directory"} aria-label={entry.name + (entry.kind === "directory" ? ", " + t("sftp.directory") : "")} onclick={(event) => selectLocalEntry(entry, event)} ondblclick={() => openLocalEntry(entry)} onfocus={() => (localFocusedPath = entry.path)} onkeydown={(event) => onLocalRowKeydown(event, index, entry)} ondragstart={(event) => { event.dataTransfer?.setData("application/x-sshterm-local-path", entry.path); if (event.dataTransfer) event.dataTransfer.effectAllowed = "copy"; }}>
                      <span class="entry-icon" aria-hidden="true">
                        {#if entry.kind === "directory"}
                          <svg viewBox="0 0 24 24"><path d="M3 6h7l2 2h9v12H3z"/><path d="M3 6V4h7l2 2"/></svg>
                        {:else if entry.kind === "symlink"}
                          <svg viewBox="0 0 24 24"><path d="M10 13a4 4 0 0 0 6 0l3-3a4 4 0 0 0-6-6l-1.5 1.5"/><path d="M14 11a4 4 0 0 0-6 0l-3 3a4 4 0 0 0 6 6l1.5-1.5"/></svg>
                        {:else}
                          <svg viewBox="0 0 24 24"><path d="M6 3h8l4 4v14H6z"/><path d="M14 3v5h4M9 13h6m-6 4h6"/></svg>
                        {/if}
                      </span>
                      <span class="entry-label" title={entry.name}>{entry.name}</span>
                    </button>
                  </div>
                  <span class="entry-meta size-meta">{entry.kind === "directory" ? "—" : formatSize(entry.size)}</span>
                  <span class="entry-meta modified-meta" title={formatModified(entry.modified)}>{formatModified(entry.modified)}</span>
                </div>
              {/each}
            {/if}
          </div>
          <footer class="pane-footer">
            <div class="sftp-filter">
              <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="10.8" cy="10.8" r="6.8"/><path d="m16 16 5 5"/></svg>
              <input bind:this={localSearchInput} bind:value={localSearch} onkeydown={onLocalSearchKeydown} placeholder={t("sftp.localSearch")} aria-label={t("sftp.localSearch")} disabled={!localDirectory} />
              {#if localSearch}<button title={t("sftp.clearSearch")} aria-label={t("sftp.clearSearch")} onclick={() => { localSearch = ""; localSearchInput?.focus(); }}>×</button>{/if}
            </div>
            <span class="footer-status">{t("sftp.localSelectedCount", { count: localSelectedPaths.length })}</span>
          </footer>
        </section>

        <section class="file-pane remote-pane" class:active-pane={activePane === "remote"} aria-label={t("sftp.remotePane")} onpointerdown={() => (activePane = "remote")} onfocusin={() => (activePane = "remote")}>
          <header class="pane-heading">
            <div class="pane-heading-title">
              <strong>{t("sftp.remotePane")}</strong>
              <span>{activeSession ? activeSession.user + "@" + activeSession.host + ":" + activeSession.port : t("sftp.selectConnection")}</span>
            </div>
            <div class="pane-heading-actions">
              <button class="tool-button" title={t("sftp.parent")} aria-label={t("sftp.parent")} onclick={openParent} disabled={!activeDirectory?.parent_path || busy || listing}>
                <svg viewBox="0 0 24 24"><path d="M19 12H5m7-7-7 7 7 7"/></svg>
              </button>
              <button class="tool-button" title={t("sftp.refresh")} aria-label={t("sftp.refresh")} onclick={() => void refreshDirectory()} disabled={!activeSession || busy || listing}>
                <svg viewBox="0 0 24 24"><path d="M20 7v5h-5M4 17v-5h5"/><path d="M5.6 9a7 7 0 0 1 12-2L20 12M4 12l2.4 5a7 7 0 0 0 12-2"/></svg>
              </button>
            </div>
          </header>
          <div class="pane-actions" aria-label={t("sftp.remoteOperations")}>
            <button class="tool-button text-tool" title={t("sftp.newFolder")} onclick={() => void showNameDialog("mkdir", "remote")} disabled={!activeSession || busy || listing}>
              <svg viewBox="0 0 24 24"><path d="M3 6h7l2 2h9v11H3z"/><path d="M12 11v5m-2.5-2.5h5"/></svg><span>{t("sftp.newFolder")}</span>
            </button>
            <button class="tool-button text-tool" title={t("sftp.rename")} onclick={() => void showNameDialog("rename", "remote")} disabled={selectedRemoteEntries.length !== 1 || busy || listing}>
              <svg viewBox="0 0 24 24"><path d="m4 16-.8 4 4-.8L19.5 7a2.1 2.1 0 0 0-3-3z"/><path d="m14.5 6.5 3 3"/></svg><span>{t("sftp.rename")}</span>
            </button>
            <button class="tool-button text-tool danger-tool" title={t("sftp.delete")} onclick={() => requestDelete("remote")} disabled={!selectedRemoteEntries.length || busy || listing}>
              <svg viewBox="0 0 24 24"><path d="M4 7h16m-10 4v6m4-6v6M6 7l1 13h10l1-13M9 7V4h6v3"/></svg><span>{t("sftp.delete")}{selectedRemoteEntries.length ? " (" + selectedRemoteEntries.length + ")" : ""}</span>
            </button>
            <button class="tool-button text-tool" title={t("sftp.download")} onclick={() => void startTransfer("download")} disabled={!activeSession || !selectedRemoteEntries.length || !localDirectory || busy || localListing}>
              <svg viewBox="0 0 24 24"><path d="M12 3v12m-4-4 4 4 4-4M4 17v4h16v-4"/></svg><span>{t("sftp.download")}{selectedRemoteEntries.length ? " (" + selectedRemoteEntries.length + ")" : ""}</span>
            </button>
          </div>
          <form class="pane-path-form" onsubmit={(event) => { event.preventDefault(); void navigateFromInput(); }}>
            <input bind:this={pathInput} bind:value={pathValue} aria-label={t("sftp.path")} placeholder={t("sftp.pathPlaceholder")} disabled={!activeSession || busy} spellcheck="false" />
            <button type="submit" title={t("sftp.go")} aria-label={t("sftp.go")} disabled={!activeSession || busy}>{t("sftp.go")}</button>
          </form>
          <div class="sftp-list-head" role="row">
            <span>{t("sftp.name")}</span>
            <span>{t("sftp.size")}</span>
            <span>{t("sftp.modified")}</span>
            <span>{t("sftp.permissions")}</span>
          </div>
          <div class="sftp-list" bind:this={rowList} role="list" aria-label={t("sftp.remoteEntries")} onscroll={updateRemoteScroll} ondragover={(event) => event.preventDefault()} ondrop={onRemoteDrop}>
            {#if !activeSession}
              <div class="sftp-empty">
                <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 7h7l2 2h9v10H3z"/><path d="M3 7V5h7l2 2"/></svg>
                <p>{t("sftp.noSession")}</p>
                <button class="primary-button" onclick={openConnectionPicker}>{t("sftp.openConnection")}</button>
              </div>
            {:else if listing && !activeDirectory}
              <div class="sftp-empty"><span class="spinner"></span><p>{t("sftp.loading")}</p></div>
            {:else if !activeDirectory}
              <div class="sftp-empty-list">{t("sftp.emptyDirectory")}</div>
            {:else if listing}
              <div class="list-loading"><span class="spinner"></span>{t("sftp.loading")}</div>
            {:else if filteredEntries.length === 0}
              <div class="sftp-empty-list">{search ? t("sftp.noSearchResults") : t("sftp.emptyDirectory")}</div>
            {:else}
              {#if remoteVisibleWindow.top > 0}
                <div class="sftp-virtual-spacer" style:height={remoteVisibleWindow.top + "px"} aria-hidden="true"></div>
              {/if}
              {#each remoteVisibleWindow.entries as entry, index (entry.path)}
                <div class="sftp-entry" class:selected={(selectedRemotePaths[activeId] ?? []).includes(entry.path)} class:focused={selectedPaths[activeId] === entry.path}>
                  <div class="entry-name-cell">
                    <input class="entry-select" type="checkbox" checked={(selectedRemotePaths[activeId] ?? []).includes(entry.path)} aria-label={t("sftp.selectEntry", { name: entry.name })} onchange={(event) => toggleRemoteSelection(entry.path, event.currentTarget.checked)} />
                    <button class="entry-name" data-entry-index={remoteVisibleWindow.start + index} class:directory-entry={entry.kind === "directory"} class:symlink-entry={entry.kind === "symlink"} tabindex={selectedPaths[activeId] === entry.path || (!selectedPaths[activeId] && remoteVisibleWindow.start + index === 0) ? 0 : -1} draggable={entry.kind === "file" || entry.kind === "directory"} aria-label={entry.name + (entry.kind === "directory" ? ", " + t("sftp.directory") : "")} onclick={(event) => selectRemoteEntry(entry, event)} ondblclick={() => openEntry(entry)} onfocus={() => chooseEntry(entry)} onkeydown={(event) => onRowKeydown(event, remoteVisibleWindow.start + index, entry)} ondragstart={(event) => { event.dataTransfer?.setData("application/x-sshterm-remote-path", entry.path); if (event.dataTransfer) event.dataTransfer.effectAllowed = "copy"; }}>
                      <span class="entry-icon" aria-hidden="true">
                        {#if entry.kind === "directory"}
                          <svg viewBox="0 0 24 24"><path d="M3 6h7l2 2h9v12H3z"/><path d="M3 6V4h7l2 2"/></svg>
                        {:else if entry.kind === "symlink"}
                          <svg viewBox="0 0 24 24"><path d="M10 13a4 4 0 0 0 6 0l3-3a4 4 0 0 0-6-6l-1.5 1.5"/><path d="M14 11a4 4 0 0 0-6 0l-3 3a4 4 0 0 0 6 6l1.5-1.5"/></svg>
                        {:else}
                          <svg viewBox="0 0 24 24"><path d="M6 3h8l4 4v14H6z"/><path d="M14 3v5h4M9 13h6m-6 4h6"/></svg>
                        {/if}
                      </span>
                      <span class="entry-label" title={entry.name}>{entry.name}</span>
                    </button>
                  </div>
                  <span class="entry-meta size-meta">{entry.kind === "directory" ? "—" : formatSize(entry.size)}</span>
                  <span class="entry-meta modified-meta" title={formatModified(entry.modified)}>{formatModified(entry.modified)}</span>
                  <span class="entry-meta permission-meta">{formatPermissions(entry.permissions)}</span>
                </div>
              {/each}
              {#if remoteVisibleWindow.bottom > 0}
                <div class="sftp-virtual-spacer" style:height={remoteVisibleWindow.bottom + "px"} aria-hidden="true"></div>
              {/if}
            {/if}
          </div>
          <footer class="pane-footer">
            <div class="sftp-filter">
              <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="10.8" cy="10.8" r="6.8"/><path d="m16 16 5 5"/></svg>
              <input bind:this={searchInput} bind:value={search} oninput={resetRemoteScroll} onkeydown={onSearchKeydown} placeholder={t("sftp.search")} aria-label={t("sftp.search")} disabled={!activeSession} />
              {#if search}<button title={t("sftp.clearSearch")} aria-label={t("sftp.clearSearch")} onclick={() => { search = ""; searchInput?.focus(); }}>×</button>{/if}
            </div>
            <span class="footer-status" aria-live="polite">{#if statusMessage}{statusMessage}{:else if activeDirectory}{t("sftp.itemCount", { count: activeDirectory.entries.length })}{:else}{t("sftp.ready")}{/if}</span>
          </footer>
        </section>
      </div>
    </div>

    {#if activeTransferJobs.length > 0}
      <div
        class="transfer-queue-resize"
        class:dragging={resizingTransferQueue}
        role="separator"
        tabindex="0"
        aria-orientation="horizontal"
        aria-label={t("sftp.resizeTransferQueue")}
        aria-valuemin={MIN_TRANSFER_QUEUE_HEIGHT}
        aria-valuemax={maxTransferQueueHeight()}
        aria-valuenow={transferQueueHeight}
        title={t("sftp.resizeTransferQueueHint")}
        onpointerdown={startTransferQueueResize}
        onpointermove={moveTransferQueueResize}
        onpointerup={endTransferQueueResize}
        onpointercancel={endTransferQueueResize}
        onkeydown={onTransferQueueResizeKeydown}
      ><span aria-hidden="true"></span></div>
      <section class="transfer-queue" style:height={transferQueueHeight + "px"} aria-label={t("sftp.transferQueue")} aria-live="polite">
        <header class="transfer-queue-header">{t("sftp.transferQueue")}</header>
        <div class="transfer-queue-list">
          {#each activeTransferJobs as job (job.transfer_id)}
            <div class="transfer-row">
              <div class="transfer-details">
                <strong>{job.direction === "upload" ? t("sftp.upload") : t("sftp.download")}</strong>
                <span title={job.current_file || job.transfer_id}>{job.current_file || t("sftp.transferPreparing")}</span>
                <small>{job.files_completed}/{job.files_total} · {formatSize(job.bytes_transferred)} / {formatSize(job.bytes_total)}</small>
                {#if job.error}<small class="transfer-error">{job.error}</small>{/if}
              </div>
              <progress max="100" value={transferPercent(job)} aria-label={t("sftp.transferProgress")}></progress>
              <span class="transfer-status">{job.status === "running" ? t("sftp.transferStatus.running") : job.status === "completed" ? t("sftp.transferStatus.completed") : job.status === "cancelled" ? t("sftp.transferStatus.cancelled") : t("sftp.transferStatus.failed")}</span>
              {#if job.status === "running"}
                <button class="tool-button danger-tool" title={t("sftp.cancelTransfer")} aria-label={t("sftp.cancelTransfer")} onclick={() => void cancelTransfer(job.transfer_id)}>×</button>
              {/if}
            </div>
          {/each}
        </div>
      </section>
    {/if}

    {#if pickerOpen}
      <div class="sftp-nested-backdrop">
        <section class="sftp-small-dialog" role="dialog" aria-modal="true" aria-labelledby="sftp-picker-title">
          <header class="small-dialog-head">
            <h3 id="sftp-picker-title">{t("sftp.openConnection")}</h3>
            <button class="icon-button" title={t("workspace.close")} aria-label={t("workspace.close")} onclick={() => (pickerOpen = false)}>×</button>
          </header>
          <input bind:this={pickerInput} bind:value={pickerSearch} class="picker-search" placeholder={t("sftp.connectionSearch")} aria-label={t("sftp.connectionSearch")} onkeydown={(event) => {
            if (event.key === "Enter" && filteredConnections[0]) { event.preventDefault(); pickerOpen = false; void openSession(filteredConnections[0].id); }
            if (event.key === "Escape") { event.preventDefault(); if (pickerSearch) pickerSearch = ""; else pickerOpen = false; }
          }} />
          <div class="connection-results">
            {#if connections.length === 0}
              <p class="empty-message">{t("workspace.pickerEmpty")}</p>
            {:else if filteredConnections.length === 0}
              <p class="empty-message">{t("tree.noMatches")}</p>
            {:else}
              {#each filteredConnections as connection (connection.id)}
                <button class="connection-option" onclick={() => { pickerOpen = false; void openSession(connection.id); }}>
                  <span class="connection-name">{connection.name}</span>
                  <span class="connection-detail">{connection.user}@{connection.host}:{connection.port}</span>
                </button>
              {/each}
            {/if}
          </div>
          <div class="small-dialog-actions">
            <button class="secondary-button" onclick={() => (pickerOpen = false)}>{t("editor.cancel")}</button>
          </div>
        </section>
      </div>
    {/if}

    {#if nameMode}
      <div class="sftp-nested-backdrop">
        <form class="sftp-small-dialog" role="dialog" aria-modal="true" aria-labelledby="sftp-name-title" onsubmit={(event) => { event.preventDefault(); void saveName(); }}>
          <header class="small-dialog-head">
            <h3 id="sftp-name-title">{namePane === "local" ? (nameMode === "mkdir" ? t("sftp.localNewFolderTitle") : t("sftp.localRenameTitle")) : (nameMode === "mkdir" ? t("sftp.newFolderTitle") : t("sftp.renameTitle"))}</h3>
          </header>
          <input bind:this={nameInput} bind:value={nameValue} class="picker-search" placeholder={t("sftp.namePlaceholder")} aria-label={t("sftp.namePlaceholder")} onkeydown={(event) => { if (event.key === "Escape") { event.preventDefault(); nameMode = null; } }} />
          <div class="small-dialog-actions">
            <button type="button" class="secondary-button" onclick={() => (nameMode = null)}>{t("editor.cancel")}</button>
            <button type="submit" class="primary-button" disabled={busy || !nameValue.trim()}>{t("editor.save")}</button>
          </div>
        </form>
      </div>
    {/if}

    {#if overwriteConfirm}
      <div class="sftp-nested-backdrop">
        <section class="sftp-small-dialog" role="alertdialog" aria-modal="true" aria-labelledby="sftp-overwrite-title">
          <header class="small-dialog-head"><h3 id="sftp-overwrite-title">{t("sftp.overwriteTitle")}</h3></header>
          <p class="confirm-copy">{t("sftp.overwriteConfirm", { count: overwriteConfirm.conflicts.length })}</p>
          <div class="overwrite-list">
            {#each overwriteConfirm.conflicts.slice(0, 8) as path (path)}<span title={path}>{path}</span>{/each}
            {#if overwriteConfirm.conflicts.length > 8}<small>{t("sftp.overwriteMore", { count: overwriteConfirm.conflicts.length - 8 })}</small>{/if}
          </div>
          <div class="small-dialog-actions">
            <button class="secondary-button" onclick={() => (overwriteConfirm = null)}>{t("editor.cancel")}</button>
            <button bind:this={overwriteConfirmButton} class="primary-button" onclick={() => void confirmOverwrite()}>{t("sftp.overwrite")}</button>
          </div>
        </section>
      </div>
    {/if}
    {#if deleteTarget}
      <div class="sftp-nested-backdrop">
        <section class="sftp-small-dialog" role="alertdialog" aria-modal="true" aria-labelledby="sftp-delete-title">
          <header class="small-dialog-head"><h3 id="sftp-delete-title">{deleteTarget.pane === "local" ? t("sftp.localDeleteTitle") : t("sftp.deleteTitle")}</h3></header>
          {#if deleteTarget.entries.length === 1}
            <p class="confirm-copy">{t(deleteTarget.pane === "local" ? "sftp.localDeleteConfirm" : "sftp.deleteConfirm", { name: deleteTarget.entries[0]?.name ?? "" })}</p>
          {:else}
            <p class="confirm-copy">{t("sftp.deleteConfirmMultiple", { count: deleteTarget.entries.length, location: t(deleteTarget.pane === "local" ? "sftp.localPane" : "sftp.remotePane") })}</p>
          {/if}
          <div class="overwrite-list">
            {#each deleteTarget.entries.slice(0, 8) as entry (entry.path)}<span title={entry.path}>{entry.name}</span>{/each}
            {#if deleteTarget.entries.length > 8}<small>{t("sftp.overwriteMore", { count: deleteTarget.entries.length - 8 })}</small>{/if}
          </div>
          {#if deleteTarget.entries.some((entry) => entry.kind === "directory")}
            <p class="confirm-hint">{t("sftp.emptyDirectoryOnly")}</p>
          {/if}
          <div class="small-dialog-actions">
            <button bind:this={deleteCancelButton} class="secondary-button" onclick={() => void dismissDelete()}>{t("editor.cancel")}</button>
            <button class="danger-button" onclick={() => void confirmDelete()} disabled={busy}>{t("sftp.delete")}{deleteTarget.entries.length > 1 ? " (" + deleteTarget.entries.length + ")" : ""}</button>
          </div>
        </section>
      </div>
    {/if}
  </section>
</div>

<style>
  .sftp-backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 20px;
    background: color-mix(in srgb, var(--c-app-bg) 72%, transparent);
    backdrop-filter: blur(3px);
  }
  .sftp-backdrop.embedded { position: absolute; inset: 0; z-index: 1; padding: 0; background: transparent; backdrop-filter: none; }
  .sftp-backdrop.hidden { display: none; }
  .sftp-backdrop.embedded .sftp-browser { width: 100%; height: 100%; min-height: 0; border: 0; border-radius: 0; box-shadow: none; }
  .sftp-browser {
    position: relative;
    display: flex;
    flex-direction: column;
    width: min(1160px, 96vw);
    height: min(780px, 92vh);
    min-height: 420px;
    min-width: 0;
    overflow: hidden;
    color: var(--text);
    background: var(--surface-raised);
    border: 1px solid var(--border-strong);
    border-radius: 8px;
    box-shadow: 0 18px 64px color-mix(in srgb, var(--c-app-bg) 45%, transparent);
  }
  .sftp-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 52px;
    padding: 0 16px;
    border-bottom: 1px solid var(--border);
  }
  .sftp-heading { display: flex; align-items: center; gap: 10px; min-width: 0; }
  .sftp-mark { width: 28px; height: 28px; display: grid; place-items: center; color: var(--accent); background: var(--surface-hover); border-radius: 6px; }
  .sftp-mark svg { width: 19px; height: 19px; fill: none; stroke: currentColor; stroke-width: 1.7; stroke-linecap: round; stroke-linejoin: round; }
  .sftp-title-block { min-width: 0; display: flex; flex-direction: column; line-height: 1.3; }
  .sftp-title-block h2 { margin: 0; font-size: var(--ui-font-title); font-weight: 600; }
  .sftp-subtitle { max-width: 70vw; overflow: hidden; color: var(--muted); font-size: var(--ui-font-meta); text-overflow: ellipsis; white-space: nowrap; }
  .icon-button, .tool-button, .dismiss-error {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 28px;
    height: 28px;
    padding: 0 6px;
    color: var(--text-secondary);
    background: transparent;
    border: 1px solid transparent;
    border-radius: 4px;
    cursor: pointer;
    font: inherit;
  }
  .icon-button svg, .tool-button svg { width: 17px; height: 17px; fill: none; stroke: currentColor; stroke-width: 1.7; stroke-linecap: round; stroke-linejoin: round; flex: 0 0 auto; }
  .icon-button:hover, .tool-button:hover { color: var(--accent); background: var(--surface-hover); border-color: var(--border); }
  .icon-button:disabled, .tool-button:disabled { opacity: .42; cursor: not-allowed; }
  .sftp-toolbar { display: flex; align-items: center; flex-wrap: wrap; gap: 4px; min-height: 40px; padding: 4px 10px; border-bottom: 1px solid var(--border); }
  .toolbar-group { display: flex; flex-wrap: wrap; gap: 3px; }
  .toolbar-spacer { flex: 1 1 auto; }
  .tool-button { gap: 6px; padding: 0 7px; font-size: var(--ui-font-control); white-space: nowrap; }
  .tool-button.text-tool { border-color: var(--border); background: var(--surface); }
  .tool-button.danger-tool { color: var(--danger); }
  .local-tool { color: var(--text-secondary); }
  .sftp-content { display: flex; flex: 1 1 auto; min-height: 0; min-width: 0; background: var(--app-bg); }
  .sftp-dual-panes { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); flex: 1 1 auto; min-height: 0; min-width: 0; }
  .file-pane { display: flex; flex-direction: column; min-width: 0; min-height: 0; overflow: hidden; background: var(--app-bg); border: 1px solid transparent; }
  .file-pane.active-pane { border-color: var(--accent); box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 55%, transparent); }
  .file-pane.active-pane .pane-heading { background: color-mix(in srgb, var(--accent) 10%, var(--surface)); }
  .file-pane.active-pane .pane-heading-title strong { color: var(--accent); }
  .local-pane { border-right-color: var(--border-strong); }
  .pane-heading { display: flex; align-items: center; justify-content: space-between; gap: 8px; min-height: 34px; padding: 3px 8px; background: var(--surface); border-bottom: 1px solid var(--border); }
  .pane-heading-title { display: flex; align-items: baseline; gap: 8px; min-width: 0; }
  .pane-heading-title strong { color: var(--text); font-size: var(--ui-font-control); font-weight: 650; }
  .pane-heading-title span { min-width: 0; overflow: hidden; color: var(--muted); font-size: var(--ui-font-meta); text-overflow: ellipsis; white-space: nowrap; }
  .pane-heading-actions { display: flex; align-items: center; gap: 3px; flex: 0 0 auto; }
  .pane-actions { display: flex; flex: 0 0 auto; align-items: center; flex-wrap: wrap; gap: 4px; min-height: 36px; padding: 3px 7px; background: var(--surface-raised); border-bottom: 1px solid var(--border); }
  .pane-actions .tool-button { min-width: 0; height: 27px; padding: 0 6px; font-size: var(--ui-font-meta); }
  .pane-actions .tool-button svg { width: 15px; height: 15px; }
  .local-root-select { max-width: 116px; height: 27px; padding: 0 5px; color: var(--text-secondary); background: var(--surface-deep); border: 1px solid var(--border); border-radius: 4px; font: inherit; font-size: var(--ui-font-meta); }
  .pane-path-form { display: flex; flex: 0 0 auto; gap: 4px; min-width: 0; padding: 5px 8px; border-bottom: 1px solid var(--border); background: var(--surface-raised); }
  .pane-path-form input, .picker-search, .sftp-filter input { min-width: 0; width: 100%; height: 28px; padding: 0 8px; color: var(--text); background: var(--surface-deep); border: 1px solid var(--border); border-radius: 4px; font: inherit; font-size: var(--ui-font-control); box-sizing: border-box; }
  .pane-path-form button, .primary-button { min-width: 44px; height: 28px; padding: 0 10px; color: var(--accent-contrast); background: var(--accent); border: 1px solid var(--accent); border-radius: 4px; cursor: pointer; font: inherit; font-size: var(--ui-font-control); font-weight: 600; }
  .pane-path-form button:disabled, .primary-button:disabled { opacity: .45; cursor: not-allowed; }
  .sftp-list-head, .sftp-entry, .local-list-head, .local-entry { display: grid; align-items: center; gap: 5px; padding: 0 7px; }
  .sftp-list-head, .sftp-entry { grid-template-columns: minmax(100px, 1fr) 58px 100px 48px; }
  .local-list-head, .local-entry { grid-template-columns: minmax(100px, 1fr) 62px 94px; }
  .sftp-list-head, .local-list-head { flex: 0 0 29px; min-height: 29px; color: var(--muted); background: var(--surface); border-bottom: 1px solid var(--border); font-size: var(--ui-font-meta); font-weight: 600; }
  .sftp-list, .local-list { display: flex; flex: 1 1 auto; flex-direction: column; min-width: 0; min-height: 0; overflow: auto; padding: 4px 0; }
  .sftp-entry, .local-entry { flex: 0 0 30px; min-height: 30px; color: var(--text); border: 1px solid transparent; border-radius: 4px; font-size: var(--ui-font-dense); }
  .sftp-entry { box-sizing: border-box; }
  .sftp-virtual-spacer { flex: 0 0 auto; min-height: 0; pointer-events: none; }
  .sftp-entry:hover, .local-entry:hover { background: var(--surface-hover); }
  .sftp-entry.selected, .local-entry.selected { background: var(--surface-hover); border-color: var(--accent); }
  .local-entry.focused:not(.selected), .sftp-entry.focused:not(.selected) { border-color: var(--border-strong); }
  .entry-name-cell { display: flex; align-items: center; min-width: 0; gap: 5px; }
  .entry-select { width: 13px; height: 13px; flex: 0 0 13px; margin: 0; accent-color: var(--accent); }
  .entry-name { display: flex; align-items: center; min-width: 0; gap: 6px; width: 100%; height: 28px; padding: 0 2px; overflow: hidden; text-align: left; color: var(--text); background: transparent; border: 0; border-radius: 3px; cursor: pointer; font: inherit; font-size: var(--ui-font-dense); }
  .entry-name:hover { color: var(--accent); }
  .entry-icon { display: inline-flex; flex: 0 0 17px; width: 17px; height: 17px; align-items: center; justify-content: center; color: var(--muted); }
  .directory-entry .entry-icon { color: var(--accent); }
  .symlink-entry .entry-icon { color: var(--warning); }
  .entry-icon svg { width: 16px; height: 16px; fill: none; stroke: currentColor; stroke-width: 1.5; stroke-linecap: round; stroke-linejoin: round; }
  .entry-label { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .entry-meta { overflow: hidden; color: var(--muted); text-overflow: ellipsis; white-space: nowrap; font-size: var(--ui-font-meta); }
  .permission-meta { font-family: ui-monospace, Consolas, monospace; }
  .pane-footer { display: flex; align-items: center; gap: 8px; flex: 0 0 auto; min-height: 38px; padding: 4px 8px; border-top: 1px solid var(--border); background: var(--surface); }
  .sftp-filter { display: flex; align-items: center; gap: 5px; width: min(220px, 58%); min-width: 100px; }
  .sftp-filter svg { width: 15px; height: 15px; flex: 0 0 15px; fill: none; stroke: var(--muted); stroke-width: 1.7; stroke-linecap: round; }
  .sftp-filter button { width: 22px; height: 22px; margin-left: -28px; color: var(--muted); background: transparent; border: 0; cursor: pointer; }
  .footer-status { min-width: 0; overflow: hidden; margin-left: auto; color: var(--text-secondary); font-size: var(--ui-font-meta); text-align: right; text-overflow: ellipsis; white-space: nowrap; }
  .local-list-error { color: var(--danger); overflow-wrap: anywhere; padding: 12px; }
  .transfer-queue-resize { display: flex; align-items: center; justify-content: center; flex: 0 0 9px; height: 9px; cursor: ns-resize; touch-action: none; background: var(--surface); border-top: 1px solid var(--border); border-bottom: 1px solid var(--border); }
  .transfer-queue-resize span { width: 44px; height: 3px; border-radius: 3px; background: var(--border-strong); transition: background 120ms ease, width 120ms ease; }
  .transfer-queue-resize:hover span, .transfer-queue-resize:focus-visible span, .transfer-queue-resize.dragging span { width: 64px; background: var(--accent); }
  .transfer-queue-resize:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  .transfer-queue { display: flex; flex: 0 0 auto; flex-direction: column; min-height: 96px; max-height: 55%; overflow: hidden; background: var(--surface); border-top: 1px solid var(--border); }
  .transfer-queue-header { flex: 0 0 28px; display: flex; align-items: center; padding: 0 10px; color: var(--text-secondary); background: var(--surface); border-bottom: 1px solid var(--border); font-size: var(--ui-font-meta); font-weight: 650; }
  .transfer-queue-list { flex: 1 1 auto; min-height: 0; overflow-y: auto; }
  .transfer-row { display: grid; grid-template-columns: minmax(160px, 1fr) minmax(80px, 180px) auto 28px; align-items: center; gap: 10px; min-height: 42px; padding: 5px 10px; border-bottom: 1px solid var(--border); }
  .transfer-details { display: flex; flex-direction: column; min-width: 0; gap: 1px; }
  .transfer-details strong { font-size: var(--ui-font-control); font-weight: 600; }
  .transfer-details span { overflow: hidden; color: var(--text-secondary); font-size: var(--ui-font-meta); text-overflow: ellipsis; white-space: nowrap; }
  .transfer-details small, .transfer-status { color: var(--muted); font-size: var(--ui-font-meta); }
  .transfer-details .transfer-error { color: var(--danger); }
  .transfer-row progress { width: 100%; height: 6px; accent-color: var(--accent); }
  .overwrite-list { display: flex; flex-direction: column; gap: 4px; max-height: 120px; overflow: auto; padding: 6px; background: var(--surface); border: 1px solid var(--border); border-radius: 4px; }
  .overwrite-list span { overflow: hidden; color: var(--text-secondary); font-size: var(--ui-font-meta); text-overflow: ellipsis; white-space: nowrap; }
  .overwrite-list small { color: var(--muted); font-size: var(--ui-font-meta); }
  .local-selection { display: flex; align-items: center; flex-wrap: wrap; gap: 7px; padding: 6px 10px; background: var(--surface); border-top: 1px solid var(--border); color: var(--text-secondary); font-size: var(--ui-font-meta); }
  .local-selection strong { color: var(--text); }
  .local-path-chip { display: inline-flex; align-items: center; gap: 4px; max-width: 320px; padding: 2px 4px 2px 6px; border: 1px solid var(--border); border-radius: 3px; }
  .local-path-chip > span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .local-path-chip button { width: 20px; height: 20px; flex: 0 0 20px; color: var(--muted); background: transparent; border: 0; border-radius: 3px; cursor: pointer; }
  .local-path-chip button:hover { color: var(--danger); background: var(--surface-hover); }
  .local-destination { max-width: min(420px, 100%); overflow: hidden; margin-left: auto; text-overflow: ellipsis; white-space: nowrap; }
  .sftp-empty, .sftp-empty-list { display: flex; align-items: center; justify-content: center; flex-direction: column; gap: 10px; flex: 1; min-height: 100px; color: var(--muted); text-align: center; font-size: var(--ui-font-body); }
  .sftp-empty svg { width: 36px; height: 36px; fill: none; stroke: var(--muted); stroke-width: 1.3; }
  .sftp-empty p { margin: 0; }
  .sftp-empty-list { min-height: 170px; }
  .list-loading { display: flex; align-items: center; justify-content: center; gap: 10px; padding: 22px; color: var(--muted); font-size: var(--ui-font-control); }
  .spinner { width: 16px; height: 16px; border: 2px solid var(--border-strong); border-top-color: var(--accent); border-radius: 50%; animation: sftp-spin .8s linear infinite; }
  @keyframes sftp-spin { to { transform: rotate(360deg); } }
  .sftp-nested-backdrop { position: absolute; z-index: 2; inset: 0; display: flex; align-items: center; justify-content: center; padding: 16px; background: color-mix(in srgb, var(--c-app-bg) 62%, transparent); }
  .sftp-small-dialog { display: flex; flex-direction: column; gap: 12px; width: min(420px, 100%); max-height: 80%; padding: 16px; overflow: hidden; color: var(--text); background: var(--surface-raised); border: 1px solid var(--border-strong); border-radius: 7px; box-shadow: 0 12px 42px color-mix(in srgb, var(--c-app-bg) 45%, transparent); }
  .small-dialog-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .small-dialog-head h3 { margin: 0; font-size: var(--ui-font-section); font-weight: 600; }
  .picker-search { flex: 0 0 auto; height: 32px; }
  .connection-results { min-height: 0; overflow-y: auto; display: flex; flex-direction: column; gap: 4px; }
  .connection-option { display: flex; flex-direction: column; gap: 3px; padding: 8px 9px; color: var(--text); text-align: left; background: var(--surface); border: 1px solid var(--border); border-radius: 4px; cursor: pointer; }
  .connection-option:hover { border-color: var(--accent); background: var(--surface-hover); }
  .connection-name { font-size: var(--ui-font-body); font-weight: 500; }
  .connection-detail, .empty-message { color: var(--muted); font-size: var(--ui-font-meta); }
  .empty-message { padding: 8px; }
  .small-dialog-actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 2px; }
  .secondary-button, .danger-button { min-height: 28px; padding: 0 10px; color: var(--text-secondary); background: var(--surface); border: 1px solid var(--border); border-radius: 4px; cursor: pointer; font: inherit; font-size: var(--ui-font-control); }
  .danger-button { color: var(--danger); }
  .confirm-copy, .confirm-hint { margin: 0; font-size: var(--ui-font-body); line-height: 1.5; }
  .confirm-hint { color: var(--warning); font-size: var(--ui-font-meta); }
  .sftp-browser button:focus-visible, .sftp-browser input:focus-visible, .sftp-browser [role="separator"]:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  @media (max-width: 920px) {
    .sftp-toolbar { align-items: flex-start; }
    .sftp-list-head, .sftp-entry { grid-template-columns: minmax(90px, 1fr) 48px 82px 42px; gap: 4px; padding-inline: 5px; }
    .local-list-head, .local-entry { grid-template-columns: minmax(90px, 1fr) 52px 78px; gap: 4px; padding-inline: 5px; }
    .pane-heading-title span { display: none; }
    .local-root-select { max-width: 92px; }
  }
  @media (max-width: 700px) {
    .sftp-dual-panes { grid-template-columns: minmax(0, 1fr); grid-template-rows: minmax(220px, 1fr) minmax(220px, 1fr); overflow-y: auto; }
    .local-pane { border-right: 0; border-bottom: 1px solid var(--border-strong); }
    .file-pane { min-height: 220px; }
    .pane-footer { min-height: 36px; }
    .sftp-filter { width: min(220px, 62%); }
  }
</style>
