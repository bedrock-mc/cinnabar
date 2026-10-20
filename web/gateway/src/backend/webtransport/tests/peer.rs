//! Real Quinn peer retaining the maintained public HTTP/3 client and stream owners.

use super::*;
use bytes::Buf;
use futures::future::poll_fn;
use h3::{
    stream::{BidiStreamHeader, WriteBuf},
    webtransport::SessionId,
};
use std::net::{Ipv4Addr, SocketAddr};
use std::{future::Future, sync::Mutex, task::Poll};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_rustls::rustls::{
    ClientConfig, RootCertStore,
    pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer},
};

type RequestStream = h3::client::RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>;
type Driver = Mutex<h3::client::Connection<h3_quinn::Connection, Bytes>>;

pub(super) struct Fixture {
    pub(super) registry: Arc<Registry>,
    pub(super) tcp: SocketAddr,
    udp: quinn::Endpoint,
    client: quinn::Endpoint,
    task: JoinHandle<io::Result<()>>,
}

impl Fixture {
    /// Supplies one actual certificate, paired listeners, registry and original serving future.
    pub(super) async fn new(upstream: Option<SocketAddr>) -> Self {
        Self::configured(upstream, false, None).await
    }

    /// Limits actual receive credit so a split-prefix test can prove the server consumed a byte.
    pub(super) async fn with_prefix_credit() -> Self {
        Self::configured(None, true, None).await
    }

    /// Bounds the actual peer's stream credit so an archive remains under physical pressure.
    pub(super) async fn with_pack_credit(upstream: SocketAddr, credit: u32) -> Self {
        Self::configured(Some(upstream), false, Some(credit.into())).await
    }

    /// Retains the original paired service and changes only the selected fixture's stream credit.
    async fn configured(
        upstream: Option<SocketAddr>,
        prefix_credit: bool,
        client_credit: Option<quinn::VarInt>,
    ) -> Self {
        if let Some(upstream) = upstream {
            assert!(upstream.ip().is_loopback() && upstream.port() != 0);
        }
        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let certificate = cert.der().clone();
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(signing_key.serialize_der()));
        let gateway =
            super::super::super::Gateway::new(vec![certificate.clone()], key, Vec::new()).unwrap();
        let registry = Arc::clone(&gateway.registry);
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let tcp = listener.local_addr().unwrap();
        let mut config = gateway.quic_config().unwrap();
        if prefix_credit {
            Arc::get_mut(&mut config.transport)
                .expect("fresh gateway transport configuration is uniquely owned")
                .stream_receive_window(1_u32.into());
        }
        let udp = quinn::Endpoint::server(config, (Ipv4Addr::LOCALHOST, 0).into()).unwrap();
        let tcp_incoming = Box::pin(futures::stream::unfold(listener, |listener| async {
            Some((listener.accept().await.map(|(socket, _)| socket), listener))
        }));
        let quic_incoming = Box::pin(futures::stream::unfold(udp.clone(), |endpoint| async {
            endpoint
                .accept()
                .await
                .map(|incoming| (Ok(incoming), endpoint))
        }));
        let task = tokio::spawn(async move {
            let run = gateway.run_paired(tcp_incoming, quic_incoming);
            if let Some(upstream) = upstream {
                crate::upstream::FIXTURE_UPSTREAM.scope(upstream, run).await
            } else {
                run.await
            }
        });
        let mut roots = RootCertStore::empty();
        roots.add(certificate).unwrap();
        let mut tls = ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
        tls.alpn_protocols = vec![b"h3".to_vec()];
        let crypto = quinn::crypto::rustls::QuicClientConfig::try_from(tls).unwrap();
        let mut client = quinn::Endpoint::client((Ipv4Addr::LOCALHOST, 0).into()).unwrap();
        let mut config = quinn::ClientConfig::new(Arc::new(crypto));
        if let Some(credit) = client_credit {
            let mut transport = quinn::TransportConfig::default();
            transport.stream_receive_window(credit);
            config.transport_config(Arc::new(transport));
        }
        client.set_default_client_config(config);
        Self {
            registry,
            tcp,
            udp,
            client,
            task,
        }
    }

    /// Completes actual TLS with the fixture's explicit localhost trust and no resolver.
    pub(super) async fn connect(&self) -> Result<quinn::Connection, quinn::ConnectionError> {
        self.client
            .connect(self.udp.local_addr().unwrap(), "localhost")
            .unwrap()
            .await
    }

    /// Uses the maintained public client builder and retains its original control driver.
    pub(super) async fn peer(&self, path: &str, origin: Option<&str>) -> Peer {
        Peer::from_connection(self.connect().await.unwrap(), path, origin).await
    }

    /// Awaits the original handlers, permit release and service erasure before dropping endpoints.
    pub(super) async fn finish(&mut self) {
        self.registry.cancelled.cancel();
        self.client.close(0_u32.into(), b"");
        tokio::time::timeout(Duration::from_secs(15), &mut self.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(
            crate::registry::tests::retained_counts(&self.registry),
            (0, 0)
        );
        self.udp.close(0_u32.into(), b"");
        self.udp.wait_idle().await;
        self.client.wait_idle().await;
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.registry.cancelled.cancel();
        self.task.abort();
        self.udp.close(0_u32.into(), b"");
        self.client.close(0_u32.into(), b"");
    }
}

pub(crate) struct Peer {
    pub(super) connection: quinn::Connection,
    pub(super) session: SessionId,
    requests: h3::client::SendRequest<h3_quinn::OpenStreams, Bytes>,
    driver: Driver,
    connect: Option<RequestStream>,
}

impl Peer {
    /// Keeps the same public HTTP3 driver on an already verified original operator connection.
    pub(crate) async fn from_connection(
        connection: quinn::Connection,
        path: &str,
        origin: Option<&str>,
    ) -> Self {
        let mut builder = h3::client::builder();
        builder
            .enable_webtransport(true)
            .enable_extended_connect(true)
            .enable_datagram(true)
            .max_webtransport_sessions(1)
            .max_field_section_size(u64::try_from(RELAY_CONTROL_BYTES).unwrap())
            .send_grease(false);
        let (driver, requests) = builder
            .build(h3_quinn::Connection::new(connection.clone()))
            .await
            .unwrap();
        let mut peer = Peer {
            connection,
            requests,
            driver: Mutex::new(driver),
            session: SessionId::try_from(0).unwrap(),
            connect: None,
        };
        let request = peer.request(path, origin).await;
        peer.session = request.id().into();
        peer.connect = Some(request);
        peer
    }

    /// Sends a real CONNECT with the maintained public request/Protocol encoder.
    pub(super) async fn request(&mut self, path: &str, origin: Option<&str>) -> RequestStream {
        let mut request = Request::builder()
            .method(Method::CONNECT)
            .uri(format!("https://localhost{path}"))
            .extension(Protocol::WEB_TRANSPORT);
        if let Some(origin) = origin {
            request = request.header(http::header::ORIGIN, origin);
        }
        drive(
            &self.driver,
            self.requests.send_request(request.body(()).unwrap()),
        )
        .await
        .unwrap()
    }

    /// Observes actual HEADERS on the original CONNECT before using application streams.
    pub(crate) async fn status(&mut self) -> StatusCode {
        response(&self.driver, self.connect.as_mut().unwrap()).await
    }

    /// Decodes the extra request response through that same public client and original driver.
    pub(super) async fn response(&self, stream: &mut RequestStream) -> StatusCode {
        response(&self.driver, stream).await
    }

    /// Produces the maintained protocol preface for this actual CONNECT stream's identity.
    pub(super) fn prefix(&self) -> Bytes {
        let mut bytes = WriteBuf::<Bytes>::from(BidiStreamHeader::WebTransportBidi(self.session));
        bytes.copy_to_bytes(bytes.remaining())
    }

    /// Opens the actual QUIC bidi and prefixes it with the original CONNECT session identity.
    pub(super) async fn stream(&self) -> tokio::io::Join<quinn::RecvStream, quinn::SendStream> {
        let (mut send, recv) = self.connection.open_bi().await.unwrap();
        send.write_all(&self.prefix()).await.unwrap();
        tokio::io::join(recv, send)
    }

    /// Observes one actual CONNECT DATA chunk without completing its original receive stream.
    pub(crate) async fn connect_chunk(&mut self) -> Bytes {
        let stream = self.connect.as_mut().unwrap();
        drive(&self.driver, async {
            let mut chunk = stream
                .recv_data()
                .await
                .unwrap()
                .expect("original CONNECT DATA");
            chunk.copy_to_bytes(chunk.remaining())
        })
        .await
    }

    /// Observes original CONNECT DATA and validated FIN while retaining the same HTTP/3 owner.
    pub(crate) async fn connect_end(&mut self) -> usize {
        let stream = self.connect.as_mut().unwrap();
        drive(&self.driver, async {
            let mut bytes = 0;
            while let Some(chunk) = stream.recv_data().await.unwrap() {
                bytes += chunk.remaining();
            }
            assert!(stream.recv_trailers().await.unwrap().is_none());
            bytes
        })
        .await
    }

    /// Takes the actual WT uni discovered by the original HTTP/3 control/stream driver.
    pub(super) async fn pack_stream(
        &self,
    ) -> h3_webtransport::stream::RecvStream<h3_quinn::RecvStream, Bytes> {
        poll_fn(|cx| {
            let mut driver = self.driver.lock().unwrap();
            if let Poll::Ready(error) = driver.poll_close(cx) {
                panic!("actual HTTP/3 peer retired before its pack stream: {error}");
            }
            match driver.inner.accepted_streams_mut().wt_uni_streams.pop() {
                Some((session, stream)) => {
                    assert_eq!(
                        session, self.session,
                        "archive stays on the actual CONNECT session"
                    );
                    Poll::Ready(h3_webtransport::stream::RecvStream::new(stream))
                }
                None => Poll::Pending,
            }
        })
        .await
    }

    /// Aborts only this actual fixture connection; the original gateway performs application cleanup.
    pub(crate) fn close(&self) {
        self.connection.close(0_u32.into(), b"");
    }
}

/// Decodes actual response headers through the original public client request stream.
async fn response(driver: &Driver, stream: &mut RequestStream) -> StatusCode {
    drive(driver, stream.recv_response())
        .await
        .unwrap()
        .status()
}

/// Drives the same maintained client without a separate task, reader or parser.
async fn drive<T>(driver: &Driver, operation: impl Future<Output = T>) -> T {
    tokio::pin!(operation);
    tokio::select! { biased;
        result = &mut operation => result,
        error = poll_fn(|cx| driver.lock().unwrap().poll_close(cx)) => {
            panic!("actual HTTP/3 peer retired before its operation: {error}");
        }
    }
}
