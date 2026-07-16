use bedrock_client::{
    args::{ClientArgs, ParseOutcome},
    run,
};

fn main() {
    match ClientArgs::parse_env() {
        Ok(ParseOutcome::Help) => print!("{}", bedrock_client::args::HELP),
        Ok(ParseOutcome::Run(args)) => {
            if let Err(error) = run(*args) {
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
