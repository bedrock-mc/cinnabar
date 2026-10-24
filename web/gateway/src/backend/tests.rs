//! Real accepted TCP/TLS/WebSocket owners with explicit fixture trust and finite task ownership.

use super::Gateway;
use super::*;
use bridge::{
    contract::*,
    relay_control::{Event, Hello, Request},
};
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use http::{
    HeaderValue,
    header::{ORIGIN, SEC_WEBSOCKET_PROTOCOL},
};
use std::net::{Ipv4Addr, SocketAddr};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};
use tokio_rustls::{
    TlsConnector,
    client::TlsStream,
    rustls::{
        ClientConfig, RootCertStore,
        pki_types::{PrivatePkcs8KeyDer, ServerName},
    },
};
use tokio_util::codec::Framed;
use tokio_websockets::ClientBuilder;

/// Borrows only the original gateway's registry before its serving owner takes the gateway.
#[cfg(all(feature = "operator", feature = "webtransport"))]
pub(crate) fn gateway_registry(gateway: &Gateway) -> Arc<Registry> {
    Arc::clone(&gateway.registry)
}

const LIMIT: usize = RELAY_CONTROL_BYTES + size_of::<u32>();
pub(crate) type Client = Framed<Socket<TlsStream<TcpStream>>, bridge::BridgeCodec>;

struct Fixture {
    task: JoinHandle<io::Result<()>>,
    registry: Arc<Registry>,
    address: SocketAddr,
    tls: TlsConnector,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.registry.cancelled.cancel();
        self.task.abort();
    }
}

impl Fixture {
    /// Runs the actual gateway on one explicitly trusted localhost certificate and socket.
    async fn new() -> Self {
        Self::with_upstream(None).await
    }

    /// Scopes an optional exact loopback fixture to this gateway's actual serving future.
    async fn with_upstream(upstream: Option<SocketAddr>) -> Self {
        if let Some(address) = upstream {
            assert!(address.ip().is_loopback() && address.port() != 0);
        }
        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let certificate = cert.der().clone();
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(signing_key.serialize_der()));
        let gateway = Gateway::new(vec![certificate.clone()], key, Vec::new()).unwrap();
        let registry = Arc::clone(&gateway.registry);
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let incoming = Box::pin(futures::stream::unfold(listener, |listener| async move {
            let connection = listener.accept().await.map(|(stream, _)| stream);
            Some((connection, listener))
        }));
        let task = tokio::spawn(async move {
            if let Some(address) = upstream {
                crate::upstream::FIXTURE_UPSTREAM
                    .scope(address, gateway.run(incoming))
                    .await
            } else {
                gateway.run(incoming).await
            }
        });
        let mut roots = RootCertStore::empty();
        roots.add(certificate).unwrap();
        let mut config = ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Self {
            task,
            registry,
            address,
            tls: TlsConnector::from(Arc::new(config)),
        }
    }

    /// Connects to the actual fixture socket with a fixed peer name and no resolver/global trust.
    async fn secure(&self) -> TlsStream<TcpStream> {
        let tcp = TcpStream::connect(self.address).await.unwrap();
        let tls = self
            .tls
            .connect(ServerName::try_from("localhost").unwrap(), tcp)
            .await
            .unwrap();
        assert_eq!(
            tls.get_ref().1.protocol_version(),
            Some(rustls::ProtocolVersion::TLSv1_3)
        );
        assert_eq!(tls.get_ref().1.alpn_protocol(), Some(&b"http/1.1"[..]));
        tls
    }

    /// Uses the maintained client codec, exact subprotocol and real framed Hello/control messages.
    async fn control(&self, resume: String) -> (Client, bridge::relay_control::Capability) {
        let tls = self.secure().await;
        connect_control(tls, self.address, resume).await
    }

    /// Joins the actual backend after it has retired every stream and erased its session registry.
    async fn finish(&mut self) {
        self.registry.cancelled.cancel();
        tokio::time::timeout(Duration::from_secs(5), &mut self.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(
            crate::registry::tests::retained_counts(&self.registry),
            (0, 0)
        );
    }
}

/// Uses the same real upgrade/Hello owner for supplied and file-backed TLS fixture certificates.
pub(crate) async fn connect_control(
    tls: TlsStream<TcpStream>,
    address: SocketAddr,
    resume: String,
) -> (Client, bridge::relay_control::Capability) {
    let uri = format!("wss://{address}{RELAY_CONTROL_PATH}")
        .parse()
        .unwrap();
    let builder = ClientBuilder::from_uri(uri)
        .add_header(
            SEC_WEBSOCKET_PROTOCOL,
            HeaderValue::from_static(RELAY_SOCKET_PROTOCOL),
        )
        .unwrap()
        .add_header(
            ORIGIN,
            HeaderValue::from_str(&format!("https://{address}")).unwrap(),
        )
        .unwrap();
    let (socket, response) = builder.connect_on(tls).await.unwrap();
    assert_eq!(
        response.headers().get(SEC_WEBSOCKET_PROTOCOL).unwrap(),
        RELAY_SOCKET_PROTOCOL
    );
    let mut client = Framed::new(
        Socket::new(socket, LIMIT),
        bridge::BridgeCodec::with_max(RELAY_CONTROL_BYTES),
    );
    client
        .send(Bytes::from(
            serde_json::to_vec(&Hello {
                version: RELAY_VERSION,
                channel: RELAY_CHANNEL_CONTROL.into(),
                channels: vec![RELAY_CHANNEL_CONTROL.into(), RELAY_CHANNEL_PLAY.into()],
                resume,
                generation: 0,
                join: 0,
            })
            .unwrap(),
        ))
        .await
        .unwrap();
    let Event::Welcome { capability } = event(&mut client).await else {
        panic!("actual control Welcome")
    };
    assert!(matches!(event(&mut client).await, Event::Account { .. }));
    (client, capability)
}

/// Decodes one actual binary control frame rather than reconstructing a capability fixture.
pub(crate) async fn event(client: &mut Client) -> Event {
    serde_json::from_slice(&client.next().await.unwrap().unwrap()).unwrap()
}

/// Resume retires only the original controller, and EOF on that old socket cannot cancel the new one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accepted_tls_control_preserves_resume_and_eof_ownership() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut fixture = Fixture::new().await;
        let (first, original) = fixture.control(String::new()).await;
        let original_play = fixture
            .registry
            .open_play(&original.resume, original.generation, Instant::now())
            .unwrap();
        let (mut replacement, current) = fixture.control(original.resume).await;
        original_play.cancel.cancelled().await;
        assert!(current.generation > original.generation);
        let current_play = fixture
            .registry
            .open_play(&current.resume, current.generation, Instant::now())
            .unwrap();
        drop(first);
        drop(original_play);
        assert!(!current_play.cancel.is_cancelled());
        replacement
            .send(Bytes::from(serde_json::to_vec(&Request::SignOut).unwrap()))
            .await
            .unwrap();
        assert!(matches!(
            event(&mut replacement).await,
            Event::Account { .. }
        ));
        replacement.close().await.unwrap();
        drop(replacement);
        current_play.cancel.cancelled().await;
        drop(current_play);
        fixture.finish().await;
    })
    .await
    .unwrap();
}

/// Capacity is charged to the actual accepted peer before even TLS and released on shutdown.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accepted_socket_capacity_precedes_tls_and_retires_on_cancel() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut fixture = Fixture::new().await;
        let mut held = Vec::new();
        for _ in 0..RELAY_MAX_PER_IP {
            held.push(TcpStream::connect(fixture.address).await.unwrap());
        }
        test_time::eventually("actual pre-TLS accepted peer capacity", || {
            crate::registry::tests::retained_counts(&fixture.registry).0 == RELAY_MAX_PER_IP
        });
        let mut extra = TcpStream::connect(fixture.address).await.unwrap();
        let mut byte = [0];
        assert!(matches!(extra.read(&mut byte).await, Ok(0) | Err(_)));
        assert_eq!(
            crate::registry::tests::retained_counts(&fixture.registry),
            (RELAY_MAX_PER_IP, 0)
        );
        fixture.finish().await;
        for mut stream in held {
            assert!(matches!(stream.read(&mut byte).await, Ok(0) | Err(_)));
        }
    })
    .await
    .unwrap();
}

/// Real TLS refusals precede101 for forbidden origins, missing protocol and unknown paths.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn origin_subprotocol_and_unknown_paths_are_checked_before_upgrade() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut fixture = Fixture::new().await;
        let unknown_path = format!("{RELAY_PLAY_PATH}/unknown");
        for (path, extra, expected) in [
            (RELAY_CONTROL_PATH, format!("Origin: https://foreign.example\r\nSec-WebSocket-Protocol: {RELAY_SOCKET_PROTOCOL}\r\n"), "403 Forbidden"),
            (RELAY_CONTROL_PATH, String::new(), "400 Bad Request"),
            (unknown_path.as_str(), format!("Sec-WebSocket-Protocol: {RELAY_SOCKET_PROTOCOL}\r\n"), "404 Not Found"),
            (RELAY_CONTROL_PATH, format!("Origin: https://{}\r\nOrigin: https://foreign.example\r\nSec-WebSocket-Protocol: {RELAY_SOCKET_PROTOCOL}\r\n", fixture.address), "400 Bad Request"),
        ] {
            let mut tls = fixture.secure().await;
            let request = format!("GET {path} HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n{extra}\r\n", fixture.address);
            tls.write_all(request.as_bytes()).await.unwrap();
            let mut response = Vec::new();
            // The server drops the refused TLS owner; rustls may report missing close_notify after
            // yielding the actual HTTP prefix, which is sufficient to observe pre-upgrade refusal.
            let _ = tls.read_to_end(&mut response).await;
            let response = String::from_utf8(response).unwrap();
            assert!(response.starts_with(&format!("HTTP/1.1 {expected}\r\n")), "{response:?}");
            assert!(!response.contains("101 Switching Protocols"));
        }
        fixture.finish().await;
    }).await.unwrap();
}

/// Completing the actual service future must not retire a still-owned accepted control socket.
#[tokio::test]
async fn completed_service_driver_retains_sessions_until_connections_retire() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let registry = Registry::new();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (stream, _) = listener.accept().await.unwrap();
        let peer = stream.peer_addr().unwrap();
        let permit = registry.admit(peer.ip(), Instant::now()).unwrap();
        let admitted = Admitted {
            stream,
            _permit: permit,
        };
        let serving = Arc::clone(&registry);
        let mut connections = FuturesUnordered::<Work>::new();
        connections.push(Box::pin(async move {
            control::run(&serving, peer.ip(), admitted).await
        }));
        let mut services = Box::pin(registry.drive());
        assert!(futures::poll!(services.as_mut()).is_pending());
        let mut client = Framed::new(
            client,
            bridge::BridgeCodec::with_max(RELAY_CONTROL_BYTES),
        );
        {
            let hello = async {
                client
                    .send(Bytes::from(
                        serde_json::to_vec(&Hello {
                            version: RELAY_VERSION,
                            channel: RELAY_CHANNEL_CONTROL.into(),
                            channels: vec![
                                RELAY_CHANNEL_CONTROL.into(),
                                RELAY_CHANNEL_PLAY.into(),
                            ],
                            resume: String::new(),
                            generation: 0,
                            join: 0,
                        })
                        .unwrap(),
                    ))
                    .await
                    .unwrap();
                let response = client.next().await.unwrap().unwrap();
                assert!(matches!(
                    serde_json::from_slice::<Event>(&response).unwrap(),
                    Event::Welcome { .. }
                ));
            };
            tokio::pin!(hello);
            tokio::select! {
                () = &mut hello => {},
                result = connections.next() => panic!("the actual control owner retired: {result:?}"),
                result = services.as_mut() => panic!("the actual registry driver retired: {result:?}"),
            }
        }
        assert_eq!(crate::registry::tests::retained_counts(&registry), (1, 1));
        assert_eq!(connections.len(), 1);
        let cancellation = registry.cancelled.cancelled();
        tokio::pin!(cancellation);
        assert!(futures::poll!(cancellation.as_mut()).is_pending());
        registry.cancelled.cancel();
        services.as_mut().await.unwrap();
        let before_connection_retirement = crate::registry::tests::retained_counts(&registry);
        assert_eq!(connections.len(), 1);
        drop(connections);
        drop(services);
        drop(client);
        assert_eq!(crate::registry::tests::retained_counts(&registry), (0, 0));
        assert_eq!(
            before_connection_retirement,
            (1, 1),
            "service completion must retain sessions until actual sockets retire"
        );
    })
    .await
    .unwrap();
}

mod channel;
mod play;
mod world;
