//! Paired TCP/QUIC listeners share the original certificate, registry and shutdown owner.

use super::{Certificates, ListenConfig, reload};
use crate::Gateway;
use futures::{StreamExt, stream};
use std::{future::Future, io, net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

/// Runs the optional native WebTransport listener beside the existing HTTPS fallback.
///
/// Both binds finish before admission. Certificate replacements feed the same resolver and only
/// new handshakes. Shutdown retires the original application handlers before account erasure;
/// dropping the future remains abortive. The caller supplies the only Tokio runtime.
pub async fn run_paired(
    config: ListenConfig,
    udp_address: SocketAddr,
    shutdown: impl Future<Output = ()>,
) -> io::Result<()> {
    let certificates = Arc::new(Certificates::load(&config).await?);
    let gateway = Gateway::with_resolver(certificates.clone(), config.origins.clone())?;
    let listener = TcpListener::bind(config.tcp_address).await?;
    let endpoint = Endpoint(quinn::Endpoint::server(
        gateway.quic_config()?,
        udp_address,
    )?);
    serve_paired(config, certificates, gateway, listener, endpoint, shutdown).await
}

/// Drives the original paired listeners, resolver and retained endpoint through joint retirement.
pub(super) async fn serve_paired(
    config: ListenConfig,
    certificates: Arc<Certificates>,
    gateway: Gateway,
    listener: TcpListener,
    endpoint: Endpoint,
    shutdown: impl Future<Output = ()>,
) -> io::Result<()> {
    let stop = CancellationToken::new();
    let tcp = stream::unfold(listener, |listener| async {
        Some((listener.accept().await.map(|(stream, _)| stream), listener))
    })
    .take_until(stop.clone().cancelled_owned());
    let quic = stream::unfold(endpoint.0.clone(), |endpoint| async {
        endpoint
            .accept()
            .await
            .map(|incoming| (Ok(incoming), endpoint))
    })
    .take_until(stop.clone().cancelled_owned());
    let running = gateway.run_paired(Box::pin(tcp), Box::pin(quic));
    let reloading = reload(certificates, &config);
    tokio::pin!(running, reloading, shutdown);
    let result = tokio::select! { biased;
        () = &mut shutdown => {
            endpoint.0.set_server_config(None);
            stop.cancel();
            running.await
        }
        result = &mut running => result,
        () = &mut reloading => {
            endpoint.0.set_server_config(None);
            stop.cancel();
            let _ = running.await;
            Err(io::Error::other("gateway certificate reloader stopped"))
        }
    };
    endpoint.0.close(0_u32.into(), b"");
    endpoint.0.wait_idle().await;
    result
}

/// Retires the original UDP endpoint on cancellation while normal shutdown awaits its drain.
pub(super) struct Endpoint(pub(super) quinn::Endpoint);
impl Drop for Endpoint {
    fn drop(&mut self) {
        self.0.close(0_u32.into(), b"");
    }
}
