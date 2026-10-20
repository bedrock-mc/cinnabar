# Native browser gateway

`web-gateway` serves browser control and play streams through the native Rust networking stack.
[`Gateway`] owns ephemeral account sessions and their connections. Your application supplies
certificates, an accepted TCP stream and a Tokio runtime with I/O and timers enabled.

```rust,no_run
use futures::stream;
use std::io;
use tokio::net::TcpListener;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use web_gateway::Gateway;

async fn serve(
    listener: TcpListener,
    certificates: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
) -> io::Result<()> {
    let gateway = Gateway::new(certificates, key, Vec::new())?;
    let incoming = Box::pin(stream::unfold(listener, |listener| async move {
        let accepted = listener.accept().await.map(|(stream, _)| stream);
        Some((accepted, listener))
    }));
    gateway.run(incoming).await
}
```

The gateway accepts TLS 1.3 HTTP/1.1 upgrades for the generated
[`control`](bridge::contract::RELAY_CONTROL_PATH) and
[`play`](bridge::contract::RELAY_PLAY_PATH) routes with the
[shared WebSocket subprotocol](bridge::contract::RELAY_SOCKET_PROTOCOL).
A control connection issues the capability and generation used by its separate play connection;
play runs native RakNet login, resource-pack transfer and the shared web-session handoff.
Upstream connections use the existing public-address admission policy.
Advertised pack HTTP/HTTPS URLs use the same public numeric address policy at DNS resolution
and before following redirects, with ambient proxies disabled. Verified archives enter the existing
pack handoff; URL failures retain game-chunk fallback. This path uses no desktop filesystem cache.

`Gateway::new` takes exact allowed origin strings. With an empty list, a nonempty Origin must
have the request Host's authority; missing Origin is permitted. Origin checks do not replace
the control capability or account checks.

End the accepted stream and await `Gateway::run` to stop admission and retire active connections
before their account state is erased. Dropping the run future drops the handlers immediately;
it does not await protocol retirement. Individual connection refusals are contained, while
accepted-stream and service-driver errors are returned to the application.

This crate is a native Tokio edge. A Bevy application can drive it on its existing I/O runtime;
it does not need a runtime per connection. Browser clients use ordinary WebSocket connections.
The default library creates no listener or runtime.

The opt-in native operator supplies the actual HTTPS listener, signal handling and one process
runtime:

```sh
cargo run -p web-gateway --features operator --bin bedrock-web-relay -- \
  --hostname relay.example --tcp 127.0.0.1:8443 \
  --tls-cert /path/to/fullchain.pem --tls-key /path/to/key.pem
```

The certificate must cover `--hostname` and be within its validity interval. The key must match
the certificate and be unencrypted PEM. The operator checks both files before binding and retries
them at the [shared reload interval](bridge::contract::RELAY_CERTIFICATE_RELOAD_SECONDS).
A valid replacement affects new handshakes; a failed replacement retains the last valid pair
and active control/play connections continue. `--origins` accepts comma-separated exact origins;
omitting it preserves the same-authority policy. SIGINT or SIGTERM stops admission and awaits
the existing gateway retirement before the runtime is dropped.

The additional `webtransport` feature supplies `Gateway::quic_config` and
`Gateway::run_paired` for caller-owned Quinn incoming connections beside the accepted TCP
stream. Both use the same registry, account service and physical connection limits. QUIC is
admitted before TLS, and one accepted-time deadline spans TLS, CONNECT, stream prefix and
control Hello. Each QUIC connection accepts one WebTransport session at the generated route;
its later play streams use that exact controller and generation. Client application uni streams
and additional CONNECT requests are refused. HTTP/3 control progress continues while a stream
prefix or play is blocked.

For the file-backed operator, enable both features and add the UDP bind:

```sh
cargo run -p web-gateway --features operator,webtransport --bin bedrock-web-relay -- \
  --hostname relay.example --tcp 127.0.0.1:8443 --udp 127.0.0.1:8443 \
  --tls-cert /path/to/fullchain.pem --tls-key /path/to/key.pem
```

The paired listeners share certificate replacements and shutdown. No account, runtime or play
pump is added per transport. Each play retains the original native connection, validated client
data, account lease and incoming FIFO. WebTransport sends its already acquired pack archives
on one server uni stream with the original join prefix, framed PackData and FIN. The archive
writer runs beside the same play pump and performs no second fetch. Once opened, cancellation
resets its writer. Partial WebTransport opener cancellation remains a dependency qualification
gate. WebSocket archive delivery remains serial on its existing play connection.

Application close first settles the original CONNECT capsule write and FIN, retaining its actual
HTTP/3 owner through the finite physical-close stage. Local flush/FIN is not a remote game or
browser acknowledgement. Dropping a handler remains abortive. These source paths require
qualification against their coherent h3 dependency and actual paired/BDS controls. Retail browser,
latest WebTransport draft, deployment and live-account acceptance remain open; WebSocket
fallback does not close those gates.
