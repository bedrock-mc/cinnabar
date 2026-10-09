mod decode;
mod id_remap;
mod overlay;
mod server_defined_blocks;

pub use id_remap::SequentialIdRemap;
pub use overlay::{BlockOverlay, MaterialOverride};
pub use server_defined_blocks::{
    ServerDefinedBlock, server_defined_blocks, server_defined_blocks_for_registry,
};

use std::sync::atomic::{AtomicU64, Ordering};

use crate::{
    Animation, BlockFace, BlockFlags, BlockVisual, CompiledBiomeAssets, ContributorRole,
    DIAGNOSTIC_MATERIAL, LightProperties, Material, ModelQuad, ModelTemplate, NO_ANIMATION,
    NO_MODEL_TEMPLATE, TextureArray, TextureMip, TexturePage, TextureRef, VisualKind,
    VisualSupport, provenance::BlobProvenance,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkIdMode {
    Sequential,
    Hashed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedFace {
    material_id: u32,
}
impl ResolvedFace {
    #[must_use]
    pub const fn material_id(self) -> u32 {
        self.material_id
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedBlock {
    visual: BlockVisual,
    light_properties: LightProperties,
    known: bool,
}
impl ResolvedBlock {
    const fn known(visual: BlockVisual, light_properties: LightProperties) -> Self {
        Self {
            visual,
            light_properties,
            known: true,
        }
    }
    const fn diagnostic() -> Self {
        Self {
            visual: diagnostic_visual(),
            light_properties: LightProperties::OPAQUE_DARK,
            known: false,
        }
    }
    #[must_use]
    pub const fn is_known(self) -> bool {
        self.known
    }
    #[must_use]
    pub const fn flags(self) -> BlockFlags {
        self.visual.flags
    }
    #[must_use]
    pub const fn face(self, face: BlockFace) -> ResolvedFace {
        ResolvedFace {
            material_id: self.visual.faces[face as usize],
        }
    }
    #[must_use]
    pub const fn kind(self) -> VisualKind {
        self.visual.kind
    }
    #[must_use]
    pub const fn support(self) -> VisualSupport {
        self.visual.support
    }
    #[must_use]
    pub const fn contributor_role(self) -> ContributorRole {
        self.visual.contributor_role
    }
    #[must_use]
    pub const fn model_template(self) -> Option<u32> {
        if self.visual.model_template == NO_MODEL_TEMPLATE {
            None
        } else {
            Some(self.visual.model_template)
        }
    }
    #[must_use]
    pub const fn animation(self) -> Option<u32> {
        if self.visual.animation == NO_ANIMATION {
            None
        } else {
            Some(self.visual.animation)
        }
    }
    #[must_use]
    pub const fn variant(self) -> u32 {
        self.visual.variant
    }
    #[must_use]
    pub const fn light_properties(self) -> LightProperties {
        self.light_properties
    }
}

const fn diagnostic_visual() -> BlockVisual {
    BlockVisual {
        faces: [DIAGNOSTIC_MATERIAL; 6],
        flags: BlockFlags::empty(),
        kind: VisualKind::Diagnostic,
        support: VisualSupport::Diagnostic,
        contributor_role: ContributorRole::Primary,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
    }
}

pub struct RuntimeAssets {
    visuals: Box<[BlockVisual]>,
    light_properties: Box<[LightProperties]>,
    hashed: Box<[(u32, u32)]>,
    materials: Box<[Material]>,
    model_templates: Box<[ModelTemplate]>,
    model_random_offsets: Box<[(u32, block_transform::random_offset::RandomOffsetComponent)]>,
    model_quads: Box<[ModelQuad]>,
    animations: Box<[Animation]>,
    animation_frames: Box<[TextureRef]>,
    texture_pages: Box<[TexturePage]>,
    overlay_texture_source_sizes: Box<[[u16; 2]]>,
    biomes: CompiledBiomeAssets,
    provenance: BlobProvenance,
    missing: AtomicU64,
}

impl RuntimeAssets {
    #[must_use]
    pub fn diagnostic() -> Self {
        let mips = [16_u32, 8, 4, 2, 1]
            .into_iter()
            .map(|size| {
                let mut rgba8 = Vec::with_capacity(size as usize * size as usize * 4);
                for y in 0..size {
                    for x in 0..size {
                        rgba8.extend_from_slice(if (x + y) & 1 == 0 {
                            &[255, 0, 255, 255]
                        } else {
                            &[0, 0, 0, 255]
                        });
                    }
                }
                TextureMip {
                    size,
                    rgba8: rgba8.into_boxed_slice(),
                }
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            visuals: vec![diagnostic_visual()].into_boxed_slice(),
            light_properties: vec![LightProperties::OPAQUE_DARK].into_boxed_slice(),
            hashed: Box::new([]),
            materials: vec![Material {
                texture: TextureRef::DIAGNOSTIC,
                flags: 0,
                animation: NO_ANIMATION,
                ..crate::Material::unvaried()
            }]
            .into_boxed_slice(),
            model_templates: Box::new([]),
            model_random_offsets: Box::new([]),
            model_quads: Box::new([]),
            animations: Box::new([]),
            animation_frames: Box::new([]),
            texture_pages: vec![TexturePage::new(TextureArray { layers: 1, mips })]
                .into_boxed_slice(),
            overlay_texture_source_sizes: Box::new([]),
            biomes: CompiledBiomeAssets::diagnostic(),
            provenance: BlobProvenance::ZEROED,
            missing: AtomicU64::new(0),
        }
    }

    #[must_use]
    pub fn resolve(&self, mode: NetworkIdMode, value: u32) -> ResolvedBlock {
        let index = match mode {
            NetworkIdMode::Sequential => Some(value),
            NetworkIdMode::Hashed => self.sequential_id_for_hash(value),
        };
        let visual = index.and_then(|index| {
            self.visuals
                .get(index as usize)
                .copied()
                .zip(self.light_properties.get(index as usize).copied())
        });
        visual.map_or_else(
            || {
                self.record_missing();
                ResolvedBlock::diagnostic()
            },
            |(visual, light)| ResolvedBlock::known(visual, light),
        )
    }

    /// Texture pixels and animation frames do not change the mesh's material addresses.
    pub fn has_same_geometry(&self, other: &Self) -> bool {
        self.visuals == other.visuals
            && self.hashed == other.hashed
            && self.model_templates == other.model_templates
            && self.model_random_offsets == other.model_random_offsets
            && self.model_quads == other.model_quads
            && self.materials.len() == other.materials.len()
            && self
                .materials
                .iter()
                .zip(other.materials.iter())
                .all(|(a, b)| a.flags == b.flags)
    }

    /// Admitted source pixels per layer, independently of the physical texture-array size.
    #[must_use]
    pub fn texture_source_size(&self, reference: TextureRef) -> [u32; 2] {
        if reference.page() == 1
            && let Some(size) = self
                .overlay_texture_source_sizes
                .get(reference.layer() as usize)
        {
            return size.map(u32::from);
        }
        let size = self
            .texture_pages
            .get(reference.page() as usize)
            .and_then(|page| page.texture.mips.first())
            .map_or(crate::TILE_SIZE, |mip| mip.size);
        [size; 2]
    }

    /// Returns terrain mips in admitted source pixels, rebuilding legacy carrier art as needed.
    pub fn terrain_texture_page(
        &self,
        page: usize,
    ) -> Result<std::borrow::Cow<'_, TextureArray>, crate::AssetError> {
        let texture = &self
            .texture_pages
            .get(page)
            .ok_or_else(|| crate::AssetError::InvalidCompiledAssets {
                detail: "terrain texture page is out of range".into(),
            })?
            .texture;
        if page == 1 && !self.overlay_texture_source_sizes.is_empty() {
            return Ok(std::borrow::Cow::Borrowed(texture));
        }
        crate::rebuild_legacy_terrain_mips(texture).map(std::borrow::Cow::Owned)
    }

    /// Number of materials in the carrier's table.
    #[must_use]
    pub fn material_count(&self) -> usize {
        self.materials.len()
    }

    /// True for the programmatic diagnostic runtime, which carries no registries.
    #[must_use]
    pub fn is_diagnostic(&self) -> bool {
        self.provenance == BlobProvenance::ZEROED
    }

    /// Returns whether the registry knows a network id, without counting misses.
    /// The diagnostic runtime has no registry and knows every id.
    #[must_use]
    pub fn is_known(&self, mode: NetworkIdMode, value: u32) -> bool {
        if self.is_diagnostic() {
            return true;
        }
        let index = match mode {
            NetworkIdMode::Sequential => Some(value),
            NetworkIdMode::Hashed => self.sequential_id_for_hash(value),
        };
        index.is_some_and(|index| (index as usize) < self.visuals.len())
    }

    /// Returns the exact sequential identity paired with a validated network
    /// hash. Coverage tooling uses this rather than visual equality because
    /// distinct states may intentionally share byte-identical visuals.
    #[must_use]
    pub fn sequential_id_for_hash(&self, network_hash: u32) -> Option<u32> {
        self.hashed
            .binary_search_by_key(&network_hash, |entry| entry.0)
            .ok()
            .map(|index| self.hashed[index].1)
    }

    /// Returns the unique network identity marked as air by the validated
    /// runtime registry. The lookup is bounded by the decoded visual and hash
    /// table limits and fails closed when either identity is ambiguous.
    #[must_use]
    pub fn air_network_id(&self, mode: NetworkIdMode) -> Option<u32> {
        let mut air_visuals = self.visuals.iter().enumerate().filter(|(_, visual)| {
            visual.flags.contains(BlockFlags::AIR)
                && visual.contributor_role == ContributorRole::Air
        });
        let sequential_id = u32::try_from(air_visuals.next()?.0).ok()?;
        if air_visuals.next().is_some() {
            return None;
        }
        if mode == NetworkIdMode::Sequential {
            return Some(sequential_id);
        }

        let mut air_hashes = self
            .hashed
            .iter()
            .filter(|(_, mapped_id)| *mapped_id == sequential_id)
            .map(|(network_hash, _)| *network_hash);
        let network_hash = air_hashes.next()?;
        if air_hashes.next().is_some() {
            return None;
        }
        Some(network_hash)
    }

    /// Number of sequential visual records in the validated runtime blob.
    #[must_use]
    pub const fn visual_count(&self) -> usize {
        self.visuals.len()
    }

    /// Number of unique network-hash mappings in the validated runtime blob.
    #[must_use]
    pub const fn hashed_count(&self) -> usize {
        self.hashed.len()
    }

    #[must_use]
    pub fn material(&self, id: u32) -> Material {
        self.materials.get(id as usize).copied().unwrap_or_else(|| {
            self.record_missing();
            self.materials[0]
        })
    }
    #[must_use]
    pub const fn materials(&self) -> &[Material] {
        &self.materials
    }
    /// State-owned component for a template; ordinary carrier templates have no override.
    #[must_use]
    pub fn model_random_offset(
        &self,
        template: u32,
    ) -> Option<block_transform::random_offset::RandomOffsetComponent> {
        self.model_random_offsets
            .binary_search_by_key(&template, |entry| entry.0)
            .ok()
            .map(|index| self.model_random_offsets[index].1)
    }

    #[must_use]
    pub const fn model_templates(&self) -> &[ModelTemplate] {
        &self.model_templates
    }
    #[must_use]
    pub const fn model_quads(&self) -> &[ModelQuad] {
        &self.model_quads
    }
    #[must_use]
    pub const fn animations(&self) -> &[Animation] {
        &self.animations
    }
    #[must_use]
    pub const fn animation_frames(&self) -> &[TextureRef] {
        &self.animation_frames
    }
    #[must_use]
    pub const fn texture_pages(&self) -> &[TexturePage] {
        &self.texture_pages
    }
    #[must_use]
    pub const fn texture_array(&self) -> &TextureArray {
        &self.texture_pages[0].texture
    }
    #[must_use]
    pub const fn biome_assets(&self) -> &CompiledBiomeAssets {
        &self.biomes
    }

    /// Returns the exact source identities embedded by the compiler: the
    /// canonical vanilla source manifest plus each consumed registry input.
    /// Startup compares these against the checkout-pinned expectations and
    /// rejects stale or foreign carriers. The programmatic diagnostic runtime
    /// carries [`BlobProvenance::ZEROED`] because it claims no source.
    #[must_use]
    pub const fn provenance(&self) -> &BlobProvenance {
        &self.provenance
    }
    #[must_use]
    pub fn missing_count(&self) -> u64 {
        self.missing.load(Ordering::Relaxed)
    }
    fn record_missing(&self) {
        self.missing.fetch_add(1, Ordering::Relaxed);
    }
}
