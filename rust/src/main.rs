#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod native;

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    native::run()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("YiSou requires Windows.");
}
