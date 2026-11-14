//! Named pipe DACL (Windows only).
//!
//! tokio's `ServerOptions::create` builds the pipe with the default security
//! descriptor, which may allow access by other local users. We cannot tighten
//! the DACL afterwards because the handle tokio returns does not grant
//! `WRITE_DAC` (SetSecurityInfo fails with ERROR_ACCESS_DENIED), and the
//! by-name variant is unsupported for pipe servers (ERROR_INVALID_PARAMETER).
//!
//! The working approach is to create the pipe ourselves with
//! `CreateNamedPipeW`, passing a `SECURITY_ATTRIBUTES` whose DACL grants
//! GENERIC_READ | GENERIC_WRITE only to the current user's SID. tokio then
//! wraps that already-secured handle via `NamedPipeServer::from_raw_handle`.
//! If any step fails, `AskpassServer::start` aborts and never exposes a pipe
//! with broader permissions than intended (fail closed).

use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::RawHandle;
use std::ptr;

use tokio::net::windows::named_pipe::NamedPipeServer;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, GENERIC_READ, GENERIC_WRITE, HANDLE,
};
use windows_sys::Win32::Security::{
    AddAccessAllowedAce, GetTokenInformation, InitializeAcl, InitializeSecurityDescriptor,
    SetSecurityDescriptorDacl, ACL, ACL_REVISION, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
    SECURITY_DESCRIPTOR, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FILE_FLAGS_AND_ATTRIBUTES,
    PIPE_ACCESS_DUPLEX, PIPE_ACCESS_INBOUND, PIPE_ACCESS_OUTBOUND,
};
use windows_sys::Win32::System::Pipes::{
    CreateNamedPipeW, NAMED_PIPE_MODE, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

// SECURITY_DESCRIPTOR_REVISION is always 1; defining it locally avoids pulling
// in the Win32_System_SystemServices feature for a single constant.
const SECURITY_DESCRIPTOR_REVISION: u32 = 1;

fn err(step: &str) -> String {
    let code = unsafe { GetLastError() };
    format!("askpass_acl:{step}:win32={code}")
}

/// Builds an absolute (in-process) security descriptor whose DACL grants
/// GENERIC_READ | GENERIC_WRITE to the current user only. The returned
/// `SECURITY_ATTRIBUTES` references the caller-provided `sd` and `acl_buf`,
/// which must outlive the `CreateNamedPipeW` call.
fn build_security_attributes(
    sd: &mut SECURITY_DESCRIPTOR,
    acl_buf: &mut [u64],
) -> Result<SECURITY_ATTRIBUTES, String> {
    unsafe {
        let mut token: HANDLE = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(err("open_process_token"));
        }

        let mut needed: u32 = 0;
        let _ = GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut needed);
        if needed == 0 {
            let _ = CloseHandle(token);
            return Err(err("token_size"));
        }
        // 8-byte aligned scratch: TOKEN_USER holds a pointer (PSID).
        let mut info_buf = vec![0u64; (needed as usize + 7) / 8];
        if GetTokenInformation(
            token,
            TokenUser,
            info_buf.as_mut_ptr() as *mut _,
            needed,
            &mut needed,
        ) == 0
        {
            let _ = CloseHandle(token);
            return Err(err("token_info"));
        }
        let sid = (*(info_buf.as_ptr() as *const TOKEN_USER)).User.Sid;
        let _ = CloseHandle(token);

        // 256-byte ACL: more than enough for one SID (max 68 bytes) + one ACE.
        let acl = acl_buf.as_mut_ptr() as *mut ACL;
        if InitializeAcl(acl, 256, ACL_REVISION) == 0 {
            return Err(err("init_acl"));
        }
        if AddAccessAllowedAce(acl, ACL_REVISION, GENERIC_READ | GENERIC_WRITE, sid) == 0 {
            return Err(err("add_ace"));
        }

        if InitializeSecurityDescriptor(
            sd as *mut _ as PSECURITY_DESCRIPTOR,
            SECURITY_DESCRIPTOR_REVISION,
        ) == 0
        {
            return Err(err("init_sd"));
        }
        if SetSecurityDescriptorDacl(sd as *mut _ as PSECURITY_DESCRIPTOR, 1, acl, 0) == 0 {
            return Err(err("set_dacl"));
        }

        Ok(SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd as *mut _ as *mut core::ffi::c_void,
            bInheritHandle: 0,
        })
    }
}

/// Creates a named pipe server whose DACL is restricted to the current user,
/// and wraps it in a tokio `NamedPipeServer`. Equivalent to:
///   ServerOptions::new()
///     .first_pipe_instance(true)
///     .reject_remote_clients(true)
///     .create(name)
/// but with a current-user-only DACL applied at creation time.
pub fn create_secure_pipe(
    name: &str,
    in_buffer: u32,
    out_buffer: u32,
    inbound: bool,
    outbound: bool,
) -> Result<NamedPipeServer, String> {
    let mut sd = SECURITY_DESCRIPTOR::default();
    let mut acl_buf = vec![0u64; 32];
    let sa = build_security_attributes(&mut sd, &mut acl_buf)?;

    let wide: Vec<u16> = std::ffi::OsStr::new(name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut open_mode: FILE_FLAGS_AND_ATTRIBUTES =
        FILE_FLAG_FIRST_PIPE_INSTANCE | FILE_FLAG_OVERLAPPED;
    if inbound && outbound {
        open_mode |= PIPE_ACCESS_DUPLEX;
    } else if outbound {
        open_mode |= PIPE_ACCESS_OUTBOUND;
    } else if inbound {
        open_mode |= PIPE_ACCESS_INBOUND;
    }

    let pipe_mode: NAMED_PIPE_MODE =
        PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS;

    // CreateNamedPipeW copies the security descriptor into the kernel object,
    // so the stack-local `sa`/`sd`/`acl_buf` are safe to drop after this call.
    let handle: HANDLE = unsafe {
        CreateNamedPipeW(
            wide.as_ptr(),
            open_mode,
            pipe_mode,
            1, // single instance
            out_buffer,
            in_buffer,
            0, // default timeout
            &sa as *const _ as *mut _,
        )
    };

    if handle as isize == -1 {
        return Err(err("create_named_pipe"));
    }

    // Safety: CreateNamedPipeW returned a valid, owned pipe server handle with
    // FILE_FLAG_OVERLAPPED set, which is what tokio requires.
    let server = unsafe { NamedPipeServer::from_raw_handle(handle as RawHandle) }
        .map_err(|e| format!("askpass_acl:from_raw_handle:{e}"))?;
    Ok(server)
}

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use super::*;

    /// The secured pipe must be created successfully and a second instance
    /// with the same name must be rejected (proving the first one is real and
    /// exclusive). Cross-user denial is enforced by the DACL and verified by
    /// code review rather than a cross-user integration test.
    #[test]
    fn secure_pipe_creates_and_blocks_second_instance() {
        let name = format!(r"\\.\pipe\sshterm-acl-test-{}", std::process::id());
        // from_raw_handle requires a tokio reactor with I/O enabled.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let server = create_secure_pipe(&name, 4096, 4096, true, true)
                .expect("first create_secure_pipe must succeed");
            let second = create_secure_pipe(&name, 4096, 4096, true, true);
            assert!(second.is_err(), "second instance should be rejected");
            drop(server);
        });
    }

    /// The askpass helper opens the pipe with std::fs::OpenOptions. If the DACL
    /// grants generic (unmapped) rights instead of specific ones, that open
    /// fails with ERROR_ACCESS_DENIED and ssh waits forever for a password.
    #[test]
    fn helper_can_open_the_secured_pipe() {
        use std::time::Duration;

        let name = format!(r"\\.\pipe\sshterm-acl-open-{}", std::process::id());
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let server = create_secure_pipe(&name, 4096, 4096, true, true)
                .expect("create_secure_pipe must succeed");

            let client_name = name.clone();
            let handle = std::thread::spawn(move || {
                std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&client_name)
            });

            tokio::time::timeout(Duration::from_secs(5), server.connect())
                .await
                .expect("client never connected")
                .expect("server connect failed");

            let result = handle.join().expect("client thread panicked");
            match result {
                Ok(_file) => {}
                Err(e) => panic!("helper cannot open pipe: {e}"),
            }
        });
    }
}
