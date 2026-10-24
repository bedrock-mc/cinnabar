//! Native HTTPS/WebSocket listener and certificate-file ownership on the caller's runtime.

mod certificate;
#[cfg(feature = "webtransport")]
mod webtransport;
#[cfg(feature = "webtransport")]
pub use webtransport::run_paired;

use crate::Gateway;
use bridge::contract::RELAY_CERTIFICATE_RELOAD_SECONDS;
use certificate::Certificates;
use futures::{StreamExt, stream};
use std::{future::Future, io, net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

/// Operator inputs for the native HTTPS/WebSocket listener.
///
/// The hostname must match the certificate. Origins use the same exact-match policy as
/// [`Gateway::new`]. Files contain the PEM certificate chain and its unencrypted private key.
/// This entrypoint serves the existing WebSocket routes; it does not open a QUIC listener.
pub struct ListenConfig {
    pub hostname: String,
    pub tcp_address: SocketAddr,
    pub certificate_file: PathBuf,
    pub key_file: PathBuf,
    pub origins: Vec<String>,
}

/// Runs the actual gateway until shutdown or a listener/service failure.
///
/// Initial TLS material is validated before binding. Replacements affect new handshakes;
/// invalid replacements retain the last valid certificate. Ending admission awaits the existing
/// connection handlers before their ephemeral account services are released. Dropping this
/// future remains abortive, like [`Gateway::run`]. No runtime or detached worker is created here.
///
/// # Errors
///
/// Returns startup configuration, file, bind or gateway service errors. Certificate errors never
/// include the key, certificate contents or input returned by a peer.
pub async fn run(config: ListenConfig, shutdown: impl Future<Output = ()>) -> io::Result<()> {
    let certificates = Arc::new(Certificates::load(&config).await?);
    let gateway = Gateway::with_resolver(certificates.clone(), config.origins.clone())?;
    let listener = TcpListener::bind(config.tcp_address).await?;
    serve(config, certificates, gateway, listener, shutdown).await
}

/// Polls reload I/O beside the original gateway instead of stalling its connection progress.
async fn serve(
    config: ListenConfig,
    certificates: Arc<Certificates>,
    gateway: Gateway,
    listener: TcpListener,
    shutdown: impl Future<Output = ()>,
) -> io::Result<()> {
    let stop = CancellationToken::new();
    let incoming = stream::unfold(listener, |listener| async {
        let accepted = listener.accept().await.map(|(stream, _)| stream);
        Some((accepted, listener))
    })
    .take_until(stop.clone().cancelled_owned());
    let running = gateway.run(Box::pin(incoming));
    let reloading = reload(certificates, &config);
    tokio::pin!(running, reloading, shutdown);
    tokio::select! { biased;
        () = &mut shutdown => {
            stop.cancel();
            running.await
        }
        result = &mut running => result,
        () = &mut reloading => Err(io::Error::other("gateway certificate reloader stopped")),
    }
}

/// Retains one timer and one replacement read, with no per-client reload state or payload queue.
async fn reload(certificates: Arc<Certificates>, config: &ListenConfig) {
    let interval = Duration::from_secs(u64::from(RELAY_CERTIFICATE_RELOAD_SECONDS));
    let mut timer = tokio::time::interval_at(tokio::time::Instant::now() + interval, interval);
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        timer.tick().await;
        if certificates.reload(config).await.is_err() {
            tracing::warn!("gateway certificate reload failed; retaining previous certificate");
        }
    }
}

#[cfg(test)]
mod tests;
