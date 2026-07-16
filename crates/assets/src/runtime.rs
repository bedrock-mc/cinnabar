mod decode;

use std::sync::atomic::{AtomicU64, Ordering};

use crate::{
    Animation, BlockFace, BlockFlags, BlockVisual, CompiledBiomeAssets, ContributorRole,
    DIAGNOSTIC_MATERIAL, LightProperties, Material, ModelQuad, ModelTemplate, NO_ANIMATION,
    NO_MODEL_TEMPLATE, TextureArray, TextureMip, TexturePage, TextureRef, VisualKind,
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
    model_quads: Box<[ModelQuad]>,
    animations: Box<[Animation]>,
    animation_frames: Box<[TextureRef]>,
    texture_pages: Box<[TexturePage]>,
    biomes: CompiledBiomeAssets,
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
            }]
            .into_boxed_slice(),
            model_templates: Box::new([]),
            model_quads: Box::new([]),
            animations: Box::new([]),
            animation_frames: Box::new([]),
            texture_pages: vec![TexturePage::new(TextureArray { layers: 1, mips })]
                .into_boxed_slice(),
            biomes: CompiledBiomeAssets::diagnostic(),
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
    #[must_use]
    pub fn missing_count(&self) -> u64 {
        self.missing.load(Ordering::Relaxed)
    }
    fn record_missing(&self) {
        self.missing.fetch_add(1, Ordering::Relaxed);
    }
}
