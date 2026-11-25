//! The out-of-process COM server of the Virtual Library thumbnail provider, declared in the MSIX
//! manifest as `com:ExeServer`. COM starts it when Explorer needs a thumbnail.

// A GUI-subsystem binary: as a console binary, every COM activation opened a visible cmd window.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    std::process::exit(lymic_shell_ext::shell::run_host());
}

#[cfg(not(windows))]
fn main() {
    eprintln!("lymic-shell-ext-host is a Windows COM server.");
    std::process::exit(1);
}
