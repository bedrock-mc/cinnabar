//! The uploaded block-entity lists the shader pack draws: alpha-tested models, blended
//! layers with the block-entity atlas. Beams and additive rays retain native composition.

use super::*;
use crate::shaderpack::actors::{BlockEntityList, BlockEntitySource};

/// The block-entity lists, once the atlas is uploaded.
pub(crate) fn shaderpack_block_entities(world: &World) -> Option<BlockEntitySource<'_>> {
    let gpu = world.get_resource::<BlockEntityGpu>()?;
    /// Borrows one nonempty uploaded vertex list.
    fn list(list: &VertexList) -> Option<BlockEntityList<'_>> {
        list.buffer
            .as_ref()
            .filter(|_| list.count != 0)
            .map(|buffer| BlockEntityList {
                buffer,
                count: list.count,
            })
    }
    Some(BlockEntitySource {
        atlas: gpu.texture.as_ref()?,
        solid: list(&gpu.solid),
        overlay: list(&gpu.overlay),
    })
}

impl BlockEntityGpu {
    /// The uploaded block-entity atlas, destroy stages included, which shader-pack crack
    /// draws read through a gamma-byte view rather than a copy of their own.
    pub(crate) fn atlas_texture(&self) -> Option<&Texture> {
        self.texture.as_ref()
    }
}
