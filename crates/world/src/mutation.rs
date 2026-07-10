use crate::{MAX_STORAGE_COUNT, MutationError};

/// One packet-neutral block change inside a 16x16x16 sub-chunk.
///
/// `layer` maps directly to `UpdateBlock.Layer`. Callers handling an
/// `UpdateSubChunkBlocks` packet use layer zero for `Blocks` and layer one for
/// `Extra`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockUpdate {
    pub x: u8,
    pub y: u8,
    pub z: u8,
    pub layer: u32,
    /// Raw network runtime ID or block-state hash.
    pub runtime_id: u32,
}

impl BlockUpdate {
    #[must_use]
    pub const fn new(x: u8, y: u8, z: u8, layer: u32, runtime_id: u32) -> Self {
        Self {
            x,
            y,
            z,
            layer,
            runtime_id,
        }
    }

    pub(crate) fn validate(self) -> Result<(), MutationError> {
        if self.x >= 16 || self.y >= 16 || self.z >= 16 {
            return Err(MutationError::LocalCoordinatesOutOfBounds {
                x: self.x,
                y: self.y,
                z: self.z,
            });
        }
        if self.layer >= MAX_STORAGE_COUNT as u32 {
            return Err(MutationError::LayerOutOfBounds {
                layer: self.layer,
                max: MAX_STORAGE_COUNT,
            });
        }
        Ok(())
    }
}
