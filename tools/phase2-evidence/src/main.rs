use std::path::PathBuf;

use clap::{Parser, Subcommand};
use phase2_evidence::{EvidenceKind, compare_files};

#[derive(Debug, Parser)]
#[command(name = "phase2-evidence")]
#[command(about = "Bounded, path-free Phase 2 image evidence comparator")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Compare {
        #[arg(long, value_enum)]
        kind: EvidenceKind,
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        native: PathBuf,
        #[arg(long)]
        cinnabar: PathBuf,
        #[arg(long = "out")]
        output: PathBuf,
    },
}

fn main() {
    let result = match Cli::parse().command {
        Command::Compare {
            kind,
            manifest,
            native,
            cinnabar,
            output,
        } => compare_files(kind, &manifest, &native, &cinnabar, &output),
    };
    match result {
        Ok(report) if report.passed => {}
        Ok(_) => {
            eprintln!("error: comparison exceeded one or more thresholds");
            std::process::exit(1);
        }
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    }
}
