//! Session block ids as distant terrain materials and classes.
//!
//! Palette values are the session's internal block ids: sequential ids in a sequential
//! session, network hashes in a hashed one, and the admitted ids of server-custom states.
//! Vanilla states are keyed by their network hash, custom states by name and canonical
//! state text, so stored materials survive id changes between sessions and servers.
//! Classes come from the runtime assets, the lowest owner of block visuals that this crate
//! can read, and are never stored.

use std::{collections::HashMap, sync::Arc};

use assets::{BlockFlags, NetworkIdMode, ResolvedBlock, RuntimeAssets, VisualKind};
use client_world::CustomBlockState;
use lod::{
    classify::{BlockClass, BlockClassifier, MaterialClasses, SampledBlock},
    registry::{CustomStateKey, MaterialKey},
    voxel::MaterialId,
};
use lod_store::store::SharedRegistry;

/// The session's block identities, rebuilt when its assets or custom blocks change.
pub(in crate::stream) struct SessionBlocks {
    assets: Arc<RuntimeAssets>,
    mode: NetworkIdMode,
    custom: Arc<HashMap<u32, CustomBlockState>>,
    /// Custom state keys back to their session ids, for classifying stored materials.
    custom_ids: HashMap<MaterialKey, u32>,
}

impl SessionBlocks {
    /// The identities of a session in `mode` with `assets` and admitted `custom` states.
    pub(in crate::stream) fn new(
        assets: Arc<RuntimeAssets>,
        mode: NetworkIdMode,
        custom: Arc<HashMap<u32, CustomBlockState>>,
    ) -> Self {
        let custom_ids = custom
            .iter()
            .filter_map(|(&id, state)| Some((custom_key(state)?, id)))
            .collect();
        Self {
            assets,
            mode,
            custom,
            custom_ids,
        }
    }

    /// True when these identities were built from exactly these inputs.
    pub(in crate::stream) fn matches(
        &self,
        assets: &Arc<RuntimeAssets>,
        mode: NetworkIdMode,
        custom: &Arc<HashMap<u32, CustomBlockState>>,
    ) -> bool {
        Arc::ptr_eq(&self.assets, assets) && self.mode == mode && Arc::ptr_eq(&self.custom, custom)
    }

    /// The persistent key of a palette value; `None` for ids no registry names.
    fn key(&self, value: u32) -> Option<MaterialKey> {
        if let Some(state) = self.custom.get(&value) {
            return custom_key(state);
        }
        let hash = match self.mode {
            NetworkIdMode::Hashed => value,
            NetworkIdMode::Sequential => assets::pinned_block_network_hash(value)?,
        };
        Some(MaterialKey::Network(hash))
    }

    /// The class of a palette value; ids the assets do not know are air, as vanilla
    /// palettes decode them.
    fn class_of_value(&self, value: u32) -> BlockClass {
        if !self.assets.is_known(self.mode, value) {
            return BlockClass::Air;
        }
        block_class(&self.assets, self.assets.resolve(self.mode, value))
    }

    /// The class of a stored material's key under the current assets; keys the session
    /// cannot resolve are opaque, as stored materials are never air.
    fn class_of_key(&self, key: &MaterialKey) -> BlockClass {
        let value = match key {
            MaterialKey::Network(hash) => match self.mode {
                NetworkIdMode::Hashed => Some(*hash),
                NetworkIdMode::Sequential => assets::pinned_block_sequential_id(*hash),
            },
            MaterialKey::Custom(_) => self.custom_ids.get(key).copied(),
        };
        match value.map(|value| self.class_of_value(value)) {
            Some(BlockClass::Air) | None => BlockClass::Opaque,
            Some(class) => class,
        }
    }
}

/// The class of a resolved block: unknown, air and invisible blocks are air, water is
/// water, other liquids are opaque, leaf models are leaves, cubes that occlude whole faces
/// are opaque and everything else lets light through.
fn block_class(assets: &RuntimeAssets, block: ResolvedBlock) -> BlockClass {
    let flags = block.flags();
    if !block.is_known() || flags.contains(BlockFlags::AIR) || block.kind() == VisualKind::Invisible
    {
        BlockClass::Air
    } else if block.kind() == VisualKind::Liquid {
        if ::meshing::is_water_block(assets, block) {
            BlockClass::Water
        } else {
            BlockClass::Opaque
        }
    } else if flags.contains(BlockFlags::LEAF_MODEL) {
        BlockClass::Leaves
    } else if block.kind() == VisualKind::Cube && flags.contains(BlockFlags::OCCLUDES_FULL_FACE) {
        BlockClass::Opaque
    } else {
        BlockClass::SeeThrough
    }
}

/// The registry key of a custom state; `None` when its text exceeds the key limit.
fn custom_key(state: &CustomBlockState) -> Option<MaterialKey> {
    CustomStateKey::new(&state.name, &state.states)
        .ok()
        .map(MaterialKey::Custom)
}

/// Palette values a column's classifier could not store, each counted once per column.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::stream) struct ClassifierCounts {
    /// Ids the session's assets do not know, sampled as air.
    pub(in crate::stream) unknown_ids: u64,
    /// Visible ids no registry key names, sampled as air.
    pub(in crate::stream) unkeyed_ids: u64,
    /// Visible states sampled as air because the store's registry is full.
    pub(in crate::stream) registry_full: u64,
}

/// Classifies one column's palette values, assigning store materials to new states and
/// remembering each value's result and the class of each material it returns.
pub(in crate::stream) struct ColumnClassifier<'a> {
    blocks: &'a SessionBlocks,
    registry: &'a SharedRegistry,
    values: HashMap<u32, SampledBlock>,
    classes: HashMap<MaterialId, BlockClass>,
    counts: ClassifierCounts,
}

impl<'a> ColumnClassifier<'a> {
    /// A classifier assigning materials in `registry`.
    pub(in crate::stream) fn new(blocks: &'a SessionBlocks, registry: &'a SharedRegistry) -> Self {
        Self {
            blocks,
            registry,
            values: HashMap::new(),
            classes: HashMap::new(),
            counts: ClassifierCounts::default(),
        }
    }

    /// The values this classifier stored as air although they are not air.
    pub(in crate::stream) fn counts(&self) -> ClassifierCounts {
        self.counts
    }

    /// The store material of `key`, assigning one when it is new; `None` when the
    /// registry is full.
    fn material(&mut self, key: &MaterialKey) -> Option<MaterialId> {
        let known = self
            .registry
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(key);
        if known.is_some() {
            return known;
        }
        self.registry
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_or_insert(key)
            .ok()
    }

    /// [`BlockClassifier::classify`] for a value not seen before, counting why a visible
    /// value is stored as air.
    fn classify_new(&mut self, runtime_id: u32) -> SampledBlock {
        if !self.blocks.assets.is_known(self.blocks.mode, runtime_id) {
            self.counts.unknown_ids += 1;
            return SampledBlock::AIR;
        }
        let class = self.blocks.class_of_value(runtime_id);
        if class == BlockClass::Air {
            return SampledBlock::AIR;
        }
        let Some(key) = self.blocks.key(runtime_id) else {
            self.counts.unkeyed_ids += 1;
            return SampledBlock::AIR;
        };
        let Some(material) = self.material(&key) else {
            self.counts.registry_full += 1;
            return SampledBlock::AIR;
        };
        self.classes.insert(material, class);
        SampledBlock { material, class }
    }
}

impl BlockClassifier for ColumnClassifier<'_> {
    fn classify(&mut self, runtime_id: u32) -> SampledBlock {
        if let Some(&block) = self.values.get(&runtime_id) {
            return block;
        }
        let block = self.classify_new(runtime_id);
        self.values.insert(runtime_id, block);
        block
    }
}

impl MaterialClasses for ColumnClassifier<'_> {
    fn class(&self, material: MaterialId) -> BlockClass {
        self.classes
            .get(&material)
            .copied()
            .unwrap_or(BlockClass::Opaque)
    }
}

/// Classes of every store material under the session's current assets, extended as the
/// registry grows and rebuilt when the session's identities change.
#[derive(Default)]
pub(in crate::stream) struct MaterialClassTable {
    /// Classes by raw material id; id 0 is air and never read.
    classes: Vec<BlockClass>,
    blocks: Option<Arc<SessionBlocks>>,
}

impl MaterialClassTable {
    /// Classifies every material `registry` assigned since the last refresh.
    pub(in crate::stream) fn refresh(
        &mut self,
        blocks: &Arc<SessionBlocks>,
        registry: &SharedRegistry,
    ) {
        if !self
            .blocks
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, blocks))
        {
            self.classes.clear();
            self.blocks = Some(Arc::clone(blocks));
        }
        let registry = registry
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let known = self
            .classes
            .len()
            .max(MaterialId::FIRST_BLOCK.raw() as usize);
        self.classes.resize(known, BlockClass::Air);
        for (material, key) in registry
            .iter()
            .skip(known - MaterialId::FIRST_BLOCK.raw() as usize)
        {
            debug_assert_eq!(material.raw() as usize, self.classes.len());
            self.classes.push(blocks.class_of_key(key));
        }
    }
}

impl MaterialClasses for MaterialClassTable {
    fn class(&self, material: MaterialId) -> BlockClass {
        self.classes
            .get(material.raw() as usize)
            .copied()
            .unwrap_or(BlockClass::Opaque)
    }
}

#[cfg(test)]
mod tests;
