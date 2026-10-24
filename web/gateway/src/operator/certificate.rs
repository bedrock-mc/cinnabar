//! Validated immutable TLS replacements shared by the actual listener handshakes.

use super::ListenConfig;
use std::{
    io,
    sync::{Arc, RwLock},
    time::{Duration, SystemTime},
};
use tokio_rustls::rustls::{
    self,
    pki_types::{CertificateDer, PrivateKeyDer, ServerName, pem::PemObject},
    server::{ClientHello, ParsedCertificate, ResolvesServerCert},
    sign::CertifiedKey,
};
use zeroize::Zeroizing;

#[derive(Debug)]
pub(super) struct Certificates {
    current: RwLock<Arc<CertifiedKey>>,
}

impl Certificates {
    /// Requires a complete usable first pair before any listener can bind.
    pub(super) async fn load(config: &ListenConfig) -> io::Result<Self> {
        Ok(Self {
            current: RwLock::new(read(config).await?),
        })
    }

    /// Publishes only after both replacement files and the whole leaf policy have succeeded.
    pub(super) async fn reload(&self, config: &ListenConfig) -> io::Result<()> {
        let replacement = read(config).await?;
        *self.current.write().map_err(|_| refused())? = replacement;
        Ok(())
    }
}

impl ResolvesServerCert for Certificates {
    /// Handshakes borrow no file buffer or lock across TLS work.
    fn resolve(&self, _hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        self.current.read().ok().map(|current| Arc::clone(&current))
    }
}

/// Reads local operator files without including their contents in any diagnostic.
async fn read(config: &ListenConfig) -> io::Result<Arc<CertifiedKey>> {
    let certificates = tokio::fs::read(&config.certificate_file)
        .await
        .map_err(|error| {
            io::Error::new(error.kind(), "gateway certificate file could not be read")
        })?;
    let key = Zeroizing::new(
        tokio::fs::read(&config.key_file)
            .await
            .map_err(|error| io::Error::new(error.kind(), "gateway key file could not be read"))?,
    );
    validated(&certificates, &key, &config.hostname, SystemTime::now())
}

/// Matches the operator's original key/name/half-open validity checks without trust or DNS I/O.
pub(super) fn validated(
    certificates: &[u8],
    key: &[u8],
    hostname: &str,
    now: SystemTime,
) -> io::Result<Arc<CertifiedKey>> {
    let certificates = CertificateDer::pem_slice_iter(certificates)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| refused())?;
    let key = PrivateKeyDer::from_pem_slice(key).map_err(|_| refused())?;
    let certified = CertifiedKey::from_der(
        certificates,
        key,
        &rustls::crypto::aws_lc_rs::default_provider(),
    )
    .map_err(|_| refused())?;
    certified.keys_match().map_err(|_| refused())?;
    let leaf = certified.end_entity_cert().map_err(|_| refused())?;
    let parsed = ParsedCertificate::try_from(leaf).map_err(|_| refused())?;
    let name = ServerName::try_from(hostname).map_err(|_| refused())?;
    rustls::client::verify_server_name(&parsed, &name).map_err(|_| refused())?;
    let (remaining, certificate) =
        x509_parser::parse_x509_certificate(leaf.as_ref()).map_err(|_| refused())?;
    let validity = certificate.validity();
    if !remaining.is_empty()
        || now < timestamp(validity.not_before.timestamp())?
        || now >= timestamp(validity.not_after.timestamp())?
    {
        return Err(refused());
    }
    Ok(Arc::new(certified))
}

/// Converts the parsed timestamp without rounding a caller's sub-second observation.
fn timestamp(seconds: i64) -> io::Result<SystemTime> {
    let duration = Duration::from_secs(seconds.unsigned_abs());
    if seconds >= 0 {
        SystemTime::UNIX_EPOCH.checked_add(duration)
    } else {
        SystemTime::UNIX_EPOCH.checked_sub(duration)
    }
    .ok_or_else(refused)
}

/// A certificate/key parse failure must not reveal local secret bytes or parser input.
fn refused() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "gateway TLS material refused")
}
