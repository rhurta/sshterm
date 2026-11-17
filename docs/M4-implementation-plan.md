> **Historical proposal:** this document records the original M4 design discussion. M4 was subsequently implemented and is complete; `docs/` is tracked by Git (it is not ignored). Use `ROADMAP.md` and the current *SSH Terminal Manager – Tauri Plan & Specs* for the present implementation state.

---

# M4 — Session Tree: Proposed Implementation Plan

**Status:** Historical proposal; M4 completed
**Scope source:** `SSH Terminal Manager – Tauri Plan & Specs.md` §5, §7, §11; `ROADMAP.md` M4 section
**Milestone goal:** Folders, CRUD, search, import from PuTTY / WinSSHTerm / `~/.ssh/config`, and multi-tab workspace.

---

## 1. Scope

### In scope (per spec + roadmap)

| Item | Spec reference |
|---|---|
| Typed vault tree (replace `serde_json::Value`) | §5 "will be realised in M4" |
| Folders: create, rename, delete | ROADMAP checklist |
| Connections: upsert, delete, move between folders | ROADMAP `conn_upsert` |
| Search (name / host / user) | ROADMAP `search` |
| Import: `~/.ssh/config`, PuTTY registry, WinSSHTerm | §12 Decisions |
| Multi-tab workspace | ROADMAP "ships together" |
| Connection editor: auth / colors / keepalive / protected | ROADMAP checklist |
| Per-connection colors on terminal + tab accent | Deferred to M9; global application themes are implemented |
| Drag-and-drop in tree | §7 Sidebar with tree |

### Out of scope (later milestones)

- Jump chains (M5)
- Tunnels (M7)
- SFTP (M8)
- Broadcast / tiling (M6)
- Syntax highlighting (M6b)
- Native `russh` engine (M10)

For `auth.type = key` and `auth.type = agent`, M4 only stores and edits the field. Behavioural use of key files is already handled by `ssh.exe` via `-i` (see §6 of the spec); the agent path is a stub.

---

## 2. Decisions to confirm before coding

These are the open points from the review. Each is flagged with a recommendation.

| # | Question | Recommendation | Impact if deferred |
|---|---|---|---|
| D1 | **WinSSHTerm import format** — sample file not yet provided | Defer Step 3c to a follow-up commit within M4 | Import dialog ships with two of three sources; the third appears when the format is known |
| D2 | **Old vault migration** — M1–M3 vaults have a flat connection list, no tree | Auto-migrate on unlock: wrap existing connections in a single root folder "Connections", show a one-line notice | A hard error would break every existing vault; not recommended |
| D3 | **Folder delete with children** | Recursive by default, with a confirm dialog showing the affected count | Reject-if-non-empty is safer but forces manual cleanup; not recommended |
| D4 | **Drag-and-drop timing** | Ship basic DnD in M4 if time permits; otherwise move to M4b so Steps 1–3, 4 (without DnD), 5–8 land first | DnD is the riskiest UI piece; blocking M4 on it delays everything else |
| D5 | **Node ID scheme** | UUID v4 (`uuid` crate), with `f_` / `c_` prefixes for display only | Counter-based IDs collide on import/merge |
| D6 | **Import conflict policy** | Default skip on duplicate `(host, port, user)`; checkbox "Import as copy" renames | Silent overwrite is destructive; always show preview first |

All recommendations above are what the rest of this document assumes. Any change to D1–D6 affects the steps noted in §3.

---

## 3. Proposed implementation steps

Each step is independently reviewable and leaves the build green. Order is chosen so the riskiest, most-uncertain work (import format, DnD) is isolated.

### Step 1 — Vault data model migration (backend only)

**Files:** `src-tauri/src/store/model.rs` (new), `src-tauri/src/vault/model.rs`, `src-tauri/src/vault/tests.rs`

**Work:**
- Define typed structures:

  ```rust
  #[derive(Serialize, Deserialize)]
  #[serde(tag = "type")]
  enum TreeNode {
      Folder { id: String, name: String, children: Vec<TreeNode> },
      Connection { id: String },
  }

  struct Connection {
      id: String,
      name: String,
      host: String,
      port: u16,
      user: String,
      auth: Auth,
      jump: Vec<String>,              // serde(default) — M5
      keepalive_secs: u16,
      colors: Option<Colors>,         // fg/bg
      protected: bool,
      tunnels: Vec<Tunnel>,           // serde(default) — M7
      sftp: Option<Sftp>,             // serde(default) — M8
      highlight: Option<Vec<String>>, // serde(default) — M6b
  }

  #[derive(Serialize, Deserialize)]
  #[serde(tag = "type")]
  enum Auth {
      Password { password: String },
      Key { path: String, passphrase: Option<String> },
      Agent,
  }
  ```

- Vault payload root becomes:

  ```rust
  struct VaultPayload {
      version: u32,
      tree: Vec<TreeNode>,
      connections: HashMap<String, Connection>,
      known_hosts: HashMap<String, String>,
  }
  ```

- Migration function `migrate_v1_to_v2(payload) -> VaultPayload`: if `tree` is absent/empty and `connections` contains a flat list, wrap all in a single folder named after the i18n key `tree.default_folder` ("Connections" / "Conexiones").
- Unit tests: round-trip, migration from a synthetic M3-era payload, forward-compat (`serde(default)` on optional fields).

**Review points:**
- Is the `TreeNode` enum (internally tagged) the right shape, or should folders and connections share a flat `parent_id` model? Enum matches the spec's JSON example; chosen here.
- Confirm `jump`, `tunnels`, `sftp`, `highlight` are forward-declared now so M5–M7 do not need another vault migration.

**Done when:** `cargo test vault` passes including migration test; existing vaults unlock and produce a tree with one folder.

---

### Step 2 — `store` module: tree CRUD + search

**Files:** `src-tauri/src/store/mod.rs`, `src-tauri/src/commands.rs` (command registration)

**Commands** (all read/write the in-memory unlocked payload, then `vault_save`):

| Command | Signature | Notes |
|---|---|---|
| `tree_get` | `() -> Vec<TreeNode>` | Full tree |
| `conn_upsert` | `(conn: Connection) -> String` | Returns id; creates or replaces |
| `conn_delete` | `(id: String)` | Removes from `connections` and every `tree` branch |
| `folder_upsert` | `(id: Option<String>, parent_id: Option<String>, name: String) -> String` | Create (`id=None`) or rename (`id=Some`) |
| `folder_delete` | `(id: String, cascade: bool)` | Rejects if non-empty and `cascade=false` |
| `move_node` | `(id: String, new_parent_id: Option<String>, index: usize)` | Prevents cycles; single parent |
| `search` | `(query: String) -> Vec<SearchHit>` | `SearchHit { id, name, host, user, path: Vec<String> }` |

**Rules:**
- `move_node` rejects a move into own descendant with error code `cycle`.
- `conn_delete` also removes the id from any `jump` list (forward-safety for M5).
- `search` is case-insensitive substring on `name`, `host`, `user`; returns the breadcrumb `path` so the sidebar can expand to the hit.

**Review points:**
- `folder_upsert` is two operations in one command; acceptable, or split into `folder_create` / `folder_rename`? Current shape matches ROADMAP's single `folder_upsert`.
- `move_node`'s `index` — required in M4 or defer? Recommend including now; DnD needs it and it is cheap.

**Done when:** unit tests cover cycle rejection, cascade delete, search hits across folders, and idempotent upsert.

---

### Step 3 — Import parsers

**Files:** `src-tauri/src/store/import/mod.rs`, `ssh_config.rs`, `putty.rs`, `winsshterm.rs` (per D1), `src-tauri/src/commands.rs`

**Shared shape:**

```rust
struct ImportCandidate {
    source_label: String,     // e.g. "Host pgw-01 (line 12)"
    name: String,
    host: String,
    port: u16,
    user: String,
    auth: Auth,
}

struct ImportResult {
    created: Vec<Connection>,
    skipped: Vec<(ImportCandidate, SkipReason)>,
}
```

> **Implementation note (post-review).** The shipped shape uses `label`,
> `warnings`, `folder_path: Vec<String>` on `ImportCandidate` and an
> `ImportReport { created, folders_created, skipped }`. `folder_path` is needed
> because the real WinSSHTerm export encodes folders as recursively nested
> `<Node Type="Container">` elements; `import_apply` recreates that chain with
> `tree::ensure_path`. See `src-tauri/src/store/import/mod.rs`.

**Step 3a — `~/.ssh/config`** (`ssh_config.rs`)
- Parse `Host`, `HostName`, `Port`, `User`, `IdentityFile`.
- Skip `Host *` and wildcard-only hosts; record skip reason.
- `IdentityFile` → `auth = Key { path }`.
- Path resolution: `~` → home, relative → relative to the config file.

**Step 3b — PuTTY registry** (`putty.rs`, `#[cfg(windows)]`)
- Read `HKCU\Software\SimonTatham\PuTTY\Sessions` via `winreg`.
- Extract `HostName`, `PortNumber`, `UserName`.
- Encrypted PuTTY passwords cannot be decrypted without extra work — skip with reason `encrypted_password`, import rest.
- Non-Windows build: return empty with reason `platform_unsupported`.

**Step 3c — WinSSHTerm** (`winsshterm.rs`) — **blocked on D1**
- Defer to a follow-up commit within M4 unless a sample file arrives before Step 3 starts.

**Commands:**
- `import_preview(source, path_or_none) -> ImportResult` — parse only, do not write.
- `import_apply(source, path_or_none, dest_folder_id, conflict_policy) -> ImportResult` — write, returns created/skipped.

**Conflict policy** (D6): default skip on duplicate `(host, port, user)` across all existing connections; `ImportAsCopy` renames to `name_2`, `name_3`, …

**Review points:**
- Is "preview then apply" the right UX, or should import be atomic in one command? Recommend preview+apply — the preview is what makes conflict handling understandable.
- PuTTY skip list: confirm encrypted passwords are acceptable to skip, or should they import as `auth = Password { password: "" }` with a UI hint? Recommend skip with reason.

**Done when:** `import_preview` on a fixture `~/.ssh/config` returns the expected candidates; `import_apply` writes the tree and persists; the import dialog shows the preview.

---

### Step 4 — Sidebar tree UI

**Files:** `src/lib/ConnectionTree.svelte` (new), `src/lib/TreeNode.svelte` (new), `src/lib/SearchBox.svelte` (new), `src/lib/ImportDialog.svelte` (new), `src/lib/ConnectionSidebar.svelte` (replaced)

**Components:**

```
ConnectionSidebar.svelte
├── SearchBox.svelte
├── ConnectionTree.svelte          (recursive; renders TreeNode)
│   └── TreeNode.svelte            (folder or connection; expand/collapse; context menu)
├── ImportDialog.svelte            (modal; source picker → preview → apply)
└── [+ Folder] [+ Conn] buttons
```

**Behaviour:**
- Expand/collapse state is per-session (Svelte store), not persisted to the vault.
- Inline rename: double-click or F2 → editable label → Enter commits via `folder_upsert` / `conn_upsert`.
- Context menu on right-click: Open in new tab, Edit, New folder, New connection, Rename, Move to…, Duplicate, Delete.
- Search: typing filters via `search` command; matching parents auto-expand; matches are highlighted in labels.
- Delete: confirm dialog; shows cascade count for folders.

**Step 4b — Drag-and-drop** (optional, per D4)
- Native HTML5 DnD on `TreeNode`. Drop targets: onto a folder (move into), between siblings (reorder).
- Cycle guard is already enforced by `move_node` in Step 2 — the UI shows a toast on rejection.
- If D4 is deferred, this is the only piece of M4 that moves to M4b.

**Review points:**
- Search triggers on every keystroke — debounce at 150–200 ms, or explicit Enter? Recommend debounce.
- Where does the "Duplicates" action live — context menu only, or also a keyboard shortcut? Not critical for M4.

**Done when:** tree renders from `tree_get`; create/rename/delete/move work end-to-end against the backend; search reveals and highlights hits; import dialog round-trips.

---

### Step 5 — Connection editor extension

**Files:** `src/lib/ConnectionEditor.svelte` (extended)

**Sections (tabs within the editor):**
- **General:** name, host, port, user
- **Auth:** type selector (Password / Key file / Agent); Password → password field (visible only when vault unlocked); Key → path picker + optional passphrase; Agent → no fields
- **Colors:** foreground/background pickers + live preview swatch
- **Advanced:** keepalive seconds, protected checkbox

**Rules:**
- The editor is only usable when the vault is unlocked (existing M2 rule). Locked vault → read-only with a banner "Unlock to edit".
- Field validation: host required, port 1–65535, name required.

**Review points:**
- Are four tabs the right navigation, or a single scrolling form with sections? Recommend tabs — the spec's editor table lists five distinct concerns.
- Should changing `colors` on an open session apply live? Recommend yes (cheap; expected).

**Done when:** all four sections round-trip through `conn_upsert`; validation errors show inline; `auth.type = key` stores a path and `ssh.exe` picks it up on next open.

---

### Step 6 — Multi-tab workspace

**Files:** `src/lib/Workspace.svelte` (extended), `src/lib/TabBar.svelte` (new), `src/lib/TerminalPane.svelte` (minor: focus handling)

**Behaviour:**
- Tab bar: `[● pgw-01 ×] [ db-01 ×] [+]` — active tab shows a dot; `×` closes the session (kill PTY, flush log, remove tab).
- Each tab holds a `sessionId` and a mounted `TerminalPane`; inactive panes stay mounted but hidden (`display: none`).
- Only the active tab triggers PTY resize (already the M2/M3 rule — verify it still holds with N panes mounted).
- `[+]` opens a new connection from the currently selected tree node.
- Last tab closed → empty state ("Select a connection from the sidebar").
- Backend: `SessionState` already holds `HashMap<SessionId, Transport>`; no backend change expected. Verify with a manual test of N concurrent sessions.

**Review points:**
- Memory cap: each pane holds an xterm instance + screen buffer. Warn above 20 open tabs, or leave unbounded for M4? Recommend leave unbounded and revisit in M9 (Polish).
- Closing a tab while its log is being flushed: `session_close` already flushes; confirm `LogCapture` flushes its 250 ms batch on close (verify in M3 code).

**Done when:** open five sessions to five connections; switch tabs; close middle tab; all logs written correctly; PTY resize goes only to the active pane.

---

### Step 7 — Per-connection colors

**Files:** `src/lib/TerminalPane.svelte`, `src/lib/TabBar.svelte`

**Behaviour:**
- `colors.fg` / `colors.bg` applied to the xterm theme on pane mount.
- Tab accent uses the connection's foreground color.
- Unset → default theme (current).
- Live update when the editor saves colors for an open session.

**Review points:**
- Should the accent be the foreground or a derived color (e.g. saturated version)? Recommend foreground for simplicity; revisit in M9.
- Accessibility: ensure fg/bg contrast is readable; show a warning in the editor if contrast < 3:1? Recommend a non-blocking warning.

**Done when:** a connection with `colors` set opens with that theme and its tab is tinted; changing colors live updates the pane.

---

### Step 8 — Integration pass and checklist close-out

**Work:**
- Close the ROADMAP M4 checklist item by item.
- Manual test matrix (see §4).
- Confirm the exe still runs from a USB path (portable rule).
- Update README status table (M4 → ✅), ROADMAP M4 entry, and the spec's §5 "Current state" note.

**Done when:** the manual test matrix passes; the exe is portable; docs updated.

---

## 4. Manual test matrix

| # | Scenario | Expected |
|---|---|---|
| 1 | Fresh vault → create folder → create connection in it | Tree shows folder + connection; `tree_get` returns it |
| 2 | Rename folder and connection inline | Names update; persisted after lock/unlock |
| 3 | Delete folder with one connection, `cascade=true` | Folder and connection gone; confirm dialog showed count 1 |
| 4 | Delete non-empty folder, `cascade=false` | Rejected with error; nothing deleted |
| 5 | Move connection into a folder | Tree updates; connection still opens correctly |
| 6 | Move folder into its own descendant | Rejected with `cycle`; tree unchanged |
| 7 | Search for host substring | Matching connections revealed; parents expanded; label highlighted |
| 8 | Import `~/.ssh/config` (fixture with 3 hosts, 1 wildcard) | Preview shows 3 candidates, 1 skip with reason; apply writes 3 |
| 9 | Import duplicate `(host, port, user)` | Skipped by default; with "import as copy", renamed |
| 10 | Import PuTTY registry | Hosts imported; encrypted-password entries skipped with reason |
| 11 | Open five sessions, switch tabs | Each tab keeps its buffer; only active tab resizes PTY |
| 12 | Close middle tab | Session ends, log file written, tab removed |
| 13 | Set colors on a connection, open it, change colors live | Theme applied; live update works |
| 14 | Lock vault, unlock | Tree persisted; editor read-only when locked |
| 15 | Run exe from a USB path | Vault, logs, config all resolve from exe dir |

---

## 5. Risks and mitigations

| Risk | Likelihood | Mitigation |
|---|---|---|
| WinSSHTerm format unknown (D1) | High | Defer Step 3c; ship two of three sources in M4 |
| DnD regressions block other UI | Medium | Step 4b is separable; cycle guard is in backend (Step 2) |
| Vault migration breaks old vaults (D2) | Medium | Migration unit test with a synthetic M3 payload; auto-backup `.bak` already exists |
| xterm memory with many tabs | Low in M4 | Leave unbounded; warn above 20; revisit in M9 |
| PuTTY encrypted passwords | Known | Skip with reason; documented in the dialog |

---

## 6. Suggested commit sequence

Each commit leaves the build green and is independently reviewable.

1. `M4 step 1: typed vault tree + migration`
2. `M4 step 2: store CRUD + search commands`
3. `M4 step 4 (no DnD): sidebar tree, search, context menu`
4. `M4 step 5: connection editor sections`
5. `M4 step 6: multi-tab workspace`
6. `M4 step 7: per-connection colors`
7. `M4 step 3a/b: ssh config + PuTTY import`
8. `M4 step 3c: WinSSHTerm import` (follow-up; depends on D1)
9. `M4 step 4b: drag-and-drop` (follow-up; depends on D4)
10. `M4 step 8: integration, docs, checklist close-out`

---

## 7. Open questions (blocking)

1. **D1** — Can you share a WinSSHTerm connections file, or should Step 3c be deferred to a follow-up commit?
2. **D2** — Confirm auto-migration of M1–M3 vaults into a single "Connections" folder.
3. **D3** — Confirm recursive folder delete by default with a confirm dialog.
4. **D4** — Ship DnD in M4, or move it to M4b?
5. **D5** — Confirm UUID v4 with `f_` / `c_` display prefixes.
6. **D6** — Confirm skip-on-duplicate default with an "import as copy" option.

This proposal is retained as an implementation-history record. Its open questions are historical, not current blockers. M4 shipped the typed tree, CRUD/search/import, connection editor, browser drag-and-drop and multi-tab workspace. Per-connection colors remain deferred to M9.

---

### Notes on this document

- It deliberately **restates the decisions from my earlier review as D1–D6** with recommendations, so the document is self-contained and the open questions are explicit blockers rather than implicit assumptions.
- The **step order differs from the strict spec order** on purpose: backend model and CRUD come first (Steps 1–2), the tree UI without DnD comes third (Step 4), then editor/tabs/colors (5–7), then import (3) and DnD (4b) last. This puts the riskiest, format-dependent work (import) and the riskiest UI work (DnD) at the end, where a slip only delays a sub-step, not the milestone.
- The **manual test matrix** is the acceptance gate for Step 8; it covers each ROADMAP checklist item plus the migration and portability rules.
- **Commit sequence** (§6) is written to be directly usable — ten commits, each independently reviewable, with the two follow-up commits (3c, 4b) clearly separated.

If you'd like, the next step can be either (a) answering D1–D6 so I can finalize the doc, or (b) starting Step 1 (typed vault model + migration) against the current codebase.