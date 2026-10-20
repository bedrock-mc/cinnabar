//! Standalone native HTTPS/WebSocket gateway using one explicitly owned Tokio runtime.

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use clap::Parser;
    use std::{error::Error, future::Future, io, net::SocketAddr, path::PathBuf};
    use web_gateway::operator::{self, ListenConfig};

    #[derive(Parser)]
    #[command(about = "Serve the Cinnabar browser relay over HTTPS/WebSocket")]
    struct Options {
        /// Hostname or IP address covered by the certificate.
        #[arg(long)]
        hostname: String,
        /// HTTPS/WebSocket listen address, including its TCP port.
        #[arg(long)]
        tcp: SocketAddr,
        /// Optional paired WebTransport UDP listen address.
        #[cfg(feature = "webtransport")]
        #[arg(long)]
        udp: Option<SocketAddr>,
        /// PEM certificate chain file.
        #[arg(long)]
        tls_cert: PathBuf,
        /// PEM unencrypted private key file.
        #[arg(long)]
        tls_key: PathBuf,
        /// Exact allowed origins, comma separated; empty means same-authority policy.
        #[arg(long, value_delimiter = ',')]
        origins: Vec<String>,
        /// Tracing filter for operator diagnostics.
        #[arg(long, default_value = "info")]
        log_level: String,
    }

    /// Owns the only runtime and waits for handler/account retirement before dropping it.
    pub(super) fn command() -> Result<(), Box<dyn Error>> {
        let options = Options::parse();
        let filter = tracing_subscriber::EnvFilter::try_new(options.log_level)?;
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .try_init()
            .map_err(io::Error::other)?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        let config = ListenConfig {
            hostname: options.hostname,
            tcp_address: options.tcp,
            certificate_file: options.tls_cert,
            key_file: options.tls_key,
            origins: options
                .origins
                .into_iter()
                .map(|origin| origin.trim().to_owned())
                .filter(|origin| !origin.is_empty())
                .collect(),
        };
        runtime.block_on(async move {
            let shutdown = shutdown()?;
            #[cfg(feature = "webtransport")]
            if let Some(udp) = options.udp {
                return operator::run_paired(config, udp, shutdown).await;
            }
            operator::run(config, shutdown).await
        })?;
        Ok(())
    }

    /// Installs signal ownership before loading certificates or starting listener work.
    #[cfg(unix)]
    fn shutdown() -> io::Result<impl Future<Output = ()>> {
        use tokio::signal::unix::{SignalKind, signal};
        let mut interrupt = signal(SignalKind::interrupt())?;
        let mut terminate = signal(SignalKind::terminate())?;
        Ok(async move {
            tokio::select! { _ = interrupt.recv() => {}, _ = terminate.recv() => {} }
        })
    }

    /// Uses the platform's console interrupt without a detached signal task.
    #[cfg(windows)]
    fn shutdown() -> io::Result<impl Future<Output = ()>> {
        let mut interrupt = tokio::signal::windows::ctrl_c()?;
        Ok(async move {
            interrupt.recv().await;
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    native::command()
}

#[cfg(target_arch = "wasm32")]
fn main() -> Result<(), &'static str> {
    Err("the relay operator requires a native I/O runtime")
}
