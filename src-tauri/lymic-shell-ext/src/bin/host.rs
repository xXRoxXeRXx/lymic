//! The out-of-process COM server of the Virtual Library thumbnail provider, declared in the MSIX
//! manifest as `com:ExeServer`. COM starts it when Explorer needs a thumbnail.

#[cfg(windows)]
fn main() {
    std::process::exit(lymic_shell_ext::shell::run_host());
}

#[cfg(not(windows))]
fn main() {
    eprintln!("lymic-shell-ext-host is a Windows COM server.");
    std::process::exit(1);
}
