# Scripts library

The command console has two buttons to keep reusable command scripts: **Save script…**
and **Load script…**. Scripts live in folders, in a second encrypted file.

## Why a separate vault

Scripts often contain host names, IP addresses and sometimes tokens, so they get the
same protection as connections, but in their own file, `scripts.vault`, next to
`sshterm.vault` (same format: Argon2 key, XChaCha20-Poly1305, atomic writes with a
`.bak` copy).

- A script library can be shared or backed up **without** exposing connections or
  credentials, and it can have its own password.
- A problem with the library cannot affect the connections vault, and the connections
  vault stays small.
- Costs: one more file to carry in a portable setup, and password handling for two
  files (below).

## Passwords

- **Default: the same master password.** When you unlock sshterm, the password you
  type also opens `scripts.vault`, in the same background task. If the file does not
  exist yet it is created empty. There is no extra prompt and the password is not
  kept after the unlock.
- **Changing the master password** re-encrypts the library too, when it was opened
  with the master password. (If re-encrypting the library fails, the connections vault
  is already changed; the next unlock then asks for the old password for the library.)
- **A library with its own password** (for example a shared one): if the master
  password does not open the file, the Scripts dialog shows an "unlock library" prompt
  with a password field. That unlock lasts until you lock sshterm, and changing the
  master password does not touch that file.
- Locking sshterm closes the library too.

## Where the file is

`scripts.vault` next to the exe by default. To use another place (a synced folder, a
network share), set it in `config.toml`:

```toml
scripts_vault = "D:/Shared/team-scripts.vault"   # absolute, or relative to the exe folder
```

Last write wins: if two people edit the same shared file at the same time, the later
save overwrites the earlier one. Coordinate or keep personal libraries.

## Using it

**Save script…** (enabled when the editor has text)
- Opens the dialog with a name field (focused, prefilled from the first line of the
  script, or the name of the loaded script) and the folder tree, which is the folder
  picker: select a folder, or none for the top level.
- Picking an existing script in the tree fills the name, like a file dialog.
- If the text came from a loaded script, **Update “name”** saves over it (and applies
  a changed name or folder); **Save as new** creates another one.
- A new script whose name already exists in that folder asks before overwriting. A
  folder is never overwritten.

**Load script…**
- Tree on the left, a read-only preview on the right, and a search field (focused on
  open) that matches script names, folder names and script contents.
- Enter or double-click loads the selected script. If the editor already has text,
  you choose **Replace the text** or **Append to the text**. Loading never sends
  anything: the multi-line confirmation and target selection of the console still apply.
- The console shows `Script: name` while its text comes from a script; Clear resets it.

**Managing the library** (both dialogs): New folder, Rename (F2), Move to…, Delete
(Delete key; folders delete with their contents, after a confirmation).

Keyboard: Up/Down/Home/End move, Right/Left expand or collapse and go to the parent,
Enter loads or toggles, Escape closes one layer at a time (inline edit, confirmation,
search, dialog). The dialog has a fixed size proportional to the window.

Limits: scripts up to 256 KB, names up to 100 characters, unique among siblings
(case-insensitive).

## Not in this version

Variables or placeholders, tags, running a script directly from the library, linking
scripts to connections, import/export of `.sh` files, and a setting in the UI for the
library path (use `config.toml`).

## Implementation

- `vault::SealedFile<D>` is the generic encrypted container; `Vault` (connections) and
  `ScriptsVault` are type aliases over it.
- `vault/scripts.rs`: the tree (`ScriptNode`), validation and operations (save, rename,
  move with cycle check, delete, search), pure and unit-tested.
- `script_store.rs`: `ScriptsState` and the `scripts_*` commands. Every change is
  saved at once and rolled back in memory if the write fails.
- Frontend: `ScriptsDialog.svelte`, `scriptsTree.ts` (visible rows, search filter,
  folder options, name suggestion; tested), and the two buttons in `CommandConsole.svelte`.
