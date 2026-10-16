//! `cinnabar-launcher`: the installed entry point. It shows installed and available releases,
//! installs updates on request and starts and supervises the game.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// Without the webview (Linux checks lacking WebKitGTK) only the Tauri-free core is built.
#![cfg_attr(not(webview), allow(dead_code))]

mod api;
#[cfg(webview)]
mod app;
mod changelog;
#[cfg(webview)]
mod commands;
#[cfg(test)]
mod contract;
mod featured;
mod game;
mod install;
mod news;
mod surface;

use update_manifest::build::Build;

#[expect(
    clippy::print_stdout,
    reason = "the build query answers the update helper on stdout"
)]
#[cfg_attr(
    not(webview),
    expect(
        clippy::print_stderr,
        reason = "no window exists to report the missing feature"
    )
)]
fn main() {
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == update_manifest::build::QUERY_FLAG)
    {
        println!(
            "{}",
            serde_json::to_string(&Build::current()).expect("build identity is serializable")
        );
        return;
    }
    #[cfg(webview)]
    if let Err(error) = app::run() {
        tracing::error!("launcher failed: {error:#}");
        std::process::exit(1);
    }
    #[cfg(not(webview))]
    {
        eprintln!("this launcher was built without its window; enable linux-webview");
        std::process::exit(1);
    }
}
