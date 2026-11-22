<script lang="ts">
  import { onMount, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { t } from "$lib/i18n/index.svelte";
  import type { ConnectionSummary, LocalDirectoryListing, SftpDirectory, SftpEntry, SftpSessionInfo } from "$lib/types";

  let {
    connectionId = null,
    embedded = false,
    visible = true,
    onclose,
  }: {
    connectionId?: string | null;
    embedded?: boolean;
    visible?: boolean;
    onclose: () => void;
  } = $props();

  let connections = $state<ConnectionSummary[]>([]);
  let sessions = $state<SftpSessionInfo[]>([]);
  let activeId = $state("");
  let directories = $state<Record<string, SftpDirectory>>({});
  let selectedPaths = $state<Record<string, string>>({});
  let search = $state("");
  let pathValue = $state("");
  let localDirectory = $state<LocalDirectoryListing | null>(null);
  let localPathValue = $state("");
  let localSearch = $state("");
  let localSelectedPaths = $state<string[]>([]);
  let localFocusedPath = $state("");
  let localListing = $state(false);
  let localErrorMessage = $state("");
  let busy = $state(false);
  let listing = $state(false);
  let errorMessage = $state("");
  let statusMessage = $state("");
  let pickerOpen = $state(false);
  let pickerSearch = $state("");
  let nameMode = $state<"mkdir" | "rename" | null>(null);
  let nameValue = $state("");
  let deleteTarget = $state<SftpEntry | null>(null);
  let selectedRemotePaths = $state<Record<string, string[]>>({});
  type TransferProgress = {
    transfer_id: string; session_id: string; direction: "upload" | "download";
    status: "running" | "completed" | "cancelled" | "failed"; current_file: string;
    files_completed: number; files_total: number; bytes_transferred: number; bytes_total: number; error: string | null;
  };
  type TransferStartResult = { transfer_id: string | null; conflicts: string[] };
  let transferJobs = $state<TransferProgress[]>([]);
  let overwriteConfirm = $state<{ sessionId: string; direction: "upload" | "download"; sources: string[]; destination: string; conflicts: string[] } | null>(null);
  let overwriteConfirmButton: HTMLButtonElement | undefined = $state();
  let searchInput: HTMLInputElement | undefined = $state();
  let localSearchInput: HTMLInputElement | undefined = $state();
  let pickerInput: HTMLInputElement | undefined = $state();
  let pathInput: HTMLInputElement | undefined = $state();
  let localPathInput: HTMLInputElement | undefined = $state();
  let nameInput: HTMLInputElement | undefined = $state();
  let rowList: HTMLDivElement | undefined = $state();
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
  const localRoot = $derived(
    localDirectory?.roots.find((root) => localDirectory.current_path.toLocaleLowerCase().startsWith(root.toLocaleLowerCase())) ?? "",
  );
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
      search = "";
      pathValue = opened.current_path;
      selectedPaths = { ...selectedPaths, [opened.session_id]: "" };
      selectedRemotePaths = { ...selectedRemotePaths, [opened.session_id]: [] };
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

  async function listDirectory(sessionId: string, path: string) {
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
    await listDirectory(activeId, path.trim());
  }

  async function navigateFromInput() {
    if (!pathValue.trim()) return;
    await navigate(pathValue);
  }

  function openParent() {
    if (activeDirectory?.parent_path) void navigate(activeDirectory.parent_path);
  }

  async function closeSession(sessionId: string) {
    const closing = sessions.find((item) => item.session_id === sessionId);
    if (!closing) return;
    const remaining = sessions.filter((item) => item.session_id !== sessionId);
    sessions = remaining;
    const nextId = remaining.at(-1)?.session_id ?? "";
    if (activeId === sessionId) {
      activeId = nextId;
      search = "";
      pathValue = directories[nextId]?.current_path ?? "";
      selectedPaths = { ...selectedPaths, [nextId]: "" };
    }
    const nextDirectories = { ...directories };
    delete nextDirectories[sessionId];
    directories = nextDirectories;
    const nextSelected = { ...selectedPaths };
    delete nextSelected[sessionId];
    selectedPaths = nextSelected;
    const nextRemoteSelected = { ...selectedRemotePaths };
    delete nextRemoteSelected[sessionId];
    selectedRemotePaths = nextRemoteSelected;
    try {
      await invoke("sftp_close", { sessionId });
    } catch (error) {
      // Vault lock or app shutdown may already have closed the runtime session.
      console.debug("SFTP session was already closed", message(error));
    }
    if (activeId) {
      await tick();
      searchInput?.focus();
    } else {
      await openConnectionPicker();
    }
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

  function openEntry(entry: SftpEntry) {
    chooseEntry(entry);
    if (entry.kind === "directory") void navigate(entry.path);
  }

  async function refreshDirectory() {
    if (activeDirectory) await navigate(activeDirectory.current_path);
  }

  async function showNameDialog(mode: "mkdir" | "rename") {
    if (!activeSession || busy) return;
    if (mode === "rename" && !selectedEntry) return;
    nameMode = mode;
    nameValue = mode === "rename" ? selectedEntry?.name ?? "" : "";
    await tick();
    nameInput?.focus();
    nameInput?.select();
  }

  async function saveName() {
    if (!activeSession || !activeDirectory || !nameMode || busy) return;
    const value = nameValue.trim();
    if (!value) return;
    const currentPath = activeDirectory.current_path;
    busy = true;
    errorMessage = "";
    try {
      if (nameMode === "mkdir") {
        await invoke("sftp_mkdir", {
          sessionId: activeId,
          remotePath: activeDirectory.current_path,
          name: value,
        });
        statusMessage = t("sftp.folderCreated", { name: value });
      } else if (selectedEntry) {
        await invoke("sftp_rename", {
          sessionId: activeId,
          remotePath: selectedEntry.path,
          newName: value,
        });
        statusMessage = t("sftp.renamed", { name: value });
      }
      nameMode = null;
      selectedPaths = { ...selectedPaths, [activeId]: "" };
      await listDirectory(activeId, currentPath);
    } catch (error) {
      errorMessage = t("sftp.operationFailed", { detail: message(error) });
    } finally {
      busy = false;
    }
  }

  async function dismissDelete() {
    deleteTarget = null;
    await tick();
    searchInput?.focus();
  }

  async function confirmDelete() {
    const target = deleteTarget;
    if (!target || !activeSession || busy) return;
    const currentPath = activeDirectory?.current_path ?? ".";
    busy = true;
    errorMessage = "";
    try {
      await invoke("sftp_delete", {
        sessionId: activeId,
        remotePath: target.path,
      });
      deleteTarget = null;
      selectedPaths = { ...selectedPaths, [activeId]: "" };
      statusMessage = t("sftp.deleted", { name: target.name });
      await listDirectory(activeId, currentPath);
      await tick();
      searchInput?.focus();
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
  }

  function selectLocalEntry(entry: LocalDirectoryListing["entries"][number], event: MouseEvent) {
    localFocusedPath = entry.path;
    if (entry.kind === "symlink" || entry.kind === "other") return;
    if (event.ctrlKey || event.metaKey) {
      toggleLocalSelection(entry.path, !localSelectedPaths.includes(entry.path));
    } else {
      localSelectedPaths = [entry.path];
    }
  }

  function openLocalEntry(entry: LocalDirectoryListing["entries"][number]) {
    if (entry.kind === "directory") {
      localSearch = "";
      void loadLocalDirectory(entry.path);
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
    selectedRemotePaths = { ...selectedRemotePaths, [activeId]: checked ? [...selected, path] : selected.filter((item) => item !== path) };
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
      if (progress.direction === "download" && activeId === progress.session_id) selectedRemotePaths = { ...selectedRemotePaths, [activeId]: [] };
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
    const rows = rowList?.querySelectorAll<HTMLButtonElement>("button.entry-name");
    if (!rows?.length) return;
    const target = Math.max(0, Math.min(index, rows.length - 1));
    rows[target]?.focus();
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
      chooseEntry(entry);
      void showNameDialog("rename");
    } else if (event.key === "Delete") {
      event.preventDefault();
      chooseEntry(entry);
      deleteTarget = entry;
    } else if (event.key === "Backspace") {
      event.preventDefault();
      openParent();
    }
  }

  function onBrowserKeydown(event: KeyboardEvent) {
    if (!visible) return;
    if (event.key === "Tab") {
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
    const editingText = target instanceof HTMLInputElement
      || target instanceof HTMLTextAreaElement
      || (target instanceof HTMLElement && target.isContentEditable);
    if (editingText && event.key !== "Escape") {
      if (event.ctrlKey && !event.altKey && event.key.toLowerCase() === "l") {
        event.preventDefault();
        pathInput?.focus();
        pathInput?.select();
      }
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      void closeBrowser();
    } else if (event.key === "F2" && selectedEntry) {
      event.preventDefault();
      void showNameDialog("rename");
    } else if (event.key === "Delete" && selectedEntry) {
      event.preventDefault();
      deleteTarget = selectedEntry;
    } else if (event.key === "Backspace" && !(event.target instanceof HTMLInputElement)) {
      event.preventDefault();
      openParent();
    } else if (event.ctrlKey && !event.altKey && event.key.toLowerCase() === "l") {
      event.preventDefault();
      pathInput?.focus();
      pathInput?.select();
    }
  }

  $effect(() => {
    if (visible) void tick().then(() => searchInput?.focus());
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
      for (const session of sessions) void invoke("sftp_close", { sessionId: session.session_id }).catch(() => {});
    };
  });
</script>

<svelte:window onkeydown={onBrowserKeydown} />

<div class="sftp-backdrop" class:embedded class:hidden={!visible}>
  <section bind:this={browserDialog} class="sftp-browser" role="dialog" aria-modal="true" aria-labelledby="sftp-title">
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
      <div class="sftp-header-actions">
        <button class="icon-button" title={t("sftp.newSession")} aria-label={t("sftp.newSession")} onclick={openConnectionPicker} disabled={busy}>
          <svg viewBox="0 0 24 24"><path d="M12 5v14M5 12h14"/></svg>
        </button>
        <button class="icon-button close-button" title={t("workspace.close")} aria-label={t("workspace.close")} onclick={() => void closeBrowser()}>
          <svg viewBox="0 0 24 24"><path d="m6 6 12 12M18 6 6 18"/></svg>
        </button>
      </div>
    </header>

    {#if sessions.length > 0}
      <nav class="sftp-tabs" aria-label={t("sftp.sessions")}>
        {#each sessions as session (session.session_id)}
          <div class="sftp-tab" class:active={session.session_id === activeId}>
            <button class="sftp-tab-label" title={session.connection_name} onclick={() => { activeId = session.session_id; search = ""; pathValue = directories[session.session_id]?.current_path ?? session.current_path; selectedPaths = { ...selectedPaths, [session.session_id]: "" }; }}>
              <span class="tab-state" aria-hidden="true"></span>
              <span>{session.connection_name}</span>
            </button>
            <button class="tab-close" title={t("sftp.closeSession")} aria-label={t("sftp.closeSession") + " " + session.connection_name} onclick={() => void closeSession(session.session_id)}>×</button>
          </div>
        {/each}
        <button class="tab-add" title={t("sftp.newSession")} aria-label={t("sftp.newSession")} onclick={openConnectionPicker}>+</button>
      </nav>
    {/if}

    <div class="sftp-toolbar">
      <div class="toolbar-group">
        <button class="tool-button" title={t("sftp.parent")} aria-label={t("sftp.parent")} onclick={openParent} disabled={!activeDirectory?.parent_path || busy || listing}>
          <svg viewBox="0 0 24 24"><path d="M19 12H5m7-7-7 7 7 7"/></svg>
        </button>
        <button class="tool-button" title={t("sftp.refresh")} aria-label={t("sftp.refresh")} onclick={() => void refreshDirectory()} disabled={!activeSession || busy || listing}>
          <svg viewBox="0 0 24 24"><path d="M20 7v5h-5M4 17v-5h5"/><path d="M5.6 9a7 7 0 0 1 12-2L20 12M4 12l2.4 5a7 7 0 0 0 12-2"/></svg>
        </button>
        <button class="tool-button text-tool" title={t("sftp.newFolder")} onclick={() => void showNameDialog("mkdir")} disabled={!activeSession || busy}>
          <svg viewBox="0 0 24 24"><path d="M3 6h7l2 2h9v11H3z"/><path d="M12 11v5m-2.5-2.5h5"/></svg>
          <span>{t("sftp.newFolder")}</span>
        </button>
        <button class="tool-button text-tool" title={t("sftp.rename")} onclick={() => void showNameDialog("rename")} disabled={!selectedEntry || busy}>
          <svg viewBox="0 0 24 24"><path d="m4 16-.8 4 4-.8L19.5 7a2.1 2.1 0 0 0-3-3z"/><path d="m14.5 6.5 3 3"/></svg>
          <span>{t("sftp.rename")}</span>
        </button>
        <button class="tool-button text-tool danger-tool" title={t("sftp.delete")} onclick={() => { if (selectedEntry) deleteTarget = selectedEntry; }} disabled={!selectedEntry || busy}>
          <svg viewBox="0 0 24 24"><path d="M4 7h16m-10 4v6m4-6v6M6 7l1 13h10l1-13M9 7V4h6v3"/></svg>
          <span>{t("sftp.delete")}</span>
        </button>
        <button class="tool-button text-tool" title={t("sftp.download")} onclick={() => void startTransfer("download")} disabled={!activeSession || !selectedRemoteEntries.length || !localDestination || busy}>
          <svg viewBox="0 0 24 24"><path d="M12 3v12m-4-4 4 4 4-4M4 17v4h16v-4"/></svg>
          <span>{t("sftp.download") + (selectedRemoteEntries.length ? " (" + selectedRemoteEntries.length + ")" : "")}</span>
        </button>
        <button class="tool-button text-tool" title={t("sftp.upload")} onclick={() => void startTransfer("upload")} disabled={!activeSession || !localPaths.length || busy}>
          <svg viewBox="0 0 24 24"><path d="M12 15V3m-4 4 4-4 4 4M4 17v4h16v-4"/></svg>
          <span>{t("sftp.upload") + (localPaths.length ? " (" + localPaths.length + ")" : "")}</span>
        </button>
      </div>
      <div class="toolbar-spacer"></div>
      <button class="tool-button text-tool local-tool" title={t("sftp.chooseLocalFiles")} onclick={() => void chooseLocalFiles()}>
        <svg viewBox="0 0 24 24"><path d="M4 5h6l2 2h8v12H4z"/><path d="M12 10v6m-3-3 3-3 3 3"/></svg>
        <span>{t("sftp.chooseLocalFiles")}</span>
      </button>
      <button class="tool-button text-tool local-tool" title={t("sftp.chooseLocalFolder")} onclick={() => void chooseLocalFolder()}>
        <svg viewBox="0 0 24 24"><path d="M3 6h7l2 2h9v11H3z"/><path d="M12 11v5m-2.5-2.5h5"/></svg>
        <span>{t("sftp.chooseLocalFolder")}</span>
      </button>
      <button class="tool-button text-tool local-tool" title={t("sftp.chooseLocalDestination")} onclick={() => void chooseLocalDestination()}>
        <svg viewBox="0 0 24 24"><path d="M12 3v12m-4-4 4 4 4-4M4 17v4h16v-4"/></svg>
        <span>{t("sftp.chooseLocalDestination")}</span>
      </button>
    </div>

    <div class="sftp-location">
      <div class="breadcrumbs" aria-label={t("sftp.path")}>
        <button class="crumb" onclick={() => void navigate("/") } disabled={!activeSession || busy}>/</button>
        {#each breadcrumbs as crumb (crumb.path)}
          <span class="crumb-separator" aria-hidden="true">/</span>
          <button class="crumb" title={crumb.path} onclick={() => void navigate(crumb.path)} disabled={!activeSession || busy}>{crumb.name}</button>
        {/each}
      </div>
      <form class="path-form" onsubmit={(event) => { event.preventDefault(); void navigateFromInput(); }}>
        <input bind:this={pathInput} bind:value={pathValue} aria-label={t("sftp.path")} placeholder={t("sftp.pathPlaceholder")} disabled={!activeSession || busy} spellcheck="false" />
        <button type="submit" title={t("sftp.go")} aria-label={t("sftp.go")} disabled={!activeSession || busy}>{t("sftp.go")}</button>
      </form>
    </div>

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
      {#if !activeSession}
        <div class="sftp-empty">
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 7h7l2 2h9v10H3z"/><path d="M3 7V5h7l2 2"/></svg>
          <p>{t("sftp.noSession")}</p>
          <button class="primary-button" onclick={openConnectionPicker}>{t("sftp.openConnection")}</button>
        </div>
      {:else if listing && !activeDirectory}
        <div class="sftp-empty"><span class="spinner"></span><p>{t("sftp.loading")}</p></div>
      {:else if activeDirectory}
        <div class="sftp-list-head" role="row">
          <span>{t("sftp.name")}</span>
          <span>{t("sftp.size")}</span>
          <span>{t("sftp.modified")}</span>
          <span>{t("sftp.permissions")}</span>
        </div>
        <div class="sftp-list" bind:this={rowList} role="list" aria-label={t("sftp.remoteEntries")}>
          {#if listing}
            <div class="list-loading"><span class="spinner"></span>{t("sftp.loading")}</div>
          {:else if filteredEntries.length === 0}
            <div class="sftp-empty-list">{search ? t("sftp.noSearchResults") : t("sftp.emptyDirectory")}</div>
          {:else}
            {#each filteredEntries as entry, index (entry.path)}
              <div class="sftp-entry" class:selected={selectedPaths[activeId] === entry.path || (selectedRemotePaths[activeId] ?? []).includes(entry.path)}>
                <div class="entry-name-cell">
                  <input class="entry-select" type="checkbox" checked={(selectedRemotePaths[activeId] ?? []).includes(entry.path)} aria-label={t("sftp.selectEntry", { name: entry.name })} onchange={(event) => toggleRemoteSelection(entry.path, event.currentTarget.checked)} />
                <button
                  class="entry-name"
                  class:directory-entry={entry.kind === "directory"}
                  class:symlink-entry={entry.kind === "symlink"}
                  tabindex={selectedPaths[activeId] === entry.path || (!selectedPaths[activeId] && index === 0) ? 0 : -1}
                  aria-label={entry.name + (entry.kind === "directory" ? ", " + t("sftp.directory") : "")}
                  onclick={() => chooseEntry(entry)}
                  ondblclick={() => openEntry(entry)}
                  onfocus={() => chooseEntry(entry)}
                  onkeydown={(event) => onRowKeydown(event, index, entry)}
                >
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
          {/if}
        </div>
      {/if}
    </div>

    <footer class="sftp-footer">
      <div class="sftp-filter">
        <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="10.8" cy="10.8" r="6.8"/><path d="m16 16 5 5"/></svg>
        <input bind:this={searchInput} bind:value={search} onkeydown={onSearchKeydown} placeholder={t("sftp.search")} aria-label={t("sftp.search")} disabled={!activeSession} />
        {#if search}
          <button title={t("sftp.clearSearch")} aria-label={t("sftp.clearSearch")} onclick={() => { search = ""; searchInput?.focus(); }}>×</button>
        {/if}
      </div>
      <span class="footer-status" aria-live="polite">
        {#if statusMessage}{statusMessage}{:else if activeDirectory}{t("sftp.itemCount", { count: activeDirectory.entries.length })}{:else}{t("sftp.ready")}{/if}
      </span>
      <span class="footer-shortcuts">Enter {t("sftp.openFolderHint")} · F2 {t("sftp.rename")} · Del {t("sftp.delete")}</span>
    </footer>

    {#if activeTransferJobs.length > 0}
      <section class="transfer-queue" aria-label={t("sftp.transferQueue")} aria-live="polite">
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
      </section>
    {/if}
    {#if localPaths.length || localDestination}
      <div class="local-selection">
        {#if localPaths.length}
          <strong>{t("sftp.localFilesSelected", { count: localPaths.length })}</strong>
          {#each localPaths as path (path)}
            <span class="local-path-chip" title={path}>
              <span>{path}</span>
              <button type="button" aria-label={t("sftp.removeLocalSelection")} title={t("sftp.removeLocalSelection")} onclick={() => removeLocalPath(path)}>×</button>
            </span>
          {/each}
        {/if}
        {#if localDestination}<span class="local-destination" title={localDestination}>{t("sftp.destination")}: {localDestination}</span>{/if}
      </div>
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
            <h3 id="sftp-name-title">{nameMode === "mkdir" ? t("sftp.newFolderTitle") : t("sftp.renameTitle")}</h3>
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
          <header class="small-dialog-head"><h3 id="sftp-delete-title">{t("sftp.deleteTitle")}</h3></header>
          <p class="confirm-copy">{t("sftp.deleteConfirm", { name: deleteTarget.name })}</p>
          {#if deleteTarget.kind === "directory"}
            <p class="confirm-hint">{t("sftp.emptyDirectoryOnly")}</p>
          {/if}
          <div class="small-dialog-actions">
            <button bind:this={deleteCancelButton} class="secondary-button" onclick={() => void dismissDelete()}>{t("editor.cancel")}</button>
            <button class="danger-button" onclick={() => void confirmDelete()} disabled={busy}>{t("sftp.delete")}</button>
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
  .sftp-header-actions { display: flex; gap: 4px; }
  .icon-button, .tool-button, .tab-close, .tab-add, .dismiss-error {
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
  .icon-button:hover, .tool-button:hover, .tab-close:hover, .tab-add:hover { color: var(--accent); background: var(--surface-hover); border-color: var(--border); }
  .icon-button:disabled, .tool-button:disabled { opacity: .42; cursor: not-allowed; }
  .close-button:hover { color: var(--danger); }
  .sftp-tabs { display: flex; align-items: stretch; gap: 3px; min-height: 34px; padding: 3px 8px 0; background: var(--surface); border-bottom: 1px solid var(--border); overflow-x: auto; }
  .sftp-tab { display: flex; align-items: center; max-width: 220px; min-width: 100px; border: 1px solid var(--border); border-bottom: 0; border-radius: 5px 5px 0 0; background: var(--surface-hover); }
  .sftp-tab.active { background: var(--surface-raised); border-top: 2px solid var(--accent); }
  .sftp-tab-label { display: flex; align-items: center; gap: 7px; min-width: 0; flex: 1; padding: 0 7px; color: var(--text-secondary); background: transparent; border: 0; cursor: pointer; font: inherit; font-size: var(--ui-font-dense); }
  .sftp-tab.active .sftp-tab-label { color: var(--text); }
  .sftp-tab-label span:last-child { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tab-state { width: 6px; height: 6px; flex: 0 0 6px; border-radius: 50%; background: var(--success); }
  .tab-close { flex: 0 0 24px; width: 24px; min-width: 24px; height: 24px; margin-right: 2px; }
  .tab-add { align-self: center; width: 28px; margin-left: 2px; color: var(--accent); font-size: 18px; }
  .sftp-toolbar { display: flex; align-items: center; flex-wrap: wrap; gap: 4px; min-height: 40px; padding: 4px 10px; border-bottom: 1px solid var(--border); }
  .toolbar-group { display: flex; flex-wrap: wrap; gap: 3px; }
  .toolbar-spacer { flex: 1 1 auto; }
  .tool-button { gap: 6px; padding: 0 7px; font-size: var(--ui-font-control); white-space: nowrap; }
  .tool-button.text-tool { border-color: var(--border); background: var(--surface); }
  .tool-button.danger-tool { color: var(--danger); }
  .local-tool { color: var(--text-secondary); }
  .sftp-location { display: flex; align-items: center; gap: 8px; min-height: 40px; padding: 5px 10px; border-bottom: 1px solid var(--border); }
  .breadcrumbs { display: flex; align-items: center; gap: 3px; min-width: 0; flex: 1 1 auto; overflow: hidden; white-space: nowrap; }
  .crumb { min-width: 22px; max-width: 180px; overflow: hidden; padding: 3px 5px; color: var(--text-secondary); background: transparent; border: 1px solid transparent; border-radius: 4px; text-overflow: ellipsis; cursor: pointer; font: inherit; font-size: var(--ui-font-dense); }
  .crumb:hover { color: var(--accent); background: var(--surface-hover); border-color: var(--border); }
  .crumb:disabled { opacity: .4; cursor: not-allowed; }
  .crumb-separator { color: var(--muted); font-size: var(--ui-font-meta); }
  .path-form { display: flex; flex: 0 1 42%; min-width: 170px; gap: 4px; }
  .path-form input, .picker-search, .sftp-filter input {
    min-width: 0;
    width: 100%;
    height: 28px;
    padding: 0 8px;
    color: var(--text);
    background: var(--surface-deep);
    border: 1px solid var(--border);
    border-radius: 4px;
    font: inherit;
    font-size: var(--ui-font-control);
    box-sizing: border-box;
  }
  .path-form button, .primary-button {
    min-width: 44px;
    height: 28px;
    padding: 0 10px;
    color: var(--accent-contrast);
    background: var(--accent);
    border: 1px solid var(--accent);
    border-radius: 4px;
    cursor: pointer;
    font: inherit;
    font-size: var(--ui-font-control);
    font-weight: 600;
  }
  .path-form button:disabled, .primary-button:disabled { opacity: .45; cursor: not-allowed; }
  .sftp-warning { padding: 6px 12px; color: var(--warning); background: var(--warning-bg); border-bottom: 1px solid var(--warning-border); font-size: var(--ui-font-meta); }
  .sftp-error { display: flex; align-items: center; gap: 8px; padding: 6px 12px; color: var(--danger); background: var(--danger-bg); border-bottom: 1px solid var(--border); font-size: var(--ui-font-control); }
  .sftp-error span { flex: 1; min-width: 0; overflow-wrap: anywhere; }
  .dismiss-error { width: 24px; min-width: 24px; height: 24px; color: inherit; font-size: 18px; }
  .sftp-content { display: flex; flex: 1 1 auto; min-height: 0; flex-direction: column; background: var(--app-bg); }
  .sftp-list-head, .sftp-entry { display: grid; grid-template-columns: minmax(220px, 1fr) 110px 190px 76px; align-items: center; gap: 8px; padding: 0 12px; }
  .entry-name-cell { display: flex; align-items: center; min-width: 0; gap: 7px; }
  .entry-select { width: 14px; height: 14px; flex: 0 0 14px; accent-color: var(--accent); }
  .sftp-list-head { min-height: 30px; color: var(--muted); background: var(--surface); border-bottom: 1px solid var(--border); font-size: var(--ui-font-meta); font-weight: 600; }
  .sftp-list { flex: 1; min-height: 0; overflow-y: auto; padding: 4px 0; }
  .sftp-entry { min-height: 30px; color: var(--text); border: 1px solid transparent; border-radius: 4px; font-size: var(--ui-font-dense); }
  .sftp-entry:hover { background: var(--surface-hover); }
  .sftp-entry.selected { background: var(--surface-hover); border-color: var(--accent); }
  .entry-name { display: flex; align-items: center; min-width: 0; gap: 8px; width: 100%; height: 28px; padding: 0 4px; overflow: hidden; text-align: left; color: var(--text); background: transparent; border: 0; border-radius: 3px; cursor: pointer; font: inherit; font-size: var(--ui-font-dense); }
  .entry-name:hover { color: var(--accent); }
  .entry-icon { display: inline-flex; flex: 0 0 18px; width: 18px; height: 18px; align-items: center; justify-content: center; color: var(--muted); }
  .directory-entry .entry-icon { color: var(--accent); }
  .symlink-entry .entry-icon { color: var(--warning); }
  .entry-icon svg { width: 17px; height: 17px; fill: none; stroke: currentColor; stroke-width: 1.5; stroke-linecap: round; stroke-linejoin: round; }
  .entry-label { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .entry-meta { overflow: hidden; color: var(--muted); text-overflow: ellipsis; white-space: nowrap; font-size: var(--ui-font-meta); }
  .permission-meta { font-family: ui-monospace, Consolas, monospace; }
  .sftp-footer { display: flex; align-items: center; flex-wrap: wrap; gap: 12px; min-height: 40px; padding: 5px 10px; border-top: 1px solid var(--border); background: var(--surface); }
  .sftp-filter { display: flex; align-items: center; gap: 6px; width: min(300px, 36%); min-width: 170px; }
  .sftp-filter svg { width: 16px; height: 16px; flex: 0 0 16px; fill: none; stroke: var(--muted); stroke-width: 1.7; stroke-linecap: round; }
  .sftp-filter button { width: 24px; height: 24px; margin-left: -30px; color: var(--muted); background: transparent; border: 0; cursor: pointer; }
  .footer-status { color: var(--text-secondary); font-size: var(--ui-font-meta); }
  .footer-shortcuts { margin-left: auto; color: var(--muted); font-size: var(--ui-font-meta); }
  .transfer-queue { display: flex; flex-direction: column; max-height: 160px; overflow: auto; background: var(--surface); border-top: 1px solid var(--border); }
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
  .sftp-browser button:focus-visible, .sftp-browser input:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  @media (max-width: 860px) {
    .sftp-toolbar { align-items: flex-start; }
    .local-tool span { display: none; }
    .sftp-list-head, .sftp-entry { grid-template-columns: minmax(140px, 1fr) 80px 130px 60px; gap: 5px; padding-inline: 8px; }
    .footer-shortcuts { display: none; }
    .sftp-location { flex-wrap: wrap; }
    .path-form { flex: 1 1 100%; }
  }
</style>
