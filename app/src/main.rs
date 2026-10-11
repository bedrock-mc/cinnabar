use bedrock_client::{
    args::{ClientArgs, ParseOutcome},
    lifecycle, run,
};
use std::io::Write;

fn main() {
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == first_run::SETUP_FLAG)
    {
        std::process::exit(first_run::run_setup_process());
    }
    match ClientArgs::parse_env() {
        Ok(ParseOutcome::Help) => print!("{}", bedrock_client::args::HELP),
        Ok(ParseOutcome::Run(args)) => {
            match lifecycle::before_run(args.assets.is_some()) {
                Ok(true) => {}
                Ok(false) => return,
                Err(error) => {
                    eprintln!("bedrock-client failed: {error:#}");
                    std::process::exit(1);
                }
            }
            let result = run(*args);
            if let Err(error) = result {
                let _ = writeln!(
                    diagnostics::console::stderr(),
                    "bedrock-client failed: {error:#}"
                );
                diagnostics::console::flush_before_exit();
                std::process::exit(1);
            }
            diagnostics::console::flush_before_exit();
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}
