# M8a — Protocol and security spike

**Branch:** `feature/m8-sftp`  
**Status:** In progress

## First increment

- SFTP target port override is optional. A blank or omitted override inherits the target connection's SSH port.
- Existing vault records with numeric `sftp.port` remain explicit overrides when deserialized.
- Jump hosts continue using their own saved SSH ports; the target's SFTP override does not affect hops.
- The editor accepts a blank SFTP port and explains inheritance in English and Spanish.
- The properties pane shows when the SFTP port inherits the connection's SSH port.

This increment does not yet establish a Russh connection or SFTP subsystem. Those are the core protocol-spike gates.

## Security baseline

Use `russh 0.65.2` and `russh-sftp 3.0.1` after confirming transitive dependency compatibility and the chosen crypto feature on Windows.

**Do not use Russh before 0.65.1.** The affected versions allowed an overridden `client::Handler::kex_done()` callback to run with KEX shared-secret material before server host-key trust was established. Keep the default no-op handler; do not expose or act on that secret. Fixed in 0.65.1. See [GHSA-9264-jvgf-6w2f](https://github.com/Eugeny/russh/security/advisories/GHSA-9264-jvgf-6w2f) and the [OSV advisory](https://osv.dev/vulnerability/GHSA-9264-jvgf-6w2f).

Use the portable `known_hosts` file and preserve OpenSSH's accept-new semantics: match existing normal/hashed entries and non-default ports; learn only unknown keys; reject changed keys without replacing the trusted key. This policy must apply to every hop and the target.

## Proof matrix

| Case | Required result | Status |
|---|---|---|
| Old vault numeric `sftp.port` | Decode as an explicit override | Model test added |
| Missing SFTP port | Inherit target connection SSH port | Model test added |
| No SFTP section | Inherit target connection SSH port | Model test added |
| Direct password and key auth | Authenticate against local OpenSSH | Pending |
| Windows SSH agent / Pageant | Use supported agent signer | Pending |
| SFTP subsystem over `Channel::into_stream()` | Initialize `SftpSession` | Pending |
| Known/unknown/changed host keys | Match / learn / reject | Pending |
| Hashed host and non-22 port | Correct known-host matching | Pending |
| Two-hop `direct-tcpip` route | Target SFTP works through each hop | Pending |
| Distinct hop auth | Each hop uses its own credentials | Pending |
| Windows x64 CI and portable executable | All checks pass | Pending |

## Dependency and lockfile gate

The repository CI runs `cargo test --locked`. Any `Cargo.toml` dependency change must include the regenerated `src-tauri/Cargo.lock`. Pin the tested Russh/SFTP releases and select one crypto backend in that commit.

## References

- [Russh client API 0.65.2](https://docs.rs/russh/0.65.2/russh/client/)
- [Russh client Handler](https://docs.rs/russh/0.65.2/russh/client/trait.Handler.html)
- [Russh SFTP client example](https://github.com/Eugeny/russh/blob/master/russh/examples/sftp_client.rs)
- [russh-sftp 3.0.1 API](https://docs.rs/russh-sftp/3.0.1/russh_sftp/)
- [GHSA-9264-jvgf-6w2f / CVE-2026-128087](https://osv.dev/vulnerability/GHSA-9264-jvgf-6w2f)
