//! Java Edition (OptiFine/Iris) shader packs: loading, configuration and translation to
//! validated naga modules with a binding contract for the render host.
//!
//! Flow: [`ShaderPack::open`] a folder or zip → read [`ShaderPack::options`] and pick values →
//! [`ShaderPack::translate`] a dimension with a [`TranslateConfig`] → bind each
//! [`ProgramTranslation`]'s naga modules ([`CompiledStage`]) using its [`ProgramReflection`].
//!
//! Translation per stage: Iris-style `#include` expansion with option overrides, the standard
//! Iris macros ([`Environment`]), full preprocessing with pp-rs, a compatibility-profile to
//! GLSL 4.50 rewrite (legacy built-ins, separate samplers, fixed bindings, interface
//! locations shared between stages), then naga's GLSL frontend and validator. Errors are
//! [`Diagnostic`]s located in the pack's own files; a failing program never stops the others.

mod buffers;
mod compile;
mod diagnostics;
mod environment;
mod expr;
mod options;
mod pack;
mod preprocess;
mod programs;
mod properties;
mod reflect;
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
    AlphaTest, AlphaTestRule, BlendFactor, BlendMode, BlendRule, BufferSize, CompareFunction,
    CustomImage, CustomTexture, CustomUniform, CustomUniformKind, FlipRule, ImageSize, ProgramRule,
    PropertiesDiagnostic, ScaleRule, ShaderProperties, SizeValue, TextureSource,
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
