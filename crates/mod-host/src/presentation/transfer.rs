//! Bounded helper chunks let retained scenes exceed one callback's output allowance.

use super::*;

/// One ordered piece of an immutable committed snapshot.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chunk {
    pub revision: u64,
    pub offset: u32,
    pub total: u32,
    pub bytes: Vec<u8>,
}

/// A committed snapshot being delivered without changing the active renderer scene.
pub(crate) struct Pending {
    revision: u64,
    bytes: Vec<u8>,
    offset: usize,
}

impl Pending {
    /// Bounds the complete transfer by the bundle's existing memory reservation.
    pub(crate) fn new(output: &Output, memory_bytes: u64) -> Result<Self> {
        let bytes = crate::helper::serialize_frame(output, usize::try_from(memory_bytes)?)?;
        ensure!(
            bytes.len() as u64 <= memory_bytes,
            "drawing transfer memory budget exceeded"
        );
        ensure!(
            u32::try_from(bytes.len()).is_ok(),
            "drawing transfer too large"
        );
        Ok(Self {
            revision: mod_render::next_owner(),
            bytes,
            offset: 0,
        })
    }

    /// Takes only bytes that fit this callback, accounting for JSON's byte-array expansion.
    pub(crate) fn take(&mut self, byte_allowance: usize) -> Option<Chunk> {
        if byte_allowance == 0 || self.complete() {
            return None;
        }
        let end = self
            .bytes
            .len()
            .min(self.offset.saturating_add(byte_allowance));
        let chunk = Chunk {
            revision: self.revision,
            offset: self.offset as u32,
            total: self.bytes.len() as u32,
            bytes: self.bytes[self.offset..end].to_vec(),
        };
        self.offset = end;
        Some(chunk)
    }

    /// Reports completion so the next callback can deliver a newer coalesced snapshot.
    pub(crate) fn complete(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

#[derive(Default)]
pub(super) struct Assembly {
    revision: u64,
    total: u32,
    bytes: Vec<u8>,
    context: Context,
}

impl Assembly {
    /// Accepts ordered bounded pieces and exposes nothing until the whole snapshot arrives.
    pub(super) fn receive(
        &mut self,
        chunk: Chunk,
        context: &Context,
        memory_bytes: u64,
    ) -> Result<Option<(Output, Context)>> {
        ensure!(
            chunk.total > 0 && u64::from(chunk.total) <= memory_bytes,
            "drawing transfer memory budget exceeded"
        );
        ensure!(
            !chunk.bytes.is_empty()
                && chunk.bytes.len() <= server_experience::policy::MAX_HOST_OUTPUT / 4,
            "invalid drawing chunk"
        );
        if chunk.offset == 0 {
            self.context = context.clone();
            self.revision = chunk.revision;
            self.total = chunk.total;
            self.bytes.clear();
        }
        ensure!(
            chunk.revision == self.revision
                && chunk.total == self.total
                && chunk.offset as usize == self.bytes.len(),
            "stale or out-of-order drawing chunk"
        );
        ensure!(
            chunk.bytes.len() <= self.total as usize - self.bytes.len(),
            "drawing transfer overflow"
        );
        self.bytes.extend_from_slice(&chunk.bytes);
        if self.bytes.len() == self.total as usize {
            let output = serde_json::from_slice(&self.bytes)?;
            let context = std::mem::take(&mut self.context);
            *self = Self::default();
            Ok(Some((output, context)))
        } else {
            Ok(None)
        }
    }
}
