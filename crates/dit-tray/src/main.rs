//! DIT's menu bar app (ADR 0029): an icon that starts and stops `dit ui`,
//! opens DIT in the browser, and lists the workspaces on this machine. No
//! window — the browser is DIT's only window, as DESIGN.md §6.5 chose.

// Built everywhere so their tests run on every CI platform; used only by
// the macOS app.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod git;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod icon;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod server;

#[cfg(target_os = "macos")]
mod app;

#[cfg(target_os = "macos")]
fn main() -> std::process::ExitCode {
    app::run()
}

#[cfg(not(target_os = "macos"))]
#[allow(clippy::print_stderr)]
fn main() -> std::process::ExitCode {
    eprintln!("dit-tray: the menu bar app is macOS-only for now — run `dit ui --all` instead");
    std::process::ExitCode::from(1)
}
