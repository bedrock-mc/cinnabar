//! The expanded Iris vertex: its word layout, the vertex buffer layouts a translated program's
//! inputs bind to, and the constant stream for attributes terrain does not carry.

use bevy::mesh::VertexBufferLayout;
use bevy::render::render_resource::{
    VertexAttribute as GpuVertexAttribute, VertexFormat, VertexStepMode,
};
use shaderpack::{ScalarType, StoredNumbers, ValueType, VertexAttribute, VertexInput};

/// One stored vertex field: its first word in the expanded vertex and the format it is
/// written in. The expansion kernels receive these words as generated WGSL constants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StoredField {
    pub(crate) placeholder: &'static str,
    pub(crate) word: u32,
    pub(crate) format: VertexFormat,
}

const fn field(placeholder: &'static str, word: u32, format: VertexFormat) -> StoredField {
    StoredField {
        placeholder,
        word,
        format,
    }
}

/// Position relative to the geometry origin (`gl_Vertex` / `vaPosition`).
pub(crate) const POSITION: StoredField = field("IRIS_WORD_POSITION", 0, VertexFormat::Float32x3);
/// Gamma tint times AO, or AO in alpha under `separateAo` (`gl_Color`).
pub(crate) const COLOR: StoredField = field("IRIS_WORD_COLOR", 3, VertexFormat::Unorm8x4);
/// Atlas texture coordinates (`gl_MultiTexCoord0` / `vaUV0`).
pub(crate) const UV0: StoredField = field("IRIS_WORD_UV0", 4, VertexFormat::Unorm16x2);
/// Mean atlas coordinates of the face (`mc_midTexCoord`).
pub(crate) const MID_TEX: StoredField = field("IRIS_WORD_MID_TEX", 5, VertexFormat::Unorm16x2);
/// Java lightmap coordinates, light level × 16 for block then sky (`gl_MultiTexCoord1/2`).
/// Half floats hold them exactly; the translator converts integer declarations
/// (`in ivec2 vaUV2`) from this one copy. Wide layouts only: the packed layout keeps them
/// in the packed position's `w` ([`TerrainLayout`]).
pub(crate) const LIGHTMAP: StoredField = field("IRIS_WORD_LIGHTMAP", 6, VertexFormat::Float16x2);
/// Face normal (`gl_Normal` / `vaNormal`).
pub(crate) const NORMAL: StoredField = field("IRIS_WORD_NORMAL", 7, VertexFormat::Snorm8x4);
/// Tangent with handedness in `w` (`at_tangent`).
pub(crate) const TANGENT: StoredField = field("IRIS_WORD_TANGENT", 8, VertexFormat::Snorm8x4);
/// `block.properties` id, -1 when unmapped, and Iris's render type: 1 for fluids, -1 for
/// every other block (`mc_Entity.xy`), as the signed shorts Iris stores; the translator
/// converts float declarations.
pub(crate) const BLOCK_ID: StoredField = field("IRIS_WORD_BLOCK_ID", 9, VertexFormat::Sint16x2);
/// Offset to the block centre in 1/64 blocks, truncated, and the block's emission
/// (`at_midBlock`), as the signed bytes Iris stores; the translator converts float
/// declarations.
pub(crate) const MID_BLOCK: StoredField = field("IRIS_WORD_MID_BLOCK", 10, VertexFormat::Sint8x4);
/// Iris's `mc_Entity.y` for everything but fluids (`ExtendedDataHelper.BLOCK_RENDER_TYPE`);
/// the liquid kernel writes `FLUID_RENDER_TYPE`, 1.
const BLOCK_RENDER_TYPE: i16 = -1;

/// Every stored field, in word order.
pub(crate) const STORED_FIELDS: [StoredField; 9] = [
    POSITION, COLOR, UV0, MID_TEX, LIGHTMAP, NORMAL, TANGENT, BLOCK_ID, MID_BLOCK,
];

/// Words of one expanded vertex.
pub const IRIS_VERTEX_WORDS: u32 = {
    let last = STORED_FIELDS[STORED_FIELDS.len() - 1];
    last.word + (last.format.size() / 4) as u32
};
/// Bytes between consecutive expanded vertices.
pub const IRIS_VERTEX_STRIDE: u64 = IRIS_VERTEX_WORDS as u64 * 4;

const _: () = {
    let mut index = 1;
    while index < STORED_FIELDS.len() {
        let previous = STORED_FIELDS[index - 1];
        assert!(previous.word + (previous.format.size() / 4) as u32 == STORED_FIELDS[index].word);
        index += 1;
    }
};

/// Vertex buffer slot of the expanded geometry stream.
pub const GEOMETRY_VERTEX_BUFFER: u32 = 0;
/// Vertex buffer slot of the constant stream: a zero-stride buffer that supplies Iris's
/// fixed values for attributes terrain does not carry.
pub const CONSTANT_VERTEX_BUFFER: u32 = 1;

/// Words of the constant stream: zero floats, zero integers, chunk fade, overlay coordinates.
pub(crate) const CONSTANT_STREAM_WORDS: [u32; 13] = [
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    1.0_f32.to_bits(),
    0,
    NO_OVERLAY[1] as u32,
    0,
    (NO_OVERLAY[1] as f32).to_bits(),
];
const ZERO_FLOAT_WORD: u32 = 0;
const ZERO_INT_WORD: u32 = 4;
const CHUNK_FADE_WORD: u32 = 8;
const OVERLAY_INT_WORD: u32 = 9;
const OVERLAY_FLOAT_WORD: u32 = 11;
/// Java's `OverlayTexture.NO_OVERLAY` coordinates (`vaUV1`): no hurt tint, no flash.
const NO_OVERLAY: [i32; 2] = [0, 10];

/// One vertex the host builds on the CPU (sky geometry) in the expanded stream's layout:
/// `position`, gamma `colour` and `normal`; texture and lightmap coordinates read zero,
/// `mc_Entity` Iris's (-1, -1) and `at_midBlock` zero.
pub(crate) fn host_vertex(
    position: [f32; 3],
    colour: [f32; 4],
    normal: [f32; 3],
) -> [u32; IRIS_VERTEX_WORDS as usize] {
    let unorm8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    let snorm8 = |v: f32| ((v.clamp(-1.0, 1.0) * 127.0).round() as i32 as u32) & 0xff;
    let mut words = [0_u32; IRIS_VERTEX_WORDS as usize];
    for (axis, value) in position.into_iter().enumerate() {
        words[POSITION.word as usize + axis] = value.to_bits();
    }
    words[COLOR.word as usize] = colour
        .into_iter()
        .enumerate()
        .map(|(channel, v)| unorm8(v) << (8 * channel))
        .sum();
    words[NORMAL.word as usize] = normal
        .into_iter()
        .enumerate()
        .map(|(axis, v)| snorm8(v) << (8 * axis))
        .sum();
    let short = |v: i16| u32::from(v as u16);
    words[BLOCK_ID.word as usize] = short(-1) | (short(BLOCK_RENDER_TYPE) << 16);
    words
}

/// A host vertex that also carries texture coordinates `uv` (with `mid_uv` as
/// `mc_midTexCoord`) and Java's raw lightmap coordinates `light` (block, sky; light level
/// × 16, which `LIGHTMAP_TEXTURE_MATRIX` maps to texel centres).
#[expect(dead_code, reason = "consumed by the shader-pack effects")]
pub(crate) fn textured_host_vertex(
    position: [f32; 3],
    colour: [f32; 4],
    normal: [f32; 3],
    uv: [f32; 2],
    mid_uv: [f32; 2],
    light: [f32; 2],
) -> [u32; IRIS_VERTEX_WORDS as usize] {
    let unorm16 = |v: f32| (v.clamp(0.0, 1.0) * 65535.0).round() as u32;
    let pair = |[a, b]: [u32; 2]| (a & 0xffff) | (b << 16);
    let mut words = host_vertex(position, colour, normal);
    words[UV0.word as usize] = pair(uv.map(unorm16));
    words[MID_TEX.word as usize] = pair(mid_uv.map(unorm16));
    words[LIGHTMAP.word as usize] = pair(light.map(|v| u32::from(f32_to_half(v))));
    words
}

/// Why a translated program's vertex input cannot bind to terrain geometry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VertexLayoutError {
    /// The input is declared with a type terrain vertices are not stored in.
    UnsupportedType { name: String, ty: ValueType },
    /// The input reads a field the vertex layout leaves out.
    NotStored { name: String },
}

impl std::fmt::Display for VertexLayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedType { name, ty } => {
                write!(f, "vertex input `{name}` has unsupported type {ty:?}")
            }
            Self::NotStored { name } => {
                write!(
                    f,
                    "vertex input `{name}` reads a field the layout leaves out"
                )
            }
        }
    }
}

impl std::error::Error for VertexLayoutError {}

/// Vertex buffer slot of the per-draw origin of packed positions: an instance-step buffer
/// of one `[f32; 4]` per resident section, which each terrain draw selects through its
/// first instance.
pub const DRAW_ORIGIN_VERTEX_BUFFER: u32 = 2;
/// Bytes of one per-draw origin.
pub(crate) const DRAW_ORIGIN_BYTES: u64 = 16;
/// Steps per block of a packed position component.
const PACKED_POSITION_STEPS: f32 = 2048.0;
/// Blocks below the section origin that a packed position component's zero stands for, so
/// the 16-bit range covers −8 to 24 blocks around the section.
const PACKED_POSITION_BIAS: i32 = 8;
/// Absent field in [`TerrainLayout::expand_words`].
const ABSENT_WORD: u32 = 0xff;

/// The expanded terrain vertex of one pack: which fields are stored, at which words, and
/// whether positions are packed.
///
/// The packed layout ([`Self::compact`]) stores the other fields some terrain program reads
/// in their [`STORED_FIELDS`] formats (so they reach the shader exactly as from the wide
/// layout), and the position as four `u16`: section-local components in 1/2048 blocks
/// from −8 (`PACKED_POSITION_BIAS`), with the lightmap coordinates in `w` (block then sky,
/// a byte each, level × 16 rounded as Java stores them). The translator unpacks every
/// position input as `float + origin.xyz + packed.xyz * origin.w` and every lightmap input
/// as `float + packed.w`'s bytes: packed draws read float zero and the section's
/// [`draw_origin`] per draw ([`DRAW_ORIGIN_VERTEX_BUFFER`]); wide draws (sky, effects,
/// actors, and the [`Self::WIDE`] terrain layout) read their float position and lightmap
/// and zero for the rest. Positions on the 1/2048 grid (every cube face and every model
/// element on Java's 1/16 grid) come out identical to float positions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TerrainLayout {
    packed: bool,
    /// Word of each of [`STORED_FIELDS`], in that order; the position's when not packed.
    words: [Option<u8>; STORED_FIELDS.len()],
    /// Word of the packed position.
    packed_word: Option<u8>,
    stride_words: u8,
}

impl TerrainLayout {
    /// Every field at its [`STORED_FIELDS`] word, positions as floats: the stream the host
    /// builds for sky, effects and actors.
    pub const WIDE: Self = {
        let mut words = [None; STORED_FIELDS.len()];
        let mut index = 0;
        while index < STORED_FIELDS.len() {
            words[index] = Some(STORED_FIELDS[index].word as u8);
            index += 1;
        }
        Self {
            packed: false,
            words,
            packed_word: None,
            stride_words: IRIS_VERTEX_WORDS as u8,
        }
    };

    /// The packed layout holding what `inputs` (every terrain program's vertex inputs) read.
    pub fn compact<'a>(inputs: impl IntoIterator<Item = &'a VertexInput>) -> Self {
        let mut needed = [false; STORED_FIELDS.len()];
        for input in inputs {
            let Some((class, _)) = class_and_size(input.ty) else {
                continue;
            };
            if let Some(Some(field)) = stored_field(&input.attribute, class)
                && let Some(index) = STORED_FIELDS.iter().position(|f| *f == field)
            {
                needed[index] = true;
            }
        }
        let mut words = [None; STORED_FIELDS.len()];
        // The packed position takes the first two words, the lightmap its `w`.
        let mut next = 2_u8;
        for (index, field) in STORED_FIELDS.iter().enumerate() {
            if *field == POSITION || *field == LIGHTMAP || !needed[index] {
                continue;
            }
            words[index] = Some(next);
            next += (field.format.size() / 4) as u8;
        }
        Self {
            packed: true,
            words,
            packed_word: Some(0),
            stride_words: next,
        }
    }

    /// Whether positions are packed relative to each draw's section.
    pub fn packed(&self) -> bool {
        self.packed
    }

    /// Bytes between consecutive vertices.
    pub fn stride(&self) -> u64 {
        u64::from(self.stride_words) * 4
    }

    /// The word `field` is stored at, if stored.
    pub(crate) fn word(&self, field: StoredField) -> Option<u32> {
        let index = STORED_FIELDS.iter().position(|f| *f == field)?;
        self.words[index].map(u32::from)
    }

    /// The kernels' `ExpandParams::words`: stride and the word of every field, a byte each
    /// (`ABSENT_WORD` when not stored), in `expand_common.wesl`'s order.
    pub(crate) fn expand_words(&self) -> [u32; 4] {
        let byte = |word: Option<u32>| word.unwrap_or(ABSENT_WORD);
        let pack =
            |bytes: [u32; 4]| bytes[0] | (bytes[1] << 8) | (bytes[2] << 16) | (bytes[3] << 24);
        let field = |field: StoredField| byte(self.word(field));
        [
            pack([
                u32::from(self.stride_words),
                field(POSITION),
                byte(self.packed_word.map(u32::from)),
                field(COLOR),
            ]),
            pack([field(UV0), field(MID_TEX), field(LIGHTMAP), field(NORMAL)]),
            pack([
                field(TANGENT),
                field(BLOCK_ID),
                field(MID_BLOCK),
                ABSENT_WORD,
            ]),
            0,
        ]
    }
}

/// The per-draw origin of packed positions of a section at block `section` when vertex
/// positions are relative to `anchor`: where the packed zero lies, and the step.
pub(crate) fn draw_origin(section: [i32; 3], anchor: [i32; 3]) -> [f32; 4] {
    let axis = |index: usize| (section[index] - anchor[index] - PACKED_POSITION_BIAS) as f32;
    [axis(0), axis(1), axis(2), PACKED_POSITION_STEPS.recip()]
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Float,
    Sint,
    Uint,
}

/// Accepts the 32-bit component formats supplied by the terrain vertex streams.
fn class_and_size(ty: ValueType) -> Option<(Class, u8)> {
    let (scalar, size) = match ty {
        ValueType::Scalar(scalar) => (scalar, 1),
        ValueType::Vector { size, scalar } => (scalar, size),
        ValueType::Matrix { .. } => return None,
    };
    let class = match scalar {
        ScalarType::Float => Class::Float,
        ScalarType::Int => Class::Sint,
        ScalarType::Uint => Class::Uint,
        _ => return None,
    };
    Some((class, size))
}

/// The stored field an input reads from the expanded stream, `Some(None)` when the constant
/// stream supplies it, or `None` when it cannot be read as declared. Stored fields are read
/// as the kind of number they hold, which the translator's conversions
/// ([`VertexAttribute::stored_numbers`]) make every declaration read. The packed position
/// parts are not fields; [`input_fetches`] places them.
fn stored_field(attribute: &VertexAttribute, class: Class) -> Option<Option<StoredField>> {
    let stored_class = match attribute.stored_numbers() {
        Some(StoredNumbers::Int) => Class::Sint,
        Some(StoredNumbers::Float) | None => Class::Float,
    };
    let stored = |field: StoredField| (class == stored_class).then_some(Some(field));
    match attribute {
        VertexAttribute::Position => stored(POSITION),
        VertexAttribute::Color => stored(COLOR),
        VertexAttribute::TexCoord => stored(UV0),
        VertexAttribute::MidTexCoord => stored(MID_TEX),
        // Iris documents gl_MultiTexCoord2 as a second name for the lightmap coordinates.
        VertexAttribute::Lightmap | VertexAttribute::MultiTexCoord2 => stored(LIGHTMAP),
        VertexAttribute::Normal => stored(NORMAL),
        VertexAttribute::Tangent => stored(TANGENT),
        VertexAttribute::BlockId => stored(BLOCK_ID),
        VertexAttribute::MidBlock => stored(MID_BLOCK),
        VertexAttribute::MultiTexCoord3
        | VertexAttribute::Overlay
        | VertexAttribute::Velocity
        | VertexAttribute::ChunkFade
        | VertexAttribute::DhMaterialId
        | VertexAttribute::PackedPosition
        | VertexAttribute::DrawOrigin
        | VertexAttribute::Other(_) => Some(None),
    }
}

/// Format and word of a constant-stream input with `size` components of `class`.
fn constant_format(attribute: &VertexAttribute, class: Class, size: u8) -> (u32, VertexFormat) {
    use VertexFormat as F;
    let word = match (attribute, class) {
        (VertexAttribute::ChunkFade, Class::Float) => CHUNK_FADE_WORD,
        (VertexAttribute::Overlay, Class::Float) => OVERLAY_FLOAT_WORD,
        (VertexAttribute::Overlay, _) => OVERLAY_INT_WORD,
        (_, Class::Float) => ZERO_FLOAT_WORD,
        _ => ZERO_INT_WORD,
    };
    // Overlay and chunk fade carry their own component counts; zeros take the declared one.
    // The packed layout's lightmap reads two zeros, as the wide layout's two stored halves,
    // so a `vec4` declaration still gets GL's `(…, 0, 1)` and texture-matrix offsets apply.
    let size = match attribute {
        VertexAttribute::ChunkFade => 1,
        VertexAttribute::Overlay => size.min(2),
        VertexAttribute::Lightmap | VertexAttribute::MultiTexCoord2 => size.min(2),
        _ => size,
    };
    let format = match (class, size) {
        (Class::Float, 1) => F::Float32,
        (Class::Float, 2) => F::Float32x2,
        (Class::Float, 3) => F::Float32x3,
        (Class::Float, _) => F::Float32x4,
        (Class::Sint, 1) => F::Sint32,
        (Class::Sint, 2) => F::Sint32x2,
        (Class::Sint, 3) => F::Sint32x3,
        (Class::Sint, _) => F::Sint32x4,
        (Class::Uint, 1) => F::Uint32,
        (Class::Uint, 2) => F::Uint32x2,
        (Class::Uint, 3) => F::Uint32x3,
        (Class::Uint, _) => F::Uint32x4,
    };
    (word, format)
}

/// Where one program input is fetched from: `buffer` is [`GEOMETRY_VERTEX_BUFFER`] (the
/// expanded stream), [`CONSTANT_VERTEX_BUFFER`] or [`DRAW_ORIGIN_VERTEX_BUFFER`], read at
/// `word` in `format`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InputFetch {
    pub(crate) location: u32,
    pub(crate) buffer: u32,
    pub(crate) word: u32,
    pub(crate) format: VertexFormat,
}

/// The fetch of every input in `inputs` from a stream in `layout`, as
/// [`vertex_buffer_layouts`] binds them.
pub(crate) fn input_fetches(
    inputs: &[VertexInput],
    layout: &TerrainLayout,
) -> Result<Vec<InputFetch>, VertexLayoutError> {
    inputs
        .iter()
        .map(|input| {
            let unsupported = || VertexLayoutError::UnsupportedType {
                name: input.name.clone(),
                ty: input.ty,
            };
            let (class, size) = class_and_size(input.ty).ok_or_else(unsupported)?;
            let constant = || {
                (
                    CONSTANT_VERTEX_BUFFER,
                    constant_format(&input.attribute, class, size),
                )
            };
            let (buffer, (word, format)) = match (&input.attribute, layout.packed) {
                (VertexAttribute::Position, true)
                | (VertexAttribute::Lightmap | VertexAttribute::MultiTexCoord2, true) => constant(),
                (VertexAttribute::PackedPosition, true) if class == Class::Uint => (
                    GEOMETRY_VERTEX_BUFFER,
                    (
                        layout.packed_word.map_or(0, u32::from),
                        VertexFormat::Uint16x4,
                    ),
                ),
                (VertexAttribute::DrawOrigin, true) if class == Class::Float => {
                    (DRAW_ORIGIN_VERTEX_BUFFER, (0, VertexFormat::Float32x4))
                }
                (VertexAttribute::PackedPosition | VertexAttribute::DrawOrigin, true) => {
                    return Err(unsupported());
                }
                (attribute, _) => match stored_field(attribute, class).ok_or_else(unsupported)? {
                    Some(field) => {
                        let word =
                            layout
                                .word(field)
                                .ok_or_else(|| VertexLayoutError::NotStored {
                                    name: input.name.clone(),
                                })?;
                        (GEOMETRY_VERTEX_BUFFER, (word, field.format))
                    }
                    None => constant(),
                },
            };
            Ok(InputFetch {
                location: input.location,
                buffer,
                word,
                format,
            })
        })
        .collect()
}

/// Vertex buffer layouts binding a translated program's `inputs` (from
/// `ProgramReflection::vertex_inputs`) to a stream in `layout`: slot
/// [`GEOMETRY_VERTEX_BUFFER`] is the expanded stream, slot [`CONSTANT_VERTEX_BUFFER`] the
/// zero-stride constant stream and, for packed layouts, slot [`DRAW_ORIGIN_VERTEX_BUFFER`]
/// the per-draw origins. Fewer stored components than an input declares read as GL's
/// `(0, 0, 0, 1)` defaults.
pub fn vertex_buffer_layouts(
    inputs: &[VertexInput],
    layout: &TerrainLayout,
) -> Result<Vec<VertexBufferLayout>, VertexLayoutError> {
    let mut buffers = vec![
        VertexBufferLayout {
            array_stride: layout.stride(),
            step_mode: VertexStepMode::Vertex,
            attributes: vec![],
        },
        VertexBufferLayout {
            array_stride: 0,
            step_mode: VertexStepMode::Vertex,
            attributes: vec![],
        },
    ];
    if layout.packed {
        buffers.push(VertexBufferLayout {
            array_stride: DRAW_ORIGIN_BYTES,
            step_mode: VertexStepMode::Instance,
            attributes: vec![],
        });
    }
    for fetch in input_fetches(inputs, layout)? {
        buffers[fetch.buffer as usize]
            .attributes
            .push(GpuVertexAttribute {
                format: fetch.format,
                offset: u64::from(fetch.word) * 4,
                shader_location: fetch.location,
            });
    }
    Ok(buffers)
}

/// Replaces the expansion kernels' layout placeholders with this module's words.
pub(crate) fn substitute_layout_constants(source: &str) -> String {
    let mut source = source.replace("IRIS_VERTEX_WORDS", &format!("{IRIS_VERTEX_WORDS}u"));
    for field in STORED_FIELDS {
        source = source.replace(field.placeholder, &format!("{}u", field.word));
    }
    source
}

/// The binary16 bits of `v`, truncating the mantissa; out-of-range values become infinity or
/// signed zero.
pub(crate) fn f32_to_half(v: f32) -> u16 {
    let bits = v.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mantissa = bits & 0x7f_ffff;
    if v.is_nan() {
        return sign | 0x7e00;
    }
    if exponent >= 31 {
        return sign | 0x7c00;
    }
    if exponent <= 0 {
        if exponent < -10 {
            return sign;
        }
        let m = (mantissa | 0x80_0000) >> (1 - exponent);
        return sign | (m >> 13) as u16;
    }
    sign | ((exponent as u16) << 10) | (mantissa >> 13) as u16
}

#[cfg(test)]
#[path = "vertex_tests.rs"]
mod tests;
