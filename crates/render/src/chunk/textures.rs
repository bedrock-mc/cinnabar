use crate::chunk::*;

/// Authored Enhanced texture layers. The arrays are sparse: `texture_refs`
/// maps the normal 16x16 atlas references to layers in these optional pages.
/// Missing entries keep the generated material fallback, so a pack can ship
/// PBR data for only the blocks it actually authors.
#[derive(Clone, Debug)]
pub struct EnhancedTextureAssets {
    pub(crate) color_pages: [TextureArray; 2],
    pub(crate) normal_pages: [TextureArray; 2],
    pub(crate) mer_pages: [TextureArray; 2],
    pub(crate) texture_refs: Box<[u32]>,
}

impl EnhancedTextureAssets {
    /// Creates a sparse authored set. `texture_refs` is indexed by the packed
    /// page/layer (page * 2048 + layer), with `u32::MAX` selecting fallback.
    #[must_use]
    pub fn new(
        color_pages: [TextureArray; 2],
        normal_pages: [TextureArray; 2],
        mer_pages: [TextureArray; 2],
        texture_refs: Box<[u32]>,
    ) -> Option<Self> {
        if texture_refs.len() != assets::MAX_TEXTURE_PAGES * assets::MAX_TEXTURE_LAYERS
            || color_pages
                .iter()
                .zip(normal_pages.iter())
                .zip(mer_pages.iter())
                .any(|((color, normal), mer)| {
                    color.layers != normal.layers || color.layers != mer.layers
                })
            || texture_refs.iter().any(|&texture_ref| {
                if texture_ref == u32::MAX {
                    return false;
                }
                let page = (texture_ref >> 31) as usize;
                let layer = (texture_ref & 0x7ff) as usize;
                page >= assets::MAX_TEXTURE_PAGES || layer >= color_pages[page].layers as usize
            })
        {
            return None;
        }
        Some(Self {
            color_pages,
            normal_pages,
            mer_pages,
            texture_refs,
        })
    }

    #[must_use]
    pub fn authored_layer_count(&self) -> usize {
        self.color_pages
            .iter()
            .enumerate()
            .filter(|(page, _)| {
                self.texture_refs
                    .iter()
                    .any(|&reference| reference != u32::MAX && (reference >> 31) as usize == *page)
            })
            .map(|(_, color)| color.layers as usize)
            .sum()
    }

    /// Returns optional pages in color, normal, and material binding order.
    pub(in crate::chunk) fn pages(&self) -> [&TextureArray; 6] {
        [
            &self.color_pages[0],
            &self.color_pages[1],
            &self.normal_pages[0],
            &self.normal_pages[1],
            &self.mer_pages[0],
            &self.mer_pages[1],
        ]
    }

    /// Checks complete optional pages and their reference table independently of the carrier.
    #[must_use]
    pub fn fits_device_limits(&self, limits: &wgpu::Limits) -> bool {
        let array_limits = TextureArrayLimits {
            max_layers: limits.max_texture_array_layers,
            max_dimension_2d: limits.max_texture_dimension_2d,
        };
        let references = std::mem::size_of_val(self.texture_refs.as_ref()) as u64;
        references <= limits.max_buffer_size
            && references <= u64::from(limits.max_storage_buffer_binding_size)
            && self.pages().into_iter().all(|page| {
                let Some(first) = page.mips.first() else {
                    return false;
                };
                page.layers > 0
                    && first.size.is_power_of_two()
                    && page.mips.len() == first.size.ilog2() as usize + 1
                    && array_limits.validate(page.layers, first.size).is_ok()
                    && page.mips.iter().enumerate().all(|(index, mip)| {
                        mip.size == first.size >> index
                            && (mip.size as usize)
                                .checked_mul(mip.size as usize)
                                .and_then(|bytes| bytes.checked_mul(page.layers as usize))
                                .and_then(|bytes| bytes.checked_mul(4))
                                == Some(mip.rgba8.len())
                    })
            })
    }
}

/// Immutable assets selected for the single global chunk texture array.
#[derive(Resource, Clone)]
pub struct ChunkTextureAssets {
    pub(in crate::chunk) assets: Arc<RuntimeAssets>,
    pub(in crate::chunk) enhanced: Option<Arc<EnhancedTextureAssets>>,
    pub(in crate::chunk) revision: u64,
}

impl Default for ChunkTextureAssets {
    fn default() -> Self {
        Self::new(Arc::new(RuntimeAssets::diagnostic()))
    }
}

impl ChunkTextureAssets {
    #[must_use]
    pub const fn new(assets: Arc<RuntimeAssets>) -> Self {
        Self {
            assets,
            enhanced: None,
            revision: 0,
        }
    }

    #[must_use]
    pub const fn with_revision(assets: Arc<RuntimeAssets>, revision: u64) -> Self {
        Self {
            assets,
            enhanced: None,
            revision,
        }
    }

    #[must_use]
    pub fn with_optional_enhanced(
        assets: Arc<RuntimeAssets>,
        enhanced: Option<Arc<EnhancedTextureAssets>>,
        revision: u64,
    ) -> Self {
        Self {
            assets,
            enhanced,
            revision,
        }
    }

    #[must_use]
    pub fn with_enhanced(
        assets: Arc<RuntimeAssets>,
        enhanced: Arc<EnhancedTextureAssets>,
        revision: u64,
    ) -> Self {
        Self::with_optional_enhanced(assets, Some(enhanced), revision)
    }

    #[must_use]
    pub fn assets(&self) -> &Arc<RuntimeAssets> {
        &self.assets
    }

    #[must_use]
    pub fn enhanced(&self) -> Option<&Arc<EnhancedTextureAssets>> {
        self.enhanced.as_ref()
    }

    /// Replace optional authored maps without changing server asset ownership.
    #[must_use]
    pub fn with_updated_enhanced(&self, enhanced: Option<Arc<EnhancedTextureAssets>>) -> Self {
        Self {
            assets: Arc::clone(&self.assets),
            enhanced,
            revision: self.revision,
        }
    }

    #[must_use]
    pub fn identity(&self) -> ChunkTextureAssetIdentity {
        ChunkTextureAssetIdentity {
            pointer: Arc::as_ptr(&self.assets) as usize,
            enhanced_pointer: self
                .enhanced
                .as_ref()
                .map_or(0, |enhanced| Arc::as_ptr(enhanced) as usize),
            revision: self.revision,
        }
    }
}

impl bevy::render::extract_resource::ExtractResource for ChunkTextureAssets {
    type Source = Self;

    fn extract_resource(source: &Self::Source) -> Self {
        Self {
            assets: Arc::clone(&source.assets),
            enhanced: source.enhanced.as_ref().map(Arc::clone),
            revision: source.revision,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkTextureAssetIdentity {
    pub(in crate::chunk) pointer: usize,
    pub(in crate::chunk) enhanced_pointer: usize,
    pub(in crate::chunk) revision: u64,
}

impl ChunkTextureAssetIdentity {
    #[cfg(test)]
    #[must_use]
    pub(in crate::chunk) const fn new(pointer: usize, revision: u64) -> Self {
        Self {
            pointer,
            enhanced_pointer: 0,
            revision,
        }
    }
}

#[must_use]
pub fn texture_asset_needs_rebuild(
    current: Option<ChunkTextureAssetIdentity>,
    next: ChunkTextureAssetIdentity,
) -> bool {
    current != Some(next)
}

pub(in crate::chunk) const ANIMATION_TICKS_PER_SECOND: f64 = 20.0;
pub(in crate::chunk) const ANIMATION_TICK_MODULUS: f64 = u32::MAX as f64 + 1.0;

/// Global Bedrock flipbook clock. Only this 16-byte value changes per frame;
/// texture pages and animation tables remain immutable for an asset revision.
#[repr(C)]
#[derive(
    Resource, Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable, ShaderType,
)]
pub struct ChunkAnimationClock {
    pub(in crate::chunk) tick: u32,
    pub(in crate::chunk) partial_tick: f32,
    pub(in crate::chunk) _padding_0: u32,
    pub(in crate::chunk) _padding_1: u32,
}

pub(in crate::chunk) const _: () = assert!(std::mem::size_of::<ChunkAnimationClock>() == 16);

impl Default for ChunkAnimationClock {
    fn default() -> Self {
        Self::from_parts(0, 0.0)
    }
}

impl ChunkAnimationClock {
    #[must_use]
    pub fn from_parts(tick: u32, partial_tick: f32) -> Self {
        Self {
            tick,
            partial_tick: if partial_tick.is_finite() {
                partial_tick.clamp(0.0, 0.999_999_94)
            } else {
                0.0
            },
            _padding_0: 0,
            _padding_1: 0,
        }
    }

    #[must_use]
    pub fn from_elapsed_seconds(elapsed_seconds: f64) -> Self {
        let elapsed_seconds = if elapsed_seconds.is_finite() {
            elapsed_seconds.max(0.0)
        } else {
            0.0
        };
        let elapsed_ticks = elapsed_seconds * ANIMATION_TICKS_PER_SECOND;
        let whole_ticks = elapsed_ticks.floor();
        Self::from_parts(
            whole_ticks.rem_euclid(ANIMATION_TICK_MODULUS) as u32,
            elapsed_ticks.fract() as f32,
        )
    }

    #[must_use]
    pub const fn tick(self) -> u32 {
        self.tick
    }

    #[must_use]
    pub const fn partial_tick(self) -> f32 {
        self.partial_tick
    }
}

impl bevy::render::extract_resource::ExtractResource for ChunkAnimationClock {
    type Source = Self;

    fn extract_resource(source: &Self::Source) -> Self {
        *source
    }
}

/// Resolved current/next physical texture references for one material.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimationFrameSample {
    pub current: TextureRef,
    pub next: TextureRef,
    pub blend: f32,
}

impl AnimationFrameSample {
    #[must_use]
    pub const fn new(current: TextureRef, next: TextureRef, blend: f32) -> Self {
        Self {
            current,
            next,
            blend,
        }
    }
}

/// CPU oracle for the exact bounded frame-selection arithmetic used by WGSL.
#[must_use]
pub fn select_animation_frames(
    material: Material,
    animations: &[Animation],
    frames: &[TextureRef],
    clock: ChunkAnimationClock,
) -> AnimationFrameSample {
    let static_sample = || AnimationFrameSample::new(material.texture, material.texture, 0.0);
    if material.animation == NO_ANIMATION {
        return static_sample();
    }
    let Some(animation) = animations.get(material.animation as usize) else {
        return static_sample();
    };
    if animation.frame_count == 0 || animation.ticks_per_frame == 0 {
        return static_sample();
    }
    let current_index = (clock.tick / animation.ticks_per_frame) % animation.frame_count;
    let Some(current_offset) = animation.frame_start.checked_add(current_index) else {
        return static_sample();
    };
    let Some(&current) = frames.get(current_offset as usize) else {
        return static_sample();
    };
    if animation.flags & ANIMATION_FLAG_BLEND == 0 || animation.frame_count == 1 {
        return AnimationFrameSample::new(current, current, 0.0);
    }
    let next_index = (current_index + 1) % animation.frame_count;
    let Some(next_offset) = animation.frame_start.checked_add(next_index) else {
        return static_sample();
    };
    let Some(&next) = frames.get(next_offset as usize) else {
        return static_sample();
    };
    let blend = (clock.tick % animation.ticks_per_frame) as f32 + clock.partial_tick;
    AnimationFrameSample::new(current, next, blend / animation.ticks_per_frame as f32)
}

/// Source assigned to each of the two physical texture bindings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TexturePageBinding {
    Asset(usize),
    DiagnosticFallback,
}

#[must_use]
pub const fn plan_texture_page_bindings(page_count: usize) -> Option<[TexturePageBinding; 2]> {
    match page_count {
        1 => Some([
            TexturePageBinding::Asset(0),
            TexturePageBinding::DiagnosticFallback,
        ]),
        2 => Some([TexturePageBinding::Asset(0), TexturePageBinding::Asset(1)]),
        _ => None,
    }
}

/// Copies physical layer zero from every mip into the real one-layer page bound
/// as page one when an asset contains only page zero.
pub fn diagnostic_texture_page(
    texture: &TextureArray,
) -> Result<TextureArray, TextureUploadPlanError> {
    if texture.layers == 0 {
        return Err(TextureUploadPlanError::InvalidMipBytes);
    }
    let mut mips = Vec::with_capacity(texture.mips.len());
    for mip in &texture.mips {
        let side = usize::try_from(mip.size).map_err(|_| TextureUploadPlanError::SizeOverflow)?;
        let layer_bytes = side
            .checked_mul(side)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(TextureUploadPlanError::SizeOverflow)?;
        let expected = layer_bytes
            .checked_mul(texture.layers as usize)
            .ok_or(TextureUploadPlanError::SizeOverflow)?;
        if mip.rgba8.len() != expected {
            return Err(TextureUploadPlanError::InvalidMipBytes);
        }
        mips.push(TextureMip {
            size: mip.size,
            rgba8: mip.rgba8[..layer_bytes].to_vec().into_boxed_slice(),
        });
    }
    Ok(TextureArray {
        layers: 1,
        mips: mips.into_boxed_slice(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureArrayLimits {
    pub max_layers: u32,
    pub max_dimension_2d: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureLimitError {
    Layers { requested: u32, supported: u32 },
    Dimension { requested: u32, supported: u32 },
}

impl TextureArrayLimits {
    pub fn validate(self, layers: u32, dimension: u32) -> Result<(), TextureLimitError> {
        if layers > self.max_layers {
            return Err(TextureLimitError::Layers {
                requested: layers,
                supported: self.max_layers,
            });
        }
        if dimension > self.max_dimension_2d {
            return Err(TextureLimitError::Dimension {
                requested: dimension,
                supported: self.max_dimension_2d,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureMipUploadPlan {
    pub mip_level: u32,
    pub size: u32,
    pub bytes_per_row: u32,
    pub rows_per_image: u32,
    pub layer_source_offsets: Box<[usize]>,
    pub layer_staging_offsets: Box<[usize]>,
    pub staging_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureUploadPlanError {
    ZeroAlignment,
    SizeOverflow,
    InvalidMipBytes,
}

pub fn plan_texture_mip_uploads(
    texture: &TextureArray,
    row_alignment: usize,
) -> Result<Vec<TextureMipUploadPlan>, TextureUploadPlanError> {
    if row_alignment == 0 {
        return Err(TextureUploadPlanError::ZeroAlignment);
    }
    let layers =
        usize::try_from(texture.layers).map_err(|_| TextureUploadPlanError::SizeOverflow)?;
    texture
        .mips
        .iter()
        .enumerate()
        .map(|(mip_level, mip)| {
            let size =
                usize::try_from(mip.size).map_err(|_| TextureUploadPlanError::SizeOverflow)?;
            let row_bytes = size
                .checked_mul(4)
                .ok_or(TextureUploadPlanError::SizeOverflow)?;
            let bytes_per_row = row_bytes
                .checked_add(row_alignment - 1)
                .map(|value| value / row_alignment * row_alignment)
                .ok_or(TextureUploadPlanError::SizeOverflow)?;
            let source_layer_bytes = row_bytes
                .checked_mul(size)
                .ok_or(TextureUploadPlanError::SizeOverflow)?;
            let staging_layer_bytes = bytes_per_row
                .checked_mul(size)
                .ok_or(TextureUploadPlanError::SizeOverflow)?;
            let expected = source_layer_bytes
                .checked_mul(layers)
                .ok_or(TextureUploadPlanError::SizeOverflow)?;
            if mip.rgba8.len() != expected {
                return Err(TextureUploadPlanError::InvalidMipBytes);
            }
            let layer_source_offsets = (0..layers)
                .map(|layer| layer * source_layer_bytes)
                .collect::<Vec<_>>()
                .into_boxed_slice();
            let layer_staging_offsets = (0..layers)
                .map(|layer| layer * staging_layer_bytes)
                .collect::<Vec<_>>()
                .into_boxed_slice();
            Ok(TextureMipUploadPlan {
                mip_level: u32::try_from(mip_level)
                    .map_err(|_| TextureUploadPlanError::SizeOverflow)?,
                size: mip.size,
                bytes_per_row: u32::try_from(bytes_per_row)
                    .map_err(|_| TextureUploadPlanError::SizeOverflow)?,
                rows_per_image: mip.size,
                layer_source_offsets,
                layer_staging_offsets,
                staging_bytes: staging_layer_bytes
                    .checked_mul(layers)
                    .ok_or(TextureUploadPlanError::SizeOverflow)?,
            })
        })
        .collect()
}

#[must_use]
pub fn greedy_texture_uv(face: Face, corner: u32, width: u32, height: u32, flags: u32) -> [f32; 2] {
    let width = width as f32;
    let height = height as f32;
    // Match the native face axes in chunk.wgsl before applying pack transforms.
    let horizontal_standard = [[0.0, height], [width, height], [width, 0.0], [0.0, 0.0]];
    let horizontal_transposed = [[0.0, 0.0], [0.0, height], [width, height], [width, 0.0]];
    let vertical_standard = [[0.0, height], [width, height], [width, 0.0], [0.0, 0.0]];
    let vertical_transposed = [[width, height], [width, 0.0], [0.0, 0.0], [0.0, height]];
    let corner = (corner & 3) as usize;
    let [mut u, mut v] = match face {
        Face::NegativeX | Face::PositiveZ => vertical_standard[corner],
        Face::PositiveX | Face::NegativeZ => vertical_transposed[corner],
        Face::NegativeY => horizontal_standard[corner],
        Face::PositiveY => horizontal_transposed[corner],
    };
    let (extent_u, extent_v) = match flags & MATERIAL_UV_ROTATION_MASK {
        MATERIAL_UV_ROTATE_90 => {
            (u, v) = (v, width - u);
            (height, width)
        }
        MATERIAL_UV_ROTATE_180 => {
            (u, v) = (width - u, height - v);
            (width, height)
        }
        MATERIAL_UV_ROTATE_270 => {
            (u, v) = (height - v, u);
            (height, width)
        }
        _ => (width, height),
    };
    if flags & MATERIAL_UV_REFLECT_U != 0 {
        u = extent_u - u;
    }
    if flags & MATERIAL_UV_REFLECT_V != 0 {
        v = extent_v - v;
    }
    [u, v]
}
