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
//! Java world identities use the pinned Bedrock→Java mappings: [`ShaderPack::id_maps`]
//! reads a pack's block, item and entity ids and [`IdMaps`] resolves them for Bedrock
//! blocks, entities and items; [`BiomeUniforms`] and the `BIOME_*`/`CAT_*`/`PPT_*` macros
//! describe Java biomes; [`sky_fog`] gives Java's sky and fog colours.
//!
//! The player's settings follow Iris: [`OptionSession`] edits values through the pack's
//! [`OptionMenu`] with [`PackLang`] text, and [`write_option_file`] / [`read_option_file`]
//! keep them in `<pack>.txt` beside the pack ([`option_file`]).
//!
//! Render-host integration and in-game acceptance remain incomplete.
//! [`CustomUniforms`] evaluates a pack's custom uniform and variable expressions each frame;
//! nothing uploads or renders them yet.
mod biomes;
mod buffers;
mod compile;
mod custom_uniforms;
mod diagnostics;
mod dimensions;
mod discover;
mod environment;
mod expr;
mod ids;
mod install;
mod java_sky;
mod lang;
mod option_file;
mod option_menu;
mod option_session;
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

pub use biomes::{BiomeCategory, BiomeUniforms};
pub use buffers::{BufferDirectives, BufferFormat, BufferId, BufferSettings};
pub use compile::CompiledStage;
pub use custom_uniforms::{CustomUniformError, CustomUniforms, UniformValue};
pub use diagnostics::{Diagnostic, Phase, SourceLocation};
pub use discover::{DiscoveredPack, discover_packs};
pub use environment::{Environment, GpuVendor, HostOs};
pub use expr::{BinaryOp, Expr, ExprError, UnaryOp};
pub use ids::{BlockIdMap, IdMaps, NamedIdMap, RenderLayer, UNMAPPED_BLOCK_ID};
pub use install::{DropOutcome, add_dropped_packs};
pub use java_sky::{BiomeColors, FogMedium, SkyFogColors, SkyFogInputs, sky_fog, sunrise_color};
pub use lang::{DEFAULT_LANGUAGE, PackLang};
pub use option_file::{
    format_option_file, option_file, parse_option_file, read_option_file, write_option_file,
};
pub use option_menu::{MenuElement, MenuScreen, OptionMenu, Profile, ProfileScan};
pub use option_session::OptionSession;
pub use options::{OptionKind, OptionValues, PackOption, normalize_values};
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
    SHADOW_SAMPLER_NAME, STORAGE_BUFFER_GROUP, SampleType, SamplerBinding, SamplerClass,
    ScalarType, StageMask, StorageImageBinding, StoredNumbers, TEXTURE_GROUP, TextureBinding,
    TextureDimension, UNIFORM_GROUP, UniformBlock, UniformMember, ValueType, VertexAttribute,
    VertexInput,
};
pub use source::{PackError, PackOrigin, PackSource};
pub use translate::{ProgramTranslation, StageTranslation};
