//! Admission and integrity checks shared by inline and independent archive delivery.

use std::path::Path;

use bridge::{
    BridgeError,
    contract::{KIND_PACK_DATA, PACK_CHUNK_BYTES},
};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt};

use super::{CoreMessage, SessionHandoff, decode_core_message, invalid};

/// One admitted archive, with its digest updated as bytes arrive.
struct Archive {
    size: u64,
    bytes: Vec<u8>,
    digest: Option<(Sha256, [u8; 32])>,
}

/// Loads cached archives and reassembles streamed ones in handoff order.
pub struct HandoffPackReceiver {
    archives: Vec<Archive>,
    next: usize,
}

impl HandoffPackReceiver {
    /// Checks the whole selection before loading any cache or accepting archive bytes.
    /// Cache reads belong on a blocking worker and use only the caller's trusted root.
    pub fn new(handoff: &SessionHandoff, cache_root: Option<&Path>) -> Result<Self, BridgeError> {
        for pack in &handoff.packs {
            if usize::try_from(pack.size).map_or(true, |size| size > isize::MAX as usize) {
                return Err(invalid("handoff archive size cannot be represented"));
            }
        }
        let archives = handoff
            .packs
            .iter()
            .map(|pack| {
                let bytes = match &pack.cache {
                    Some(reference) => reference.read(cache_root, pack.size)?,
                    None => Vec::new(),
                };
                let digest = pack.sha256.map(|expected| {
                    let mut hash = Sha256::new();
                    hash.update(&bytes);
                    (hash, expected)
                });
                Ok(Archive {
                    size: pack.size,
                    bytes,
                    digest,
                })
            })
            .collect::<Result<Vec<_>, BridgeError>>()?;
        let mut receiver = Self { archives, next: 0 };
        receiver.skip_complete()?;
        Ok(receiver)
    }

    /// Admits web archives only with a digest and without native filesystem references.
    pub fn web(handoff: &SessionHandoff) -> Result<Self, BridgeError> {
        if handoff
            .packs
            .iter()
            .any(|pack| pack.sha256.is_none() || pack.cache.is_some())
        {
            return Err(invalid("web archive requires a streamed digest"));
        }
        Self::new(handoff, None)
    }

    /// Whether every declared archive has passed its length and digest checks.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.next == self.archives.len()
    }

    /// Appends a nonempty chunk to the first incomplete archive and checks its final digest.
    pub fn accept(&mut self, index: u32, data: &[u8]) -> Result<(), BridgeError> {
        if self.is_complete() || self.next as u64 != u64::from(index) {
            return Err(invalid("pack data out of order"));
        }
        let archive = &mut self.archives[self.next];
        if data.is_empty() || data.len() as u64 > archive.size - archive.bytes.len() as u64 {
            return Err(invalid("pack data exceeds the archive size"));
        }
        if let Some((hash, _)) = &mut archive.digest {
            hash.update(data);
        }
        archive.bytes.extend_from_slice(data);
        self.skip_complete()
    }

    /// Reads one join's independent stream through FIN. Extra, truncated or misassociated data
    /// fails the join. Dropping this future and its stream resets browser receive ownership.
    pub async fn receive_stream(
        &mut self,
        join: u64,
        mut stream: impl AsyncRead + Unpin,
    ) -> Result<(), BridgeError> {
        if join == 0 || stream.read_u64().await? != join {
            return Err(invalid("pack stream join mismatch"));
        }
        loop {
            let mut header = [0; 4];
            if stream.read(&mut header[..1]).await? == 0 {
                return if self.is_complete() {
                    Ok(())
                } else {
                    Err(invalid("pack stream incomplete"))
                };
            }
            stream.read_exact(&mut header[1..]).await?;
            let length = u32::from_be_bytes(header) as usize;
            if !(6..=PACK_CHUNK_BYTES + 5).contains(&length) {
                return Err(invalid("pack stream frame limit"));
            }
            // Grow only as bytes arrive; a declared chunk never reserves its entire size.
            let mut frame = Vec::new();
            let mut limited = (&mut stream).take(length as u64);
            limited.read_to_end(&mut frame).await?;
            if frame.len() != length {
                return Err(invalid("truncated pack stream frame"));
            }
            if frame.first() != Some(&KIND_PACK_DATA) {
                return Err(invalid("non-pack frame on pack stream"));
            }
            match decode_core_message(frame.into())? {
                CoreMessage::PackData { index, data } => self.accept(index, &data)?,
                _ => return Err(invalid("non-pack frame on pack stream")),
            }
        }
    }

    /// Returns verified archives in handoff order; partial or corrupt archives cannot escape.
    pub fn into_archives(self) -> Result<Vec<Vec<u8>>, BridgeError> {
        if !self.is_complete() {
            return Err(invalid("pack data incomplete"));
        }
        Ok(self
            .archives
            .into_iter()
            .map(|archive| archive.bytes)
            .collect())
    }

    /// Verifies finished archives, including empty and cached ones, before advancing.
    fn skip_complete(&mut self) -> Result<(), BridgeError> {
        while let Some(archive) = self.archives.get_mut(self.next) {
            if archive.bytes.len() as u64 != archive.size {
                break;
            }
            if let Some((hash, expected)) = &mut archive.digest {
                let actual: [u8; 32] = std::mem::take(hash).finalize().into();
                if actual != *expected {
                    return Err(invalid("streamed archive hash mismatch"));
                }
            }
            self.next += 1;
        }
        Ok(())
    }
}
