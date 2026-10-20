//! One verified guest's original local BDS upstream, without another runtime or world process.

mod startup;
#[cfg(test)]
mod tests;
pub use startup::{NativeHostedRelay, NativeHostedRelayClosing, NativeHostedRelayProgress};

use super::{
    Connection, NativeAcceptingIo, NativeAcceptingSend, PacketConnection, failure, negotiated,
    packs,
};
use crate::{NativeConnectionTarget, ProtocolError, RawClientData};
use bedrock_client::server::session::{
    AcceptedSession, ServerPackContent, ServerPackSnapshot, ServerWorldReady,
};
use bedrock_net::login::{AuthenticatedLogin, LoginVerifier};
use bedrock_protocol_1_26_51::{packets, wire};
use bytes::Bytes;
use p384::ecdsa::VerifyingKey;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io,
    time::{Instant, SystemTime},
};

/// Sole original local upstream codec/ciphers/FIFO and verified compressed archive source lease.
/// Construction performs no account acquisition, world startup, directory publication or task spawn.
#[must_use]
pub struct NativeHostedUpstream {
    connection: Connection,
    snapshot: Option<ServerPackSnapshot>,
    archives: Vec<Bytes>,
    guest: Guest,
    deadline: Instant,
    initialized: bool,
}

struct Guest {
    xuid: String,
    key: VerifyingKey,
    client_data: [u8; 32],
    uuid: uuid::Uuid,
    name: String,
}

impl Guest {
    /// Binds the original verified guest without cloning its token, nonce or signed metadata.
    fn new(login: &AuthenticatedLogin) -> io::Result<Self> {
        let player = login.player_identity().ok_or_else(invalid)?;
        Ok(Self {
            xuid: login.xuid().into(),
            key: *login.public_key(),
            client_data: Sha256::digest(login.client_data().get().as_bytes()).into(),
            uuid: player.uuid(),
            name: player.display_name().into(),
        })
    }

    /// Denies a replacement Login even when it reuses the same display name or network endpoint.
    fn matches(&self, login: &AuthenticatedLogin) -> bool {
        self.xuid == login.xuid()
            && self.key == *login.public_key()
            && login.player_identity().is_some_and(|player| {
                player.uuid() == self.uuid && player.display_name() == self.name
            })
            && self.client_data
                == <[u8; 32]>::from(Sha256::digest(login.client_data().get().as_bytes()))
    }
}

impl NativeHostedUpstream {
    /// Opens only the original local HTTP/LAN selection with verified guest UUID/name offline.
    /// The exact caller cutoff includes transport, Login, packs and first real StartGame.
    /// Cancellation of this owned future physically retires its original connection.
    pub async fn connect(
        target: &NativeConnectionTarget,
        login: &AuthenticatedLogin,
        client_cache: bool,
        deadline: Instant,
    ) -> Result<Self, ProtocolError> {
        let credentials = negotiated::Credentials::local_guest(login)?;
        let claims = RawClientData::from_raw(login.client_data()).map_err(failure)?;
        let guest = Guest::new(login).map_err(failure)?;
        tokio::time::timeout_at(deadline.into(), async {
            check(deadline).map_err(failure)?;
            let (transport, address) =
                negotiated::connect_local(target, &credentials, deadline.into()).await?;
            let mut connection = Connection::new(transport);
            connection.primary_only();
            let (acquired, _) = negotiated::negotiate_connected(
                &mut connection,
                &address,
                negotiated::LoginData::Relay {
                    claims,
                    client_cache,
                },
                credentials,
                packs::Acquisition::Hosted,
            )
            .await?;
            let (snapshot, archives) = snapshot(acquired).map_err(failure)?;
            connection.replay_deferred();
            let mut startup = Vec::new();
            let mut bytes = 0_usize;
            loop {
                let raw = connection.next_raw().await.map_err(failure)?;
                let id = header(&raw).map_err(failure)?.id;
                if matches!(
                    packets::PacketId::from_raw(id),
                    Some(packets::PacketId::Disconnect | packets::PacketId::Transfer)
                ) {
                    let decoded = decode(&raw, false).map_err(failure)?;
                    match super::negotiation_boundary(decoded) {
                        Ok(Some(error)) => return Err(error),
                        Err(reason) => {
                            tracing::warn!(?reason, "skipped unusable local hosted transfer")
                        }
                        Ok(None) => {}
                    }
                    continue;
                }
                bytes = bytes
                    .checked_add(raw.len())
                    .filter(|bytes| *bytes <= super::super::startup::MAX_DEFERRED_PACKET_BYTES)
                    .ok_or_else(|| failure(invalid()))?;
                startup.push(Bytes::copy_from_slice(&raw));
                if id == packets::StartGame::ID {
                    break;
                }
            }
            connection.finish_current();
            connection.prepend_raw(startup);
            check(deadline).map_err(failure)?;
            Ok(Self {
                connection,
                snapshot: Some(snapshot),
                archives,
                guest,
                deadline,
                initialized: false,
            })
        })
        .await
        .map_err(|_| failure(expired()))?
    }

    /// Moves actual original metadata once; archives remain with their same bounded source lease.
    pub fn take_packs(&mut self) -> io::Result<ServerPackSnapshot> {
        check(self.deadline)?;
        self.snapshot.take().ok_or_else(invalid)
    }

    /// Reads only a checked original external range; no archive/cache/CDN operation is started.
    pub fn read_pack(&self, archive: usize, offset: u64, length: usize) -> io::Result<Bytes> {
        check(self.deadline)?;
        if length == 0 {
            return Err(invalid());
        }
        let source = self.archives.get(archive).ok_or_else(invalid)?;
        let start = usize::try_from(offset).map_err(|_| invalid())?;
        let end = start
            .checked_add(length)
            .filter(|end| *end <= source.len())
            .ok_or_else(invalid)?;
        Ok(source.slice(start..end))
    }

    /// Reads one original raw packet while retaining every existing codec/cipher/FIFO sibling.
    pub async fn receive(&mut self) -> io::Result<Bytes> {
        self.connection.next_raw().await
    }

    /// Releases only the last actual raw cursor after its destination's complete send.
    /// True means that original packet was a usable terminal, not a remote processing receipt.
    pub fn release_received(&mut self) -> io::Result<bool> {
        if !self.initialized {
            return Err(invalid());
        }
        let terminal = self.connection.current_terminal()?.is_some();
        self.connection.finish_current();
        Ok(terminal)
    }

    /// Prepares one original guest packet on the same upstream without committing its cipher/permit.
    pub fn prepare_raw(
        &mut self,
        raw: Bytes,
        deadline: Instant,
    ) -> io::Result<NativeAcceptingSend<'_>> {
        if !self.initialized {
            return Err(invalid());
        }
        let body = self.connection.raw_body(raw)?;
        self.connection.prepare_send(body, deadline)?;
        Ok(NativeAcceptingSend {
            connection: &mut self.connection,
        })
    }

    /// Resumes the exact original prepared prefix, granted permit or admission receipt.
    pub fn resume_send(&mut self) -> io::Result<NativeAcceptingSend<'_>> {
        if !self.initialized {
            return Err(invalid());
        }
        self.connection.prepared_deadline()?;
        Ok(NativeAcceptingSend {
            connection: &mut self.connection,
        })
    }

    /// Seals new sends while retaining only original receipt/transport/source cleanup.
    pub fn begin_close(mut self, deadline: Instant) -> NativeHostedUpstreamClosing {
        self.connection.retire_send(deadline);
        NativeHostedUpstreamClosing {
            upstream: self,
            deadline,
            failure: None,
            closed: false,
        }
    }

    /// Pairs only this original guest upstream with its actual acknowledged accepting pack owner.
    pub fn relay_world<A>(
        self,
        wire: NativeAcceptingIo,
        ready: ServerWorldReady<A>,
        verifier: &impl LoginVerifier,
        now: Instant,
        utc: SystemTime,
    ) -> io::Result<NativeHostedRelay<A>> {
        if !self.guest.matches(ready.login()) || self.snapshot.is_some() {
            return Err(invalid());
        }
        check(self.deadline)?;
        NativeHostedRelay::new(self, wire, ready, verifier, now, utc)
    }
}

/// Original upstream retirement, without new application admission or replacement IO.
#[must_use]
pub struct NativeHostedUpstreamClosing {
    upstream: NativeHostedUpstream,
    deadline: Instant,
    failure: Option<std::sync::Arc<io::Error>>,
    closed: bool,
}

impl NativeHostedUpstreamClosing {
    /// Releases the same already accepted cursor after a completed downstream effect.
    pub fn release_received(&mut self) -> io::Result<bool> {
        self.upstream.release_received()
    }

    /// Settles only original admitted receipts, then closes the same transport even on failure.
    /// Cancellation retains the first cutoff, receipt, input FIFO and archive source lease.
    pub async fn finish_close(&mut self) -> io::Result<()> {
        if !self.closed {
            let result = self.upstream.connection.settle_send().await;
            if let Err(error) = result {
                self.failure
                    .get_or_insert_with(|| std::sync::Arc::new(error));
            }
            let result =
                tokio::time::timeout_at(self.deadline.into(), self.upstream.connection.close())
                    .await;
            let result = result.unwrap_or_else(|_| Err(expired()));
            if let Err(error) = result {
                self.failure
                    .get_or_insert_with(|| std::sync::Arc::new(error));
            }
            self.closed = true;
        }
        match &self.failure {
            Some(error) => Err(io::Error::new(error.kind(), std::sync::Arc::clone(error))),
            None => Ok(()),
        }
    }

    /// Drains actual accepted input under the first original retirement cutoff.
    pub async fn receive(&mut self) -> io::Result<Bytes> {
        tokio::time::timeout_at(self.deadline.into(), self.upstream.receive())
            .await
            .unwrap_or_else(|_| Err(expired()))
    }
}

/// Keeps exact admitted metadata and original archives aligned by actual offer identity/version.
fn snapshot(acquired: packs::Acquired) -> io::Result<(ServerPackSnapshot, Vec<Bytes>)> {
    let packs::Acquired {
        handoff,
        metadata,
        mut preloaded,
    } = acquired;
    let metadata = *metadata.ok_or_else(invalid)?;
    if metadata.data.len() != metadata.offer.resource_packs.len() {
        return Err(invalid());
    }
    let mut acquired: BTreeMap<_, _> = handoff
        .into_archives()
        .into_iter()
        .map(|pack| ((pack.pack_id, pack.version.clone()), pack))
        .collect();
    let mut contents = Vec::with_capacity(metadata.data.len());
    let mut archives = Vec::with_capacity(metadata.data.len());
    for (offer, data) in metadata.offer.resource_packs.iter().zip(metadata.data) {
        let id = (
            offer.pack_id_version.pack_uuid,
            offer
                .pack_id_version
                .pack_version
                .version
                .to_string_lossy()
                .into_owned(),
        );
        let archive = acquired.remove(&id).ok_or_else(invalid)?;
        let size = u64::try_from(archive.archive.len()).map_err(|_| invalid())?;
        if size != offer.pack_size.0 || archive.content_key.expose() != offer.content_key.as_bytes()
        {
            return Err(invalid());
        }
        let content = match data {
            Some(data) => {
                let hash: [u8; 32] = data.file_hash.as_ref().try_into().map_err(|_| invalid())?;
                if size != data.file_size.0 {
                    return Err(invalid());
                }
                ServerPackContent::external(size, hash, data.pack_type.0, data.is_premium_pack)
            }
            None => preloaded
                .remove(&id.0)
                .ok_or_else(invalid)?
                .into_content(&archive)?,
        };
        contents.push(content);
        archives.push(Bytes::from(archive.archive));
    }
    if !acquired.is_empty() {
        return Err(invalid());
    }
    Ok((
        ServerPackSnapshot {
            offer: metadata.offer,
            stack: metadata.stack,
            contents,
        },
        archives,
    ))
}

/// Reads only the original bounded packet header; unknown well-formed packet IDs stay raw.
fn header(raw: &Bytes) -> io::Result<wire::PacketHeader> {
    let mut reader = wire::Reader::from_shared(raw);
    reader
        .read_var_u32()
        .map(wire::PacketHeader::from_raw)
        .map_err(io::Error::other)
}

/// Decodes only the rare generated control required by the original typed startup observer.
fn decode(raw: &Bytes, client: bool) -> io::Result<packets::Packet> {
    let mut reader = wire::Reader::from_shared(raw);
    let header = wire::PacketHeader::from_raw(reader.read_var_u32().map_err(io::Error::other)?);
    if header.sender_subclient != 0 || header.target_subclient != 0 {
        return Err(invalid());
    }
    let id = packets::PacketId::from_raw(header.id).ok_or_else(invalid)?;
    let body = &raw.slice(reader.position()..);
    if client {
        crate::codec::decode_client_body(id, body)
    } else {
        crate::codec::decode_body(id, body)
    }
    .map_err(io::Error::other)
}

/// Uses the original edge clock and finite caller deadline, without a new timeout policy.
fn check(deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        Err(expired())
    } else {
        Ok(())
    }
}

/// Omits endpoint, account, signed data and source-pack details from diagnostics.
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid original local hosted upstream",
    )
}

/// Reports the original caller cutoff without leaking the selected endpoint or guest.
fn expired() -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        "local hosted upstream deadline elapsed",
    )
}
