//! One original accepting pack driver, with concrete retained archive source owners.
use super::*;
use bedrock_client::server::session::{
    ServerPackDelivery, ServerPackEvent, ServerPackSnapshot, ServerWorldReady,
};
use bytes::Bytes;
use protocol::NativeHostedUpstream;

pub(super) enum PackSource<'a> {
    Dragonfly(&'a mut PendingBackendPeer),
    Bds(&'a mut NativeHostedUpstream),
}
impl PackSource<'_> {
    /// Reads only the requested original archive range; no alternate cache or acquisition is created.
    async fn read(
        &mut self,
        archive: u32,
        offset: u64,
        length: u32,
        deadline: Instant,
    ) -> io::Result<Bytes> {
        match self {
            Self::Dragonfly(backend) => backend.read_pack(archive, offset, length, deadline).await,
            Self::Bds(upstream) => upstream.read_pack(
                usize::try_from(archive).map_err(|_| rejected())?,
                offset,
                usize::try_from(length).map_err(|_| rejected())?,
            ),
        }
    }
}
impl NativeHostedWorld {
    /// Delivers one original snapshot/source through actual generated pack flush continuations.
    pub(super) async fn deliver_packs(
        &mut self,
        wire: &mut NativeAcceptingIo,
        ready: ServerPacksReady<HostedLoginApproval>,
        snapshot: ServerPackSnapshot,
        mut source: PackSource<'_>,
        policy: &NativeHostedLoginPolicy,
        deadline: Instant,
    ) -> io::Result<ServerWorldReady<HostedLoginApproval>> {
        self.wait_login(ready.login(), policy, ready.grant())
            .await?;
        let delivery = ServerPackDelivery {
            chunk_size: bridge::host_world::contract::MAX_PACK_READ_BYTES,
            ..Default::default()
        };
        let (mut session, packets) = ready
            .start_packs(
                snapshot,
                delivery,
                policy,
                Instant::now(),
                policy.utc().map_err(|_| rejected())?,
            )
            .map_err(|_| rejected())?;
        self.send_setup(
            wire.prepare_packets(packets, deadline)?,
            SendAuthority::Login(session.login(), policy, session.grant()),
        )
        .await?;
        let ready = loop {
            self.wait_login(session.login(), policy, session.grant())
                .await?;
            let packet = self.peer_io(Some(deadline), wire.receive(deadline)).await?;
            self.wait_login(session.login(), policy, session.grant())
                .await?;
            match session
                .receive(packet, Instant::now())
                .map_err(|_| rejected())?
            {
                ServerPackEvent::Send {
                    session: retained,
                    packets,
                } => {
                    self.wait_login(retained.login(), policy, retained.grant())
                        .await?;
                    self.send_setup(
                        wire.prepare_packets(packets, deadline)?,
                        SendAuthority::Login(retained.login(), policy, retained.grant()),
                    )
                    .await?;
                    session = retained;
                }
                ServerPackEvent::ReadChunk(read) => {
                    self.wait_login(read.login(), policy, read.grant()).await?;
                    let archive = u32::try_from(read.archive_index()).map_err(|_| rejected())?;
                    let length = u32::try_from(read.byte_len()).map_err(|_| rejected())?;
                    let bytes = self
                        .peer_io(
                            Some(deadline),
                            source.read(archive, read.offset(), length, deadline),
                        )
                        .await?;
                    self.wait_login(read.login(), policy, read.grant()).await?;
                    let pending = read
                        .supply(bytes.to_vec(), Instant::now())
                        .map_err(|_| rejected())?;
                    self.wait_login(pending.login(), policy, pending.grant())
                        .await?;
                    self.send_setup(
                        wire.prepare_packets(vec![pending.packet().clone().into()], deadline)?,
                        SendAuthority::Login(pending.login(), policy, pending.grant()),
                    )
                    .await?;
                    self.wait_login(pending.login(), policy, pending.grant())
                        .await?;
                    let (retained, packets) =
                        pending.flushed(Instant::now()).map_err(|_| rejected())?;
                    self.send_setup(
                        wire.prepare_packets(packets, deadline)?,
                        SendAuthority::Login(retained.login(), policy, retained.grant()),
                    )
                    .await?;
                    session = retained;
                }
                ServerPackEvent::Chunk(pending) => {
                    self.wait_login(pending.login(), policy, pending.grant())
                        .await?;
                    self.send_setup(
                        wire.prepare_packets(vec![pending.packet().clone().into()], deadline)?,
                        SendAuthority::Login(pending.login(), policy, pending.grant()),
                    )
                    .await?;
                    self.wait_login(pending.login(), policy, pending.grant())
                        .await?;
                    let (retained, packets) =
                        pending.flushed(Instant::now()).map_err(|_| rejected())?;
                    self.send_setup(
                        wire.prepare_packets(packets, deadline)?,
                        SendAuthority::Login(retained.login(), policy, retained.grant()),
                    )
                    .await?;
                    session = retained;
                }
                ServerPackEvent::WorldReady(ready) => break ready,
                ServerPackEvent::Unhandled {
                    session: retained,
                    packet,
                } => {
                    wire.handle_unhandled(&packet)?;
                    session = retained;
                }
            }
        };
        self.wait_login(ready.login(), policy, ready.grant())
            .await?;
        Ok(ready)
    }
}
