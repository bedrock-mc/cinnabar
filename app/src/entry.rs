use cinnabar::{
    args::{ClientArgs, ParseOutcome},
    lifecycle, run,
};
use launcher_host::lifecycle::crash;
use std::io::Write;

/// Dispatches client startup and reports failures before the process exits.
#[expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "--query-build and --help answer on stdout; argument errors precede logging setup"
)]
pub(crate) fn dispatch() {
    let mode = std::env::args_os().nth(1);
    if mode
        .as_ref()
        .is_some_and(|arg| arg == update_manifest::build::QUERY_FLAG)
    {
        println!(
            "{}",
            serde_json::to_string(&update_manifest::build::Build::current())
                .expect("build identity is serializable")
        );
        return;
    }
    if mode
        .as_ref()
        .is_some_and(|arg| arg == first_run::SETUP_FLAG)
    {
        std::process::exit(lifecycle::run_first_run_setup());
    }
    if mode.as_ref().is_some_and(|arg| arg == crash::MONITOR_FLAG) {
        std::process::exit(lifecycle::run_crash_monitor());
    }
    if mode
        .as_ref()
        .is_some_and(|arg| arg == bootstrap_ui::startup::FLAG)
    {
        if let Err(error) = bootstrap_ui::startup::run() {
            eprintln!("startup screen failed: {error:#}");
            std::process::exit(1);
        }
        return;
    }
    match ClientArgs::parse_env() {
        Ok(ParseOutcome::Help) => print!("{}", cinnabar::args::HELP),
        Ok(ParseOutcome::Run(args)) => {
            match lifecycle::before_run(args.assets.is_some()) {
                Ok(true) => {}
                Ok(false) => {
                    crash::announce_exit(0);
                    return;
                }
                Err(error) => fail(&error),
            }
            if let Err(error) = run(*args) {
                fail(&error);
            }
            launcher_host::lifecycle::update::apply_after_exit();
            diagnostics::console::flush_before_exit();
            crash::announce_exit(0);
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}

/// Logs why `main` failed, shows it when no console can, and exits with 1.
fn fail(error: &anyhow::Error) -> ! {
    launcher_host::lifecycle::update::recover_after_failed_start();
    let _ = writeln!(diagnostics::console::stderr(), "cinnabar failed: {error:#}");
    diagnostics::console::flush_before_exit();
    lifecycle::show_failure(error);
    crash::announce_exit(1);
    std::process::exit(1);
}
