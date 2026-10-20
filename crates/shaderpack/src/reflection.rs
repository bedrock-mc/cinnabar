//! The binding contract of a translated program: what the render host must supply (vertex
//! attributes, one uniform block, textures, samplers, images, buffers) and what the program
//! writes (render targets), with every `(group, binding)` fixed by the translator.
//!
//! Binding layout, shared by every program:
//! - group 0, binding 0: the program's loose uniforms as one std140 block ([`UniformBlock`]);
//!   further pack-declared uniform blocks follow from binding 1.
//! - group 1: binding 0 is the shared filtering sampler, binding 1 the shared comparison
//!   sampler; sampled textures start at binding 2, numbered per program by first declaration.
//! - group 2: storage images from binding 0.
//! - group 3: storage buffers at the pack's own `binding`.

use crate::buffers::BufferFormat;
use crate::programs::{ProgramKind, StageKind};

/// Group of the uniform blocks.
pub const UNIFORM_GROUP: u32 = 0;
/// Binding of the loose-uniform block in [`UNIFORM_GROUP`].
pub const LOOSE_UNIFORM_BINDING: u32 = 0;
/// Group of samplers and sampled textures.
pub const TEXTURE_GROUP: u32 = 1;
/// Binding of the shared filtering sampler in [`TEXTURE_GROUP`].
pub const SAMPLER_BINDING: u32 = 0;
/// Binding of the shared depth-comparison sampler in [`TEXTURE_GROUP`].
pub const SHADOW_SAMPLER_BINDING: u32 = 1;
/// First sampled-texture binding in [`TEXTURE_GROUP`].
pub const FIRST_TEXTURE_BINDING: u32 = 2;
/// Group of storage images.
pub const IMAGE_GROUP: u32 = 2;
/// Group of storage buffers.
pub const STORAGE_BUFFER_GROUP: u32 = 3;
/// Name of the loose-uniform block in translated GLSL.
pub const LOOSE_UNIFORM_BLOCK: &str = "IrisUniforms";
/// Name of the shared filtering sampler in translated GLSL.
pub const SAMPLER_NAME: &str = "iris_sampler";
/// Name of the shared comparison sampler in translated GLSL.
pub const SHADOW_SAMPLER_NAME: &str = "iris_samplerShadow";

/// Fixed-function state of the compatibility profile, supplied as members of the loose
/// uniform block: (GLSL built-in, block member name, member declaration).
pub const LEGACY_UNIFORMS: &[(&str, &str, &str)] = &[
    (
        "gl_ModelViewMatrix",
        "iris_ModelViewMat",
        "mat4 iris_ModelViewMat;",
    ),
    ("gl_ProjectionMatrix", "iris_ProjMat", "mat4 iris_ProjMat;"),
    (
        "gl_ModelViewMatrixInverse",
        "iris_ModelViewMatInverse",
        "mat4 iris_ModelViewMatInverse;",
    ),
    (
        "gl_ProjectionMatrixInverse",
        "iris_ProjMatInverse",
        "mat4 iris_ProjMatInverse;",
    ),
    ("gl_NormalMatrix", "iris_NormalMat", "mat3 iris_NormalMat;"),
    (
        "gl_TextureMatrix",
        "iris_TextureMat",
        "mat4 iris_TextureMat[4];",
    ),
    ("gl_Fog", "iris_Fog", "iris_FogParameters iris_Fog;"),
];

/// Declares supplied vertex inputs, names and location order together.
macro_rules! vertex_attributes {
    ($( $(#[$doc:meta])* $kind:ident => [$($name:literal),+]),+ $(,)?) => {
        /// A per-vertex input the host's vertex buffers must provide.
        #[derive(Clone, Debug, PartialEq, Eq, Hash)]
        pub enum VertexAttribute {
            $($(#[$doc])* $kind,)+
            /// An input the pack declares that Iris does not supply.
            Other(String),
        }

        /// GLSL input aliases in location order.
        const ATTRIBUTE_NAMES: &[(&str, VertexAttribute)] = &[
            $($(($name, VertexAttribute::$kind),)+)+
        ];

        /// Supplied attributes in location order.
        const SUPPLIED: &[VertexAttribute] = &[$(VertexAttribute::$kind),+];
    };
}

vertex_attributes! {
    /// `gl_Vertex` (vec4) or `vaPosition` (vec3).
    Position => ["iris_Vertex", "vaPosition"],
    /// `gl_Color` or `vaColor`.
    Color => ["iris_Color", "vaColor"],
    /// `gl_Normal` or `vaNormal`.
    Normal => ["iris_Normal", "vaNormal"],
    /// `gl_MultiTexCoord0` or `vaUV0`: the texture atlas coordinate.
    TexCoord => ["iris_MultiTexCoord0", "vaUV0"],
    /// `gl_MultiTexCoord1` or `vaUV2`: the lightmap coordinate.
    Lightmap => ["iris_MultiTexCoord1", "vaUV2"],
    /// `gl_MultiTexCoord2`.
    MultiTexCoord2 => ["iris_MultiTexCoord2"],
    /// `gl_MultiTexCoord3`.
    MultiTexCoord3 => ["iris_MultiTexCoord3"],
    /// `vaUV1`: the entity overlay coordinate.
    Overlay => ["vaUV1"],
    /// `mc_Entity`: block or entity id from `block.properties`.
    BlockId => ["mc_Entity"],
    /// `mc_midTexCoord`: centre of the sprite.
    MidTexCoord => ["mc_midTexCoord"],
    /// `at_tangent`.
    Tangent => ["at_tangent"],
    /// `at_midBlock`: offset to the block centre and light emission.
    MidBlock => ["at_midBlock"],
    /// `at_velocity`.
    Velocity => ["at_velocity"],
    /// `mc_chunkFade`.
    ChunkFade => ["mc_chunkFade"],
    /// `dhMaterialId`.
    DhMaterialId => ["dhMaterialId"],
}

/// First location used for [`VertexAttribute::Other`] inputs.
pub const FIRST_OTHER_ATTRIBUTE_LOCATION: u32 = SUPPLIED.len() as u32;

impl VertexAttribute {
    /// The attribute a translated vertex input name stands for.
    pub fn from_glsl_name(name: &str) -> VertexAttribute {
        ATTRIBUTE_NAMES
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, a)| a.clone())
            .unwrap_or_else(|| VertexAttribute::Other(name.to_string()))
    }

    /// The fixed shader location of a supplied attribute; `None` for `Other`.
    pub fn location(&self) -> Option<u32> {
        SUPPLIED.iter().position(|a| a == self).map(|i| i as u32)
    }
}

/// Scalar component type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScalarType {
    Bool,
    Int8,
    Int16,
    Int,
    Int64,
    Uint8,
    Uint16,
    Uint,
    Uint64,
    Half,
    Float,
    Double,
}

/// A value type in an interface or uniform block. Bool uniforms are stored as `Int`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ValueType {
    Scalar(ScalarType),
    Vector {
        size: u8,
        scalar: ScalarType,
    },
    Matrix {
        columns: u8,
        rows: u8,
        scalar: ScalarType,
    },
}

/// A vertex input of the program's vertex stage.
#[derive(Clone, Debug, PartialEq)]
pub struct VertexInput {
    pub location: u32,
    /// The input's name in translated GLSL.
    pub name: String,
    pub attribute: VertexAttribute,
    pub ty: ValueType,
}

/// Which stages of a program use a binding.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct StageMask {
    pub vertex: bool,
    pub fragment: bool,
    pub compute: bool,
}

impl StageMask {
    #[allow(
        dead_code,
        reason = "wired into ShaderPack::translate later in the translator stack"
    )]
    /// Marks a stage as using the reflected resource.
    pub(crate) fn add(&mut self, stage: StageKind) {
        match stage {
            StageKind::Fragment => self.fragment = true,
            StageKind::Compute => self.compute = true,
            _ => self.vertex = true,
        }
    }
}

/// One leaf of a uniform block: arrays of structs are expanded to `name[i].member`.
#[derive(Clone, Debug, PartialEq)]
pub struct UniformMember {
    /// Path inside the block, e.g. `frameTimeCounter`, `iris_Fog.color`, `lights[2]`.
    pub name: String,
    /// Byte offset from the block start.
    pub offset: u32,
    pub ty: ValueType,
    /// Element count and byte stride for arrays of scalars, vectors or matrices.
    pub array: Option<(u32, u32)>,
}

/// A std140 uniform block. The loose-uniform block is identical in every stage of a program.
#[derive(Clone, Debug, PartialEq)]
pub struct UniformBlock {
    pub name: String,
    pub group: u32,
    pub binding: u32,
    /// Total size in bytes (the minimum buffer binding size).
    pub size: u32,
    pub members: Vec<UniformMember>,
    pub stages: StageMask,
}

/// Texture shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextureDimension {
    D1,
    D1Array,
    D2,
    D2Array,
    D3,
    Cube,
    CubeArray,
}

/// How a sampled texture is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SampleType {
    Float,
    Sint,
    Uint,
    /// Depth texture sampled with the comparison sampler (`sampler2DShadow`).
    Depth,
}

/// A sampled texture (`sampler2D colortex0` becomes a separate `texture2D`).
#[derive(Clone, Debug, PartialEq)]
pub struct TextureBinding {
    /// The pack's sampler name (`colortex0`, `shadowtex1`, `noisetex`, a custom texture …).
    pub name: String,
    pub group: u32,
    pub binding: u32,
    pub dimension: TextureDimension,
    pub sample_type: SampleType,
    pub multisampled: bool,
    pub stages: StageMask,
}

/// One of the two shared samplers.
#[derive(Clone, Debug, PartialEq)]
pub struct SamplerBinding {
    pub name: String,
    pub group: u32,
    pub binding: u32,
    pub comparison: bool,
    pub stages: StageMask,
}

/// A storage image (`image2D`, `uimage3D` …).
#[derive(Clone, Debug, PartialEq)]
pub struct StorageImageBinding {
    pub name: String,
    pub group: u32,
    pub binding: u32,
    pub dimension: TextureDimension,
    pub format: naga::StorageFormat,
    pub read: bool,
    pub write: bool,
    pub stages: StageMask,
}

/// A pack-declared uniform or storage buffer block.
#[derive(Clone, Debug, PartialEq)]
pub struct BufferBinding {
    pub name: String,
    pub group: u32,
    pub binding: u32,
    /// Byte size of the fixed part (the minimum binding size).
    pub min_size: u32,
    /// Storage buffers only: no stage writes it.
    pub read_only: bool,
    pub stages: StageMask,
}

/// A fragment output and the colour buffer it lands in.
#[derive(Clone, Debug, PartialEq)]
pub struct ColorOutput {
    pub location: u32,
    /// The output's name in translated GLSL.
    pub name: String,
    pub ty: ValueType,
    /// The colortex (or, for shadow programs, shadowcolor) index from RENDERTARGETS or
    /// DRAWBUFFERS; `None` when the location is past the list (GL discards the write).
    pub buffer: Option<u32>,
    /// The buffer's format from its `colortexNFormat`/`shadowcolorNFormat` directive
    /// anywhere in the pack, or Iris's default.
    pub format: BufferFormat,
}

/// Compute dispatch shape.
#[derive(Clone, Debug, PartialEq)]
pub struct ComputeDispatch {
    /// `layout(local_size_x …)`.
    pub workgroup_size: [u32; 3],
    /// `const ivec3 workGroups`: a fixed dispatch size.
    pub work_groups: Option<[u32; 3]>,
    /// `const vec2 workGroupsRender`: dispatch size relative to the screen. When neither is
    /// declared Iris dispatches as `workGroupsRender = vec2(1.0, 1.0)`.
    pub work_groups_render: Option<[f32; 2]>,
}

/// Everything the host binds for one translated program.
#[derive(Clone, Debug, PartialEq)]
pub struct ProgramReflection {
    pub kind: ProgramKind,
    /// Stages with their entry point names, in pipeline order.
    pub entry_points: Vec<(StageKind, String)>,
    pub vertex_inputs: Vec<VertexInput>,
    /// The loose-uniform block, if any stage uses one.
    pub uniforms: Option<UniformBlock>,
    pub uniform_buffers: Vec<BufferBinding>,
    pub storage_buffers: Vec<BufferBinding>,
    pub textures: Vec<TextureBinding>,
    pub samplers: Vec<SamplerBinding>,
    pub storage_images: Vec<StorageImageBinding>,
    pub color_outputs: Vec<ColorOutput>,
    /// The fragment stage writes `gl_FragDepth`.
    pub writes_depth: bool,
    pub compute: Option<ComputeDispatch>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_named_attribute_has_a_location_and_every_location_a_name() {
        for (name, attribute) in ATTRIBUTE_NAMES {
            assert!(attribute.location().is_some(), "{name} has no location");
            assert_eq!(&VertexAttribute::from_glsl_name(name), attribute);
        }
        for attribute in SUPPLIED {
            assert!(
                ATTRIBUTE_NAMES.iter().any(|(_, a)| a == attribute),
                "{attribute:?} has no GLSL name"
            );
        }
        assert_eq!(
            VertexAttribute::from_glsl_name("myInput"),
            VertexAttribute::Other("myInput".to_string())
        );
        assert_eq!(VertexAttribute::Other("x".into()).location(), None);
    }
}
