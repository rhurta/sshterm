# M8b Windows acceptance panel

The temporary **SFTP QA** launcher is shown in the bottom-right corner whenever the vault is unlocked on `feature/m8-sftp`. It opens a developer-only panel that exercises the M8b Tauri commands directly. It is not the M8c SFTP browser and deliberately has no upload, download, rename, or delete operations.

## Panel controls

- Search saved connections and open one or more SFTP sessions.
- Inspect connection endpoint and resolved jump-host names.
- List remote directories, open child directories, go to the parent, enter a POSIX path, and refresh.
- Close one session or call `sftp_close_all`.
- Inspect a bounded activity log. No authentication material is returned or displayed.
- Use **Lock vault and close SFTP** to run the normal application lock path. The panel saves the current runtime session IDs outside the component before locking.
- After unlocking, reopen **SFTP QA** and select **Verify vault-lock cleanup**. The panel calls `sftp_list` with each saved session ID and expects `sftp_session_not_found`.

## Suggested manual acceptance

1. Use saved Windows OpenSSH test connections for direct password auth, key/passphrase auth, and agent auth where available.
2. Open the target directly, list the configured start directory, enter nested and empty directories, and check spaced/non-ASCII filenames.
3. Open a target configured with one jump host, then with a multi-hop chain. Confirm the displayed route and verify the target is not directly reachable from the client.
4. Open more than one SFTP session. Close one and verify the other remains usable; then use **Close all**.
5. Open one or more sessions, select **Lock vault and close SFTP**, unlock the vault, reopen the panel, and run **Verify vault-lock cleanup**. Every previous ID must be rejected with `sftp_session_not_found`.
6. Close the app while sessions are open and check that no SFTP connection remains active.

This panel is temporary M8b acceptance tooling. Remove it after the Windows acceptance checklist is complete and before or as part of the M8c browser work.
