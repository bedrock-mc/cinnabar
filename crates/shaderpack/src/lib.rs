//! Java Edition OptiFine/Iris shader-pack loading and configuration metadata.
//!
//! Open a pack with [`ShaderPack::open`], inspect [`ShaderPack::options`], select a
//! [`ShaderPack::dimension_folder`] and discover [`ShaderPack::programs`]. Chosen options
//! and an [`Environment`] supply the macros used by [`ShaderPack::properties`] and
//! [`ProgramRule::is_enabled`]. [`PackSource`] also exposes sorted text and bounded
//! binary asset reads from folders, ZIP archives or memory.
//!
//! Shader translation, render-host integration and in-game acceptance remain incomplete.
mod buffers;
#[allow(
    dead_code,
    reason = "wired into ShaderPack::translate later in the translator stack"
)]
mod compile;
#[allow(
    dead_code,
    reason = "wired into ShaderPack::translate later in the translator stack"
)]
mod diagnostics;
mod dimensions;
mod environment;
mod expr;
mod options;
mod pack;
#[allow(
    dead_code,
    reason = "wired into ShaderPack::translate later in the translator stack"
)]
mod preprocess;
mod programs;
mod properties;
#[allow(
    dead_code,
    reason = "wired into ShaderPack::translate later in the translator stack"
)]
mod reflect;
mod reflection;
mod source;
#[allow(
    dead_code,
    reason = "wired into ShaderPack::translate later in the translator stack"
)]
mod token;

pub use buffers::{BufferDirectives, BufferFormat, BufferId, BufferSettings};
pub use compile::CompiledStage;
pub use diagnostics::{Diagnostic, Phase, SourceLocation};
pub use environment::{Environment, GpuVendor, HostOs};
pub use expr::{BinaryOp, Expr, ExprError, UnaryOp};
pub use options::{OptionKind, OptionValues, PackOption};
pub use pack::ShaderPack;
pub use preprocess::ConstDirective;
pub use programs::{
    GEOMETRY_FALLBACKS, ProgramGroup, ProgramKind, ProgramSource, StageKind, geometry_fallback,
    program_group,
};
pub use properties::{CustomImage, ImageSize, ProgramRule, PropertiesDiagnostic, ShaderProperties};
pub use reflection::{
    BufferBinding, ColorOutput, ComputeDispatch, FIRST_OTHER_ATTRIBUTE_LOCATION,
    FIRST_TEXTURE_BINDING, IMAGE_GROUP, LEGACY_UNIFORMS, LOOSE_UNIFORM_BINDING,
    LOOSE_UNIFORM_BLOCK, ProgramReflection, SAMPLER_BINDING, SAMPLER_NAME, SHADOW_SAMPLER_BINDING,
    SHADOW_SAMPLER_NAME, STORAGE_BUFFER_GROUP, SampleType, SamplerBinding, ScalarType, StageMask,
    StorageImageBinding, TEXTURE_GROUP, TextureBinding, TextureDimension, UNIFORM_GROUP,
    UniformBlock, UniformMember, ValueType, VertexAttribute, VertexInput,
};
pub use source::{PackError, PackOrigin, PackSource};
