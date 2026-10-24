//! The acquired archive owner, delivered serially or on the same WebTransport session.

#[cfg(feature = "webtransport")]
use super::current;
use super::{Bytes, CoreMessage, FrameQueue, PlayLease, duplex, invalid, session_wire};
use bridge::contract::PACK_CHUNK_BYTES;
use std::{io, time::Instant};

pub(super) enum Delivery {
    Serial,
    #[cfg(feature = "webtransport")]
    WebTransport(std::sync::Arc<crate::backend::webtransport::Transport>),
}

impl Delivery {
    /// Only an actual WT session can advertise an independently owned archive stream.
    pub(super) fn independent(&self) -> bool {
        match self {
            Self::Serial => false,
            #[cfg(feature = "webtransport")]
            Self::WebTransport(_) => true,
        }
    }
}

/// Moves the actual negotiated archives once; no fetch, archive cache or detached writer is added.
pub(super) async fn send(
    delivery: Delivery,
    lease: &PlayLease,
    frames: &FrameQueue,
    archives: Vec<Vec<u8>>,
    deadline: Instant,
) -> io::Result<()> {
    match delivery {
        Delivery::Serial => {
            for (index, archive) in archives.into_iter().enumerate() {
                let index = u32::try_from(index).map_err(invalid)?;
                for chunk in archive.chunks(PACK_CHUNK_BYTES) {
                    lease.current()?;
                    let frame = packet(index, chunk)?;
                    duplex::write(lease, frames, frame, Some(deadline)).await?;
                }
            }
            Ok(())
        }
        #[cfg(feature = "webtransport")]
        Delivery::WebTransport(transport) => {
            let writing = write_uni(&transport, lease, archives, deadline);
            tokio::pin!(writing);
            tokio::select! { biased;
                () = lease.cancel.cancelled() => Err(super::closed()),
                result = tokio::time::timeout_at(deadline.into(), &mut writing) => result.map_err(|_| super::elapsed())?,
            }
        }
    }
}

/// Encodes one original generated PackData frame at the existing bounded chunk size.
fn packet(index: u32, chunk: &[u8]) -> io::Result<Bytes> {
    session_wire::encode_core_message(&CoreMessage::PackData {
        index,
        data: Bytes::copy_from_slice(chunk),
    })
    .map_err(invalid)
}

/// Finishes the original stream only after the join prefix and every archive byte were accepted.
#[cfg(feature = "webtransport")]
async fn write_uni(
    transport: &crate::backend::webtransport::Transport,
    lease: &PlayLease,
    archives: Vec<Vec<u8>>,
    deadline: Instant,
) -> io::Result<()> {
    use tokio::io::AsyncWriteExt;
    current(lease, Some(deadline))?;
    let stream = transport
        .open_uni(transport.session_id())
        .await
        .map_err(invalid)?;
    let mut writer = UniWriter(Some(stream));
    let stream = writer.0.as_mut().expect("original pack stream");
    current(lease, Some(deadline))?;
    stream.write_all(&lease.join.to_be_bytes()).await?;
    for (index, archive) in archives.into_iter().enumerate() {
        let index = u32::try_from(index).map_err(invalid)?;
        for chunk in archive.chunks(PACK_CHUNK_BYTES) {
            current(lease, Some(deadline))?;
            let frame = packet(index, chunk)?;
            let length = u32::try_from(frame.len()).map_err(invalid)?;
            stream.write_all(&length.to_be_bytes()).await?;
            stream.write_all(&frame).await?;
        }
    }
    stream.flush().await?;
    current(lease, Some(deadline))?;
    stream.shutdown().await?;
    current(lease, Some(deadline))?;
    writer.0 = None;
    Ok(())
}

#[cfg(feature = "webtransport")]
struct UniWriter(Option<h3_webtransport::stream::SendStream<h3_quinn::SendStream<Bytes>, Bytes>>);

#[cfg(feature = "webtransport")]
impl Drop for UniWriter {
    fn drop(&mut self) {
        use h3::quic::SendStream;
        if let Some(stream) = self.0.as_mut() {
            stream.reset(0);
        }
    }
}
