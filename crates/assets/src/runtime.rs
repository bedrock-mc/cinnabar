mod decode;

use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
};

use crate::{
    Animation, AssetError, BlockFace, BlockFlags, BlockVisual, CompiledBiomeAssets,
    ContributorRole, DIAGNOSTIC_MATERIAL, LightProperties, Material, ModelQuad, ModelTemplate,
    NO_ANIMATION, NO_MODEL_TEMPLATE, TextureArray, TextureMip, TexturePage, TextureRef, VisualKind,
    VisualSupport,
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

    /// Returns an immutable session asset set with selected 16x16 texture
    /// layers replaced by server-provided pixels. The compiled block
    /// topology, materials, animations, and registries remain unchanged;
    /// only the physical texture pages are copied and updated.
    pub fn with_texture_overrides(
        &self,
        replacements: &BTreeMap<TextureRef, Box<[u8]>>,
    ) -> Result<Self, AssetError> {
        let mut texture_pages = self.texture_pages.to_vec();
        for (reference, rgba8) in replacements {
            if rgba8.len() != (crate::TILE_SIZE * crate::TILE_SIZE * 4) as usize {
                return Err(AssetError::InvalidCompiledAssets {
                    detail: format!(
                        "texture override for reference {:#010x} is not a 16x16 RGBA8 tile",
                        reference.raw()
                    )
                    .into_boxed_str(),
                });
            }
            let page_index = usize::try_from(reference.page()).map_err(|_| {
                AssetError::InvalidCompiledAssets {
                    detail: "texture override page index overflows usize".into(),
                }
            })?;
            let layer = usize::try_from(reference.layer()).map_err(|_| {
                AssetError::InvalidCompiledAssets {
                    detail: "texture override layer index overflows usize".into(),
                }
            })?;
            let page = texture_pages.get_mut(page_index).ok_or_else(|| {
                AssetError::InvalidCompiledAssets {
                    detail: format!(
                        "texture override page {} is not present in the runtime assets",
                        reference.page()
                    )
                    .into_boxed_str(),
                }
            })?;
            if layer >= page.texture.layers as usize {
                return Err(AssetError::InvalidCompiledAssets {
                    detail: format!(
                        "texture override layer {} is not present on page {}",
                        reference.layer(),
                        reference.page()
                    )
                    .into_boxed_str(),
                });
            }
            let mips = texture_override_mips(rgba8)?;
            for (mip_index, mip) in mips.iter().enumerate() {
                let side: usize =
                    usize::try_from(mip.size).map_err(|_| AssetError::InvalidCompiledAssets {
                        detail: "texture override mip size overflows usize".into(),
                    })?;
                let layer_bytes = side
                    .checked_mul(side)
                    .and_then(|pixels| pixels.checked_mul(4))
                    .ok_or_else(|| AssetError::InvalidCompiledAssets {
                        detail: "texture override mip byte count overflows usize".into(),
                    })?;
                let offset = layer.checked_mul(layer_bytes).ok_or_else(|| {
                    AssetError::InvalidCompiledAssets {
                        detail: "texture override layer offset overflows usize".into(),
                    }
                })?;
                let destination = page.texture.mips.get_mut(mip_index).ok_or_else(|| {
                    AssetError::InvalidCompiledAssets {
                        detail: "texture override has more mip levels than the runtime page".into(),
                    }
                })?;
                let end = offset.checked_add(layer_bytes).ok_or_else(|| {
                    AssetError::InvalidCompiledAssets {
                        detail: "texture override mip range overflows usize".into(),
                    }
                })?;
                if destination.size != mip.size || end > destination.rgba8.len() {
                    return Err(AssetError::InvalidCompiledAssets {
                        detail: "texture override mip dimensions do not match the runtime page"
                            .into(),
                    });
                }
                destination.rgba8[offset..end].copy_from_slice(&mip.rgba8);
            }
        }
        Ok(Self {
            visuals: self.visuals.clone(),
            light_properties: self.light_properties.clone(),
            hashed: self.hashed.clone(),
            materials: self.materials.clone(),
            model_templates: self.model_templates.clone(),
            model_quads: self.model_quads.clone(),
            animations: self.animations.clone(),
            animation_frames: self.animation_frames.clone(),
            texture_pages: texture_pages.into_boxed_slice(),
            biomes: self.biomes.clone(),
            missing: AtomicU64::new(self.missing_count()),
        })
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

fn texture_override_mips(rgba8: &[u8]) -> Result<Box<[TextureMip]>, AssetError> {
    let expected = (crate::TILE_SIZE * crate::TILE_SIZE * 4) as usize;
    if rgba8.len() != expected {
        return Err(AssetError::InvalidCompiledAssets {
            detail: "texture override is not a 16x16 RGBA8 tile".into(),
        });
    }
    let mut mips = Vec::with_capacity(crate::MIP_COUNT as usize);
    let mut current = rgba8.to_vec();
    let mut size = crate::TILE_SIZE as usize;
    loop {
        mips.push(TextureMip {
            size: u32::try_from(size).map_err(|_| AssetError::InvalidCompiledAssets {
                detail: "texture override mip size overflows u32".into(),
            })?,
            rgba8: current.clone().into_boxed_slice(),
        });
        if size == 1 {
            break;
        }
        let next_size = size / 2;
        let mut next = vec![0; next_size * next_size * 4];
        for y in 0..next_size {
            for x in 0..next_size {
                let mut alpha_sum = 0u32;
                let mut color_sum = [0u32; 3];
                for source_y in 0..2 {
                    for source_x in 0..2 {
                        let source = ((y * 2 + source_y) * size + x * 2 + source_x) * 4;
                        let alpha = u32::from(current[source + 3]);
                        alpha_sum += alpha;
                        for (channel, sum) in color_sum.iter_mut().enumerate() {
                            *sum += u32::from(current[source + channel]) * alpha;
                        }
                    }
                }
                let destination = (y * next_size + x) * 4;
                let alpha = (alpha_sum / 4).min(255);
                next[destination + 3] = alpha as u8;
                for (channel, sum) in color_sum.into_iter().enumerate() {
                    next[destination + channel] = if alpha_sum == 0 {
                        0
                    } else {
                        (sum / alpha_sum).min(255) as u8
                    };
                }
            }
        }
        current = next;
        size = next_size;
    }
    Ok(mips.into_boxed_slice())
}
