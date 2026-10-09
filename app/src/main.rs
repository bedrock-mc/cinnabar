use bedrock_client::{
    args::{ClientArgs, ParseOutcome},
    lifecycle, run,
};

fn main() {
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == lifecycle::FIRST_RUN_SETUP_FLAG)
    {
        std::process::exit(lifecycle::run_first_run_setup());
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
            if let Err(error) = run(*args) {
                // Queued log lines precede the failure that ends them.
                diagnostics::console::flush();
                eprintln!("bedrock-client failed: {error:#}");
                std::process::exit(1);
            }
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}
