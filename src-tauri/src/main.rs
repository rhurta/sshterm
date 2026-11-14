// Prevents an extra console window on Windows in release builds. Do not remove.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // The same exe acts as the SSH_ASKPASS helper when ssh.exe invokes it.
    if let Some(code) = sshterm_lib::maybe_run_askpass() {
        std::process::exit(code);
    }
    sshterm_lib::run()
}
