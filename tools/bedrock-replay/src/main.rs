//! Replays saved server packets through the client's local session endpoint.
mod capture;
mod options;
mod packs;
mod replay;
mod runner;
#[cfg(test)]
mod tests;

use anyhow::Result;

/// Owns signal cancellation and reports a nonzero exit for interrupted or incomplete runs.
#[tokio::main]
async fn main() {
    if let Err(error) = command().await {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}

/// Parses the compatible CLI before running the offline server.
async fn command() -> Result<()> {
    let Some(options) = options::Options::parse(std::env::args().skip(1))? else {
        print!("{}", options::HELP);
        return Ok(());
    };
    runner::run(options, &mut std::io::stdout(), interrupted()).await
}

/// Resolves on an interrupt or termination signal without spawning a detached task.
async fn interrupted() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
    }
    #[cfg(windows)]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
