//! The original paired operator owns both TLS edges, reload publication and account retirement.

use super::super::webtransport::{Endpoint, serve_paired};
use super::*;
use crate::backend::tests::gateway_registry;
use crate::backend::webtransport::tests::{
    control as paired_control, event as paired_event, peer::Peer as PairedPeer,
};
use bridge::contract::RELAY_TRANSPORT_PATH;
use http::StatusCode;

struct PairedFixture {
    task: Option<JoinHandle<io::Result<()>>>,
    stop: Option<oneshot::Sender<()>>,
    files: Files,
    certificates: Arc<Certificates>,
    registry: Arc<crate::registry::Registry>,
    tcp: SocketAddr,
    udp: SocketAddr,
    connector: TlsConnector,
    client: quinn::Endpoint,
}

impl PairedFixture {
    /// Binds actual tuples, then passes the original guarded sockets to the production paired owner.
    async fn new(first: &Pair, second: &Pair) -> Self {
        let files = Files::new();
        files.write(first);
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let tcp = listener.local_addr().unwrap();
        let config = files.config(tcp);
        let certificates = Arc::new(Certificates::load(&config).await.unwrap());
        let gateway = Gateway::with_resolver(certificates.clone(), config.origins.clone()).unwrap();
        let registry = gateway_registry(&gateway);
        let endpoint = quinn::Endpoint::server(
            gateway.quic_config().unwrap(),
            (Ipv4Addr::LOCALHOST, 0).into(),
        )
        .unwrap();
        let udp = endpoint.local_addr().unwrap();
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(serve_paired(
            config,
            Arc::clone(&certificates),
            gateway,
            listener,
            Endpoint(endpoint),
            async move {
                let _ = stopped.await;
            },
        ));
        let mut tls = client_tls(first, second);
        let connector = TlsConnector::from(Arc::new(tls.clone()));
        tls.alpn_protocols = vec![b"h3".to_vec()];
        let crypto = quinn::crypto::rustls::QuicClientConfig::try_from(tls).unwrap();
        let mut quic = quinn::ClientConfig::new(Arc::new(crypto));
        let mut transport = quinn::TransportConfig::default();
        transport.stream_receive_window(1_u32.into());
        quic.transport_config(Arc::new(transport));
        let mut client = quinn::Endpoint::client((Ipv4Addr::LOCALHOST, 0).into()).unwrap();
        client.set_default_client_config(quic);
        Self {
            task: Some(task),
            stop: Some(stop),
            files,
            certificates,
            registry,
            tcp,
            udp,
            connector,
            client,
        }
    }

    /// Observes the certificate selected by an actual fresh verified TCP handshake.
    async fn secure(&self) -> (TlsStream<TcpStream>, CertificateDer<'static>) {
        let socket = TcpStream::connect(self.tcp).await.unwrap();
        let stream = self
            .connector
            .connect(ServerName::try_from("localhost").unwrap(), socket)
            .await
            .unwrap();
        let certificate = stream.get_ref().1.peer_certificates().unwrap()[0].clone();
        (stream, certificate)
    }

    /// Observes the same resolver through Quinn's actual rustls peer identity on a fresh handshake.
    async fn quic(&self) -> (quinn::Connection, CertificateDer<'static>) {
        let connection = self
            .client
            .connect(self.udp, "localhost")
            .unwrap()
            .await
            .unwrap();
        let identity = connection
            .peer_identity()
            .expect("verified rustls peer identity");
        let certificates = match identity.downcast::<Vec<CertificateDer<'static>>>() {
            Ok(certificates) => certificates,
            Err(_) => panic!("actual rustls certificate chain"),
        };
        let certificate = certificates[0].clone();
        (connection, certificate)
    }

    /// Publishes through the exact complete-file reload owner used by the operator timer.
    async fn reload(&self) -> io::Result<()> {
        self.certificates.reload(&self.files.config(self.tcp)).await
    }

    /// Completes both actual handshake probes without creating replacement account sessions.
    async fn observed(&self) -> (CertificateDer<'static>, CertificateDer<'static>) {
        let (tcp, first) = self.secure().await;
        drop(tcp);
        let (quic, second) = self.quic().await;
        quic.close(0_u32.into(), b"");
        quic.closed().await;
        (first, second)
    }

    /// Signals only the existing production shutdown future, leaving original retirement to it.
    fn stop(&mut self) {
        self.stop.take().unwrap().send(()).unwrap();
    }

    /// Joins the same serving owner before dropping its client endpoint and fixture files.
    async fn finish(&mut self) {
        self.task.take().unwrap().await.unwrap().unwrap();
        assert_eq!(
            crate::registry::tests::retained_counts(&self.registry),
            (0, 0)
        );
        self.client.close(0_u32.into(), b"");
        self.client.wait_idle().await;
    }
}

impl Drop for PairedFixture {
    fn drop(&mut self) {
        self.stop.take();
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.client.close(0_u32.into(), b"");
    }
}

/// Each fresh original operator remains within its unchanged per-IP admission burst.
#[derive(Clone, Copy, Debug)]
enum RejectedReplacement {
    ForeignName,
    MismatchedKey,
    MalformedCertificate,
}

/// Reloads both actual protocols, then keeps accounts until pressured application close can finish.
#[tokio::test]
async fn paired_operator_reloads_both_tls_edges_and_retires_handlers_before_accounts() {
    for rejected in [
        RejectedReplacement::ForeignName,
        RejectedReplacement::MismatchedKey,
        RejectedReplacement::MalformedCertificate,
    ] {
        tokio::time::timeout(Duration::from_secs(30), async {
            let first = Pair::new("localhost");
            let second = Pair::new("localhost");
            let mut fixture = PairedFixture::new(&first, &second).await;
            let (secure, tcp_first) = fixture.secure().await;
            let (mut tcp_control, tcp_capability) =
                connect_control(secure, fixture.tcp, String::new()).await;
            let (connection, quic_first) = fixture.quic().await;
            let mut peer =
                PairedPeer::from_connection(connection, RELAY_TRANSPORT_PATH, None).await;
            assert_eq!(peer.status().await, StatusCode::OK);
            let (mut quic_control, quic_capability) = paired_control(&peer).await;
            assert!(
                tcp_capability.resume != quic_capability.resume,
                "separate actual controllers must retain separate session capabilities"
            );
            assert_ne!(tcp_capability.generation, 0);
            assert_ne!(quic_capability.generation, 0);
            assert_eq!(
                crate::registry::tests::retained_counts(&fixture.registry).1,
                2
            );

            fixture.files.write(&second);
            fixture.reload().await.unwrap();
            let replacement = fixture.observed().await;

            match rejected {
                RejectedReplacement::ForeignName => {
                    fixture.files.write(&Pair::new("other.invalid"));
                }
                RejectedReplacement::MismatchedKey => {
                    fixture.files.write(&second);
                    std::fs::write(fixture.files.0.join("key.pem"), &first.key).unwrap();
                }
                RejectedReplacement::MalformedCertificate => {
                    std::fs::write(
                        fixture.files.0.join("certificate.pem"),
                        b"not a certificate",
                    )
                    .unwrap();
                }
            }
            let rejected_replacement = fixture.reload().await.is_err();
            let after_rejected = fixture.observed().await;

            let sign_out = Bytes::from(serde_json::to_vec(&Request::SignOut).unwrap());
            tcp_control.send(sign_out.clone()).await.unwrap();
            quic_control.send(sign_out).await.unwrap();
            assert!(matches!(
                event(&mut tcp_control).await,
                Event::Account { .. }
            ));
            assert!(matches!(
                paired_event(&mut quic_control).await,
                Event::Account { .. }
            ));
            fixture.stop();

            // One-byte actual credit exposes local close pressure without assuming physical-close policy.
            let prefix = peer.connect_chunk().await;
            assert_eq!(prefix.len(), 1);
            assert!(prefix.len() + 1 < size_of::<u32>());
            assert_eq!(
                crate::registry::tests::retained_counts(&fixture.registry).1,
                2
            );
            assert!(!fixture.task.as_ref().unwrap().is_finished());
            let remainder = peer.connect_end().await;
            assert!(prefix.len() + remainder >= size_of::<u32>());
            let tcp_retired = !matches!(tcp_control.next().await, Some(Ok(_)));
            let quic_retired = !matches!(quic_control.next().await, Some(Ok(_)));
            peer.close();
            fixture.finish().await;
            drop((tcp_control, quic_control));

            assert_eq!(tcp_first, first.certificate);
            assert_eq!(quic_first, first.certificate);
            assert!(
                rejected_replacement,
                "replacement was accepted: {rejected:?}"
            );
            for (tcp, quic) in [replacement, after_rejected] {
                assert_eq!(tcp, second.certificate);
                assert_eq!(quic, second.certificate);
            }
            assert!(tcp_retired && quic_retired);
        })
        .await
        .expect("actual paired operator reload/retirement stalled");
    }
}
