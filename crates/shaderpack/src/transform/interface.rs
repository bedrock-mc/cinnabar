//! Location and binding numbering shared by a program's stages.

use crate::reflection::{FIRST_TEXTURE_BINDING, LOOSE_UNIFORM_BINDING};
use std::collections::BTreeMap;

/// Interface numbering shared by a program's stages: varying name → (first location,
/// count), and resource name → binding, so the vertex and fragment stages agree.
#[derive(Default)]
pub(crate) struct ProgramInterface {
    varyings: BTreeMap<String, (u32, u32)>,
    next_varying: u32,
    textures: BTreeMap<String, u32>,
    images: BTreeMap<String, u32>,
    uniform_blocks: BTreeMap<String, u32>,
    storage: BTreeMap<String, u32>,
}

impl ProgramInterface {
    /// First location of varying `name`, allocating `size` locations on first use.
    pub(super) fn varying(&mut self, name: &str, size: u32) -> u32 {
        if let Some((l, _)) = self.varyings.get(name) {
            return *l;
        }
        let l = self.next_varying;
        self.varyings.insert(name.to_string(), (l, size));
        self.next_varying += size;
        l
    }

    /// Binding of sampled texture `name`, after the two shared samplers.
    pub(super) fn texture_binding(&mut self, name: &str) -> u32 {
        let next = FIRST_TEXTURE_BINDING + self.textures.len() as u32;
        *self.textures.entry(name.to_string()).or_insert(next)
    }

    /// Binding of storage image `name`.
    pub(super) fn image_binding(&mut self, name: &str) -> u32 {
        let next = self.images.len() as u32;
        *self.images.entry(name.to_string()).or_insert(next)
    }

    /// Binding of pack uniform block `name`, after the loose-uniform block.
    pub(super) fn uniform_block_binding(&mut self, name: &str) -> u32 {
        let next = LOOSE_UNIFORM_BINDING + 1 + self.uniform_blocks.len() as u32;
        *self.uniform_blocks.entry(name.to_string()).or_insert(next)
    }

    /// Binding of storage block `name`: the pack's own `binding`, else the next free one.
    pub(super) fn storage_binding(&mut self, name: &str, declared: Option<u32>) -> u32 {
        let next = self.storage.values().max().map_or(0, |m| m + 1);
        *self
            .storage
            .entry(name.to_string())
            .or_insert(declared.unwrap_or(next))
    }
}
