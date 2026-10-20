//! Actual operator TLS handshakes, unchanged control framing and finite listener shutdown.

use super::*;
use crate::backend::tests::{connect_control, event};
use bridge::relay_control::{Event, Request};
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use std::{
    net::Ipv4Addr,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{net::TcpStream, sync::oneshot, task::JoinHandle};
use tokio_rustls::{
    TlsConnector,
    client::TlsStream,
    rustls::{
        self, ClientConfig, RootCertStore,
        pki_types::{CertificateDer, ServerName},
    },
};

struct Pair {
    certificate: CertificateDer<'static>,
    pem: String,
    key: String,
    not_before: SystemTime,
    not_after: SystemTime,
}

impl Pair {
    /// Creates a local-only certificate with a bounded, explicit validity interval.
    fn new(hostname: &str) -> Self {
        let parameters = rcgen::CertificateParams::new(vec![hostname.to_owned()]).unwrap();
        let not_before = UNIX_EPOCH
            + Duration::from_secs(u64::try_from(parameters.not_before.unix_timestamp()).unwrap());
        let not_after = UNIX_EPOCH
            + Duration::from_secs(u64::try_from(parameters.not_after.unix_timestamp()).unwrap());
        let key = rcgen::KeyPair::generate().unwrap();
        let certificate = parameters.self_signed(&key).unwrap();
        Self {
            certificate: certificate.der().clone(),
            pem: certificate.pem(),
            key: key.serialize_pem(),
            not_before,
            not_after,
        }
    }
}

struct Files(PathBuf);

impl Files {
    /// Owns only this fixture's newly created certificate directory, with no global env mutation.
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "cinnabar-gateway-operator-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    /// Writes one complete synthetic pair before asking the actual reload owner to publish it.
    fn write(&self, pair: &Pair) {
        std::fs::write(self.0.join("certificate.pem"), &pair.pem).unwrap();
        std::fs::write(self.0.join("key.pem"), &pair.key).unwrap();
    }

    /// Supplies real file paths and the same listener/origin inputs as the operator binary.
    fn config(&self, tcp_address: SocketAddr) -> ListenConfig {
        ListenConfig {
            hostname: "localhost".into(),
            tcp_address,
            certificate_file: self.0.join("certificate.pem"),
            key_file: self.0.join("key.pem"),
            origins: Vec::new(),
        }
    }
}

impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Trusts the same two synthetic pairs and disables resumption for fresh certificate observations.
fn client_tls(first: &Pair, second: &Pair) -> ClientConfig {
    let mut roots = RootCertStore::empty();
    roots.add(first.certificate.clone()).unwrap();
    roots.add(second.certificate.clone()).unwrap();
    let mut client = ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    client.alpn_protocols = vec![b"http/1.1".to_vec()];
    // A resumed TLS session intentionally carries its original certificate observation.
    client.resumption = rustls::client::Resumption::disabled();
    client
}

struct Fixture {
    task: Option<JoinHandle<io::Result<()>>>,
    stop: Option<oneshot::Sender<()>>,
    files: Files,
    certificates: Arc<Certificates>,
    address: SocketAddr,
    connector: TlsConnector,
}

impl Fixture {
    /// Runs the production operator loop with actual files, TLS and the original Gateway owner.
    async fn new(first: &Pair, second: &Pair) -> Self {
        let files = Files::new();
        files.write(first);
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let config = files.config(address);
        let certificates = Arc::new(Certificates::load(&config).await.unwrap());
        let gateway = Gateway::with_resolver(certificates.clone(), Vec::new()).unwrap();
        let (stop, stopped) = oneshot::channel();
        let serving_certificates = Arc::clone(&certificates);
        let task = tokio::spawn(serve(
            config,
            serving_certificates,
            gateway,
            listener,
            async move {
                let _ = stopped.await;
            },
        ));
        let client = client_tls(first, second);
        Self {
            task: Some(task),
            stop: Some(stop),
            files,
            certificates,
            address,
            connector: TlsConnector::from(Arc::new(client)),
        }
    }

    /// Observes the certificate actually selected by a fresh verified loopback handshake.
    async fn secure(&self) -> (TlsStream<TcpStream>, CertificateDer<'static>) {
        let socket = TcpStream::connect(self.address).await.unwrap();
        let stream = self
            .connector
            .connect(ServerName::try_from("localhost").unwrap(), socket)
            .await
            .unwrap();
        let certificate = stream.get_ref().1.peer_certificates().unwrap()[0].clone();
        (stream, certificate)
    }

    /// Exercises the same complete-file validation and publication owner used by the timer.
    async fn reload(&self) -> io::Result<()> {
        self.certificates
            .reload(&self.files.config(self.address))
            .await
    }

    /// Awaits the real operator's shutdown path before dropping the certificate or file owners.
    async fn finish(&mut self) {
        self.stop.take().unwrap().send(()).unwrap();
        self.task.take().unwrap().await.unwrap().unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.take();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

/// Valid reloads affect new TLS only; rejected file replacements cannot replace a good pair.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn operator_reload_preserves_control_and_retains_last_valid_pair() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let first = Pair::new("localhost");
        let second = Pair::new("localhost");
        let mut fixture = Fixture::new(&first, &second).await;
        let (secure, selected_first) = fixture.secure().await;
        let (mut control, original) = connect_control(secure, fixture.address, String::new()).await;

        fixture.files.write(&second);
        fixture.reload().await.unwrap();
        let (secure, selected_second) = fixture.secure().await;
        drop(secure);

        let foreign = Pair::new("other.invalid");
        fixture.files.write(&foreign);
        let foreign_rejected = fixture.reload().await.is_err();
        let (secure, after_foreign) = fixture.secure().await;
        drop(secure);

        fixture.files.write(&second);
        std::fs::write(fixture.files.0.join("key.pem"), &first.key).unwrap();
        let mismatch_rejected = fixture.reload().await.is_err();
        let (secure, after_mismatch) = fixture.secure().await;
        drop(secure);

        std::fs::write(
            fixture.files.0.join("certificate.pem"),
            b"not a certificate",
        )
        .unwrap();
        let malformed_rejected = fixture.reload().await.is_err();
        let (secure, after_malformed) = fixture.secure().await;
        drop(secure);

        control
            .send(Bytes::from(serde_json::to_vec(&Request::SignOut).unwrap()))
            .await
            .unwrap();
        let control_progressed = matches!(event(&mut control).await, Event::Account { .. });
        fixture.finish().await;
        let retired = !matches!(control.next().await, Some(Ok(_)));
        drop(control);

        assert_eq!(selected_first, first.certificate);
        assert_eq!(selected_second, second.certificate);
        assert!(foreign_rejected && mismatch_rejected && malformed_rejected);
        assert_eq!(after_foreign, second.certificate);
        assert_eq!(after_mismatch, second.certificate);
        assert_eq!(after_malformed, second.certificate);
        assert!(control_progressed && retired && original.generation != 0);
    })
    .await
    .unwrap();
}

/// Pure validity uses the caller's exact observation, including the original exclusive NotAfter.
#[test]
fn operator_certificate_validity_is_half_open_and_key_name_checked() {
    let pair = Pair::new("localhost");
    let other = Pair::new("localhost");
    let before = pair.not_before;
    let after = pair.not_after;
    let validate = |now, hostname: &str, key: &str| {
        certificate::validated(pair.pem.as_bytes(), key.as_bytes(), hostname, now)
    };
    assert!(validate(before, "localhost", &pair.key).is_ok());
    assert!(validate(after - Duration::from_nanos(1), "localhost", &pair.key).is_ok());
    for now in [before - Duration::from_nanos(1), after] {
        assert!(validate(now, "localhost", &pair.key).is_err());
    }
    assert!(validate(before, "other.invalid", &pair.key).is_err());
    assert!(validate(before, "localhost", &other.key).is_err());
}

/// Invalid first material wins over a real occupied bind and cannot open a serving socket.
#[tokio::test]
async fn operator_refuses_invalid_initial_material_before_binding() {
    let files = Files::new();
    let pair = Pair::new("localhost");
    files.write(&pair);
    std::fs::write(files.0.join("key.pem"), b"not a private key").unwrap();
    let occupied = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = occupied.local_addr().unwrap();
    let error = run(files.config(address), std::future::pending())
        .await
        .unwrap_err();
    drop(occupied);
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(!error.to_string().contains("not a private key"));
    assert!(!error.to_string().contains(files.0.to_str().unwrap()));
}

#[cfg(feature = "webtransport")]
mod paired;
