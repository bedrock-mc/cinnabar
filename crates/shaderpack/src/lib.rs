//! Java Edition OptiFine/Iris shader packs: loading, configuration metadata and translation
//! to validated naga modules with a binding contract for the render host.
//!
//! Open a pack with [`ShaderPack::open`], inspect [`ShaderPack::options`], select a
//! [`ShaderPack::dimension_folder`] and discover [`ShaderPack::programs`]. Chosen options
//! and an [`Environment`] supply the macros used by [`ShaderPack::properties`] and
//! [`ProgramRule::is_enabled`]. [`PackSource`] also exposes sorted text and bounded
//! binary asset reads from folders, ZIP archives or memory.
//!
//! [`ShaderPack::translate`] translates a folder's programs with a [`TranslateConfig`]; a host
//! binds each [`ProgramTranslation`]'s naga modules ([`CompiledStage`]) using its
//! [`ProgramReflection`]. Translation per stage: Iris-style `#include` expansion with option
//! overrides, the standard Iris macros, full preprocessing with pp-rs, a
//! compatibility-profile to GLSL 4.50 rewrite (legacy built-ins, separate samplers, fixed
//! bindings, interface locations shared between stages), then naga's GLSL frontend and
//! validator. Errors are [`Diagnostic`]s located in the pack's own files; a failing program
//! never stops the others.
//!
//! Render-host integration and in-game acceptance remain incomplete.
//! Parsing custom uniform expressions does not execute or render them.
mod buffers;
mod compile;
mod diagnostics;
mod dimensions;
mod environment;
mod expr;
mod options;
mod pack;
mod preprocess;
mod programs;
mod properties;
mod reflect;
mod reflect_link;
mod reflection;
mod source;
mod token;
mod transform;
mod translate;

pub use buffers::{BufferDirectives, BufferFormat, BufferId, BufferSettings};
pub use compile::CompiledStage;
pub use diagnostics::{Diagnostic, Phase, SourceLocation};
pub use environment::{Environment, GpuVendor, HostOs};
pub use expr::{BinaryOp, Expr, ExprError, UnaryOp};
pub use options::{OptionKind, OptionValues, PackOption};
pub use pack::{PackTranslation, ShaderPack, TranslateConfig};
pub use preprocess::ConstDirective;
pub use programs::{
    GEOMETRY_FALLBACKS, ProgramGroup, ProgramKind, ProgramSource, StageKind, geometry_fallback,
    program_group,
};
pub use properties::{
    BufferSize, CustomImage, CustomTexture, CustomUniform, CustomUniformKind, FlipRule, ImageSize,
    ProgramRule, PropertiesDiagnostic, ScaleRule, ShaderProperties, SizeValue, TextureSource,
};
pub use reflection::{
    BufferBinding, ColorOutput, ComputeDispatch, FIRST_OTHER_ATTRIBUTE_LOCATION,
    FIRST_TEXTURE_BINDING, IMAGE_GROUP, LEGACY_UNIFORMS, LOOSE_UNIFORM_BINDING,
    LOOSE_UNIFORM_BLOCK, ProgramReflection, SAMPLER_BINDING, SAMPLER_NAME, SHADOW_SAMPLER_BINDING,
    SHADOW_SAMPLER_NAME, STORAGE_BUFFER_GROUP, SampleType, SamplerBinding, ScalarType, StageMask,
    StorageImageBinding, TEXTURE_GROUP, TextureBinding, TextureDimension, UNIFORM_GROUP,
    UniformBlock, UniformMember, ValueType, VertexAttribute, VertexInput,
};
pub use source::{PackError, PackOrigin, PackSource};
pub use translate::{ProgramTranslation, StageTranslation};
