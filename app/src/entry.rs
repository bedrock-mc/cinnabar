use cinnabar::{
    args::{ClientArgs, ParseOutcome},
    lifecycle, run,
};
use std::io::Write;

/// Dispatches client startup and reports failures before the process exits.
fn main() {
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == update_manifest::build::QUERY_FLAG)
    {
        println!(
            "{}",
            serde_json::to_string(&update_manifest::build::Build::current())
                .expect("build identity is serializable")
        );
        return;
    }
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == first_run::SETUP_FLAG)
    {
        std::process::exit(first_run::run_setup_process());
    }
    match ClientArgs::parse_env() {
        Ok(ParseOutcome::Help) => print!("{}", cinnabar::args::HELP),
        Ok(ParseOutcome::Run(args)) => {
            match lifecycle::before_run(args.assets.is_some()) {
                Ok(true) => {}
                Ok(false) => return,
                Err(error) => {
                    launcher_host::lifecycle::update::recover_after_failed_start();
                    let _ = writeln!(diagnostics::console::stderr(), "cinnabar failed: {error:#}");
                    diagnostics::console::flush_before_exit();
                    std::process::exit(1);
                }
            }
            let result = run(*args);
            if let Err(error) = result {
                launcher_host::lifecycle::update::recover_after_failed_start();
                let _ = writeln!(diagnostics::console::stderr(), "cinnabar failed: {error:#}");
                diagnostics::console::flush_before_exit();
                std::process::exit(1);
            }
            launcher_host::lifecycle::update::apply_after_exit();
            diagnostics::console::flush_before_exit();
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}
