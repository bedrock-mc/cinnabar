use crate::chunk::*;

/// Immutable assets selected for the single global chunk texture array.
#[derive(Resource, Clone)]
pub struct ChunkTextureAssets {
    pub(in crate::chunk) assets: Arc<RuntimeAssets>,
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
            revision: 0,
        }
    }

    #[must_use]
    pub const fn with_revision(assets: Arc<RuntimeAssets>, revision: u64) -> Self {
        Self { assets, revision }
    }

    #[must_use]
    pub fn assets(&self) -> &Arc<RuntimeAssets> {
        &self.assets
    }

    #[must_use]
    pub fn identity(&self) -> ChunkTextureAssetIdentity {
        ChunkTextureAssetIdentity {
            pointer: Arc::as_ptr(&self.assets) as usize,
            revision: self.revision,
        }
    }
}

impl bevy::render::extract_resource::ExtractResource for ChunkTextureAssets {
    type Source = Self;

    fn extract_resource(source: &Self::Source) -> Self {
        Self {
            assets: Arc::clone(&source.assets),
            revision: source.revision,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkTextureAssetIdentity {
    pub(in crate::chunk) pointer: usize,
    pub(in crate::chunk) revision: u64,
}

impl ChunkTextureAssetIdentity {
    #[cfg(test)]
    #[must_use]
    pub(in crate::chunk) const fn new(pointer: usize, revision: u64) -> Self {
        Self { pointer, revision }
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
    let horizontal_standard = [[0.0, 0.0], [width, 0.0], [width, height], [0.0, height]];
    let horizontal_transposed = [[0.0, 0.0], [0.0, height], [width, height], [width, 0.0]];
    let vertical_standard = [[0.0, height], [width, height], [width, 0.0], [0.0, 0.0]];
    let vertical_transposed = [[0.0, height], [0.0, 0.0], [width, 0.0], [width, height]];
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
