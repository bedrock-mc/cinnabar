use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Check {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        policy: PathBuf,
    },
}

fn main() {
    let Cli {
        command: Command::Check { root, policy },
    } = Cli::parse();
    match architecture::check_repository(&root, &policy) {
        Ok(diagnostics) if diagnostics.is_empty() => {}
        Ok(diagnostics) => {
            for diagnostic in diagnostics {
                eprintln!("{diagnostic}");
            }
            std::process::exit(1);
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}
