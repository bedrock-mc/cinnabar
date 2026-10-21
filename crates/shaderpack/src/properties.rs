//! `shaders.properties`: the pack-wide settings file of OptiFine/Iris packs.
//!
//! The file is first run through a C-like conditional preprocessor (`#if`, `#ifdef`, ...),
//! then read as java.util.Properties. Program enable rules, buffer sizes, flips, scales, custom
//! texture bindings and custom images are typed; other keys stay in `entries`.
//! Malformed input never fails the whole file: it becomes a [`PropertiesDiagnostic`].

pub(crate) mod reader;
mod rules;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use crate::expr::{Expr, macro_value};
use crate::programs::ProgramGroup;
use crate::reflection::{SamplerClass, TextureDimension};
use crate::source::PackSource;

/// Parsed shaders.properties after conditional preprocessing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShaderProperties {
    /// Every key/value in file order after `#if` processing (later duplicates kept; typed
    /// accessors use the last occurrence, matching java.util.Properties).
    pub entries: Vec<(String, String)>,
    /// `program.[<dimension>/]<program>.enabled` rules, one per key.
    pub program_rules: Vec<ProgramRule>,
    /// `size.buffer.<buffer>` rules, one per key.
    pub buffer_sizes: Vec<BufferSize>,
    /// `flip.<program>.<buffer>` rules, one per key.
    pub flips: Vec<FlipRule>,
    /// `scale.<program>` rules, one per key.
    pub scales: Vec<ScaleRule>,
    /// `blend.<program>[.<buffer>]` overrides, one per key.
    pub blends: Vec<BlendRule>,
    /// `alphaTest.<program>` overrides, one per key.
    pub alpha_tests: Vec<AlphaTestRule>,
    /// `texture.<stage>.<sampler>` and `texture.noise` bindings, one per key.
    pub custom_textures: Vec<CustomTexture>,
    /// `image.<name>` declarations, one per key.
    pub images: Vec<CustomImage>,
    /// `uniform.<type>.<name>` and `variable.<type>.<name>` definitions, one per key.
    pub custom_uniforms: Vec<CustomUniform>,
    /// Malformed lines and expressions (never fatal), with 1-based line numbers.
    pub diagnostics: Vec<PropertiesDiagnostic>,
}

/// A malformed line, directive or value; the offending entry is skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertiesDiagnostic {
    /// 1-based line number (the first line of a continued logical line).
    pub line: u32,
    /// Human-readable description of the problem.
    pub message: String,
}

impl ShaderProperties {
    /// `text` is the raw file; `defines` maps macro names visible to `#if`/`#ifdef`
    /// (enabled boolean options → "", value options → value, standard Iris macros) to values.
    ///
    /// In `#if` conditions a macro evaluates to its value parsed as a number (int, float or
    /// hex), to 1 when its value is empty, and to 0 otherwise; undefined names are 0.
    /// `#define`/`#undef` in active regions affect later conditionals.
    pub fn parse(text: &str, defines: &BTreeMap<String, String>) -> ShaderProperties {
        let mut properties = ShaderProperties::default();
        let raw = reader::read_entries(text, defines, &mut properties.diagnostics);
        // Iris interprets java.util.Properties, so only each key's last value becomes a rule;
        // a malformed last value leaves no rule for that key.
        for entry in last_by_key(&raw, |entry| &entry.key) {
            rules::apply(&mut properties, entry.line, &entry.key, &entry.value);
        }
        properties.entries = raw
            .into_iter()
            .map(|entry| (entry.key, entry.value))
            .collect();
        properties
    }

    /// Each key once, at its first position with its last value, as java.util.Properties holds it.
    pub fn effective_entries(&self) -> Vec<(&str, &str)> {
        last_by_key(&self.entries, |(key, _)| key)
            .into_iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect()
    }

    /// Last value for `key`, trimmed.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .rev()
            .find(|(entry_key, _)| entry_key == key)
            .map(|(_, value)| value.as_str())
    }

    /// The enable rule for `program` in `dimension` (folder name like "world0"): a
    /// `program.<dimension>/<program>.enabled` rule wins over `program.<program>.enabled`.
    pub fn program_rule(&self, dimension: &str, program: &str) -> Option<&ProgramRule> {
        let for_program = || {
            self.program_rules
                .iter()
                .rev()
                .filter(move |rule| rule.program == program)
        };
        for_program()
            .find(|rule| rule.dimension.as_deref() == Some(dimension))
            .or_else(|| for_program().find(|rule| rule.dimension.is_none()))
    }
}

/// Keeps the last item for each key, ordered by the key's first occurrence.
pub(crate) fn last_by_key<T>(items: &[T], key: impl Fn(&T) -> &str) -> Vec<&T> {
    let mut positions = BTreeMap::new();
    let mut kept: Vec<&T> = Vec::new();
    for item in items {
        match positions.entry(key(item)) {
            std::collections::btree_map::Entry::Occupied(index) => kept[*index.get()] = item,
            std::collections::btree_map::Entry::Vacant(index) => {
                index.insert(kept.len());
                kept.push(item);
            }
        }
    }
    kept
}

/// `program.[<dimension>/]<program>.enabled=<expr>`: whether a program runs.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgramRule {
    /// Dimension folder (e.g. "world-1") when the key is dimension-qualified.
    pub dimension: Option<String>,
    /// Program name, e.g. "composite2" or "shadow".
    pub program: String,
    /// Condition over option macros; non-zero means enabled.
    pub expr: Expr,
    /// 1-based line of the rule.
    pub line: u32,
}

impl ProgramRule {
    /// Evaluates this rule with the supplied macros. Empty values count as one, numeric
    /// literals keep their value, and other values or missing names count as zero.
    /// Evaluation errors leave the program enabled.
    pub fn is_enabled(&self, defines: &BTreeMap<String, String>) -> bool {
        let value = self
            .expr
            .eval(&|name| defines.get(name).map(|value| macro_value(value)));
        !matches!(value, Ok(value) if value == 0.0)
    }
}

/// `size.buffer.<buffer>=<width> <height>`: a fixed or screen-relative render target size.
#[derive(Debug, Clone, PartialEq)]
pub struct BufferSize {
    /// Buffer name, e.g. "colortex4".
    pub buffer: String,
    /// Width in pixels or as a fraction of the screen width.
    pub width: SizeValue,
    /// Height in pixels or as a fraction of the screen height.
    pub height: SizeValue,
}

/// Iris rule: a value containing '.' is relative to the screen size, else absolute pixels.
/// Both kinds are always positive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SizeValue {
    /// Size in pixels.
    Absolute(u32),
    /// Multiple of the screen dimension.
    Relative(f32),
}

/// `blend.<program>=...` (buffer None) or `blend.<program>.<buffer>=...`.
#[derive(Debug, Clone, PartialEq)]
pub struct BlendRule {
    /// Program the override applies to.
    pub program: String,
    /// Draw buffer (e.g. "colortex0") for a per-buffer override, else all buffers.
    pub buffer: Option<String>,
    /// The blend state to use.
    pub mode: BlendMode,
}

/// Blend state from `off` or `<srcColor> <dstColor> <srcAlpha> <dstAlpha>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendMode {
    /// Blending disabled.
    Off,
    /// Separate color and alpha factors, in the order written (additive equation).
    Factors {
        src_color: BlendFactor,
        dst_color: BlendFactor,
        src_alpha: BlendFactor,
        dst_alpha: BlendFactor,
    },
}

/// A GL blend factor, written in packs as its upper-case GL name without the `GL_`
/// prefix (`ZERO`, `ONE`, `SRC_COLOR`, ..., `SRC_ALPHA_SATURATE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendFactor {
    Zero,
    One,
    SrcColor,
    OneMinusSrcColor,
    DstColor,
    OneMinusDstColor,
    SrcAlpha,
    OneMinusSrcAlpha,
    DstAlpha,
    OneMinusDstAlpha,
    SrcAlphaSaturate,
}

/// `alphaTest.<program>=off` / `false` or `<FUNC> <ref>` (FUNC: NEVER LESS EQUAL LEQUAL GREATER NOTEQUAL GEQUAL ALWAYS).
#[derive(Debug, Clone, PartialEq)]
pub struct AlphaTestRule {
    /// Program the override applies to.
    pub program: String,
    /// The alpha test, or `None` when the value is `off` or `false`.
    pub test: Option<AlphaTest>,
}

/// Fragments pass when `alpha <function> reference` holds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AlphaTest {
    /// Comparison applied to the fragment alpha.
    pub function: CompareFunction,
    /// Reference value the alpha is compared against.
    pub reference: f32,
}

/// Comparison of an alpha test, written in packs as its GL name: NEVER, LESS, EQUAL,
/// LEQUAL, GREATER, NOTEQUAL, GEQUAL or ALWAYS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareFunction {
    Never,
    Less,
    Equal,
    LessEqual,
    Greater,
    NotEqual,
    GreaterEqual,
    Always,
}

/// `flip.<program>.<buffer>=true|false`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlipRule {
    /// Program after which the flip applies.
    pub program: String,
    /// Ping-pong buffer to flip, e.g. "colortex6".
    pub buffer: String,
    /// Whether to flip the buffer.
    pub flip: bool,
}

/// `scale.<program>=<scale> [<offsetX> <offsetY>]`.
#[derive(Debug, Clone, PartialEq)]
pub struct ScaleRule {
    /// Program whose viewport is scaled.
    pub program: String,
    /// Positive viewport scale relative to the screen.
    pub scale: f32,
    /// Viewport offset as a fraction of the screen; `[0.0, 0.0]` when omitted.
    pub offset: [f32; 2],
}

/// `texture.<stage>.<sampler>=<value>`; stage e.g. "gbuffers", "deferred", "composite",
/// "shadowcomp", "prepare". `texture.noise` yields stage "noise" with sampler "noisetex",
/// the uniform it binds.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomTexture {
    /// Program stage group the binding applies to.
    pub stage: String,
    /// Sampler name, possibly dotted (e.g. "colortex6.1").
    pub sampler: String,
    /// Where the texture comes from.
    pub source: TextureSource,
}

impl CustomTexture {
    /// The uniform it binds: [`Self::sampler`] without a texture-unit suffix
    /// (`colortex6.1` → `colortex6`).
    pub fn uniform(&self) -> &str {
        match self.sampler.rsplit_once('.') {
            Some((name, unit)) if unit.bytes().all(|b| b.is_ascii_digit()) => name,
            _ => &self.sampler,
        }
    }

    /// Whether it binds in programs of `group` (`texture.noise` binds everywhere).
    pub fn applies_to(&self, group: ProgramGroup) -> bool {
        self.stage == group.texture_stage() || self.stage == "noise"
    }

    /// Its filtering, as Iris reads it: `blur` and `clamp` from `<path>.mcmeta`, defaulting
    /// to nearest/repeat for images and linear/clamp for raw textures.
    pub fn sampler_class(&self, source: &PackSource) -> SamplerClass {
        let (path, default) = match &self.source {
            TextureSource::File(path) => (Some(path), false),
            TextureSource::Raw { path, .. } => (Some(path), true),
            TextureSource::Resource(_) => (None, false),
        };
        let meta = path
            .and_then(|path| source.text(&format!("{path}.mcmeta")))
            .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok());
        let flag = |key: &str| {
            meta.as_ref()
                .and_then(|meta| meta.get("texture")?.get(key)?.as_bool())
                .unwrap_or(default)
        };
        SamplerClass::from_filtering(flag("blur"), flag("clamp"))
    }
}

/// Source of a custom texture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextureSource {
    /// Pack-relative path (leading '/' stripped), e.g. "tex/noise.png".
    File(String),
    /// `namespace:path` resource location.
    Resource(String),
    /// Raw texture: `<path> <TYPE> <internalFormat> <dims...> <pixelFormat> <pixelType>`;
    /// `path` has its leading '/' stripped and `spec` keeps the remaining tokens unvalidated.
    Raw { path: String, spec: Vec<String> },
}

impl TextureSource {
    /// The texture's shape: raw textures name it (`TEXTURE_1D`, `TEXTURE_3D` …), images are 2D.
    /// OptiFine binds each shape on its own target of the sampler's unit, so a custom texture
    /// only replaces a sampler declared with the same shape.
    pub fn dimension(&self) -> TextureDimension {
        match self {
            TextureSource::Raw { spec, .. } => match spec.first().map(String::as_str) {
                Some("TEXTURE_1D") => TextureDimension::D1,
                Some("TEXTURE_3D") => TextureDimension::D3,
                _ => TextureDimension::D2,
            },
            TextureSource::File(_) | TextureSource::Resource(_) => TextureDimension::D2,
        }
    }
}

/// `image.<name> = <sampler> <format> <internalFormat> <pixelType> <clear> <relative> <w> <h> [<d>]`.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomImage {
    /// Image uniform name.
    pub name: String,
    /// Sampler uniform name for reading the image.
    pub sampler: String,
    /// Pixel format token, e.g. "rgba" or "red_integer".
    pub format: String,
    /// Internal format token, e.g. "rgba16f" or "r8ui".
    pub internal_format: String,
    /// Pixel type token, e.g. "half_float" or "unsigned_byte".
    pub pixel_type: String,
    /// Whether the image is cleared every frame.
    pub clear: bool,
    /// Image dimensions.
    pub size: ImageSize,
}

/// Size of a custom image; relative images are 2D, absolute images 1D, 2D or 3D.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ImageSize {
    /// Positive fractions of the screen width and height.
    Relative { width: f32, height: f32 },
    /// Positive pixel sizes; `height` is set for 2D and 3D images, `depth` for 3D.
    Absolute {
        width: u32,
        height: Option<u32>,
        depth: Option<u32>,
    },
}

/// A `uniform.<type>.<name>=<expr>` or `variable.<type>.<name>=<expr>` definition.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomUniform {
    /// Whether the value is exposed to shaders or only usable by later expressions.
    pub kind: CustomUniformKind,
    /// GLSL type token, e.g. "float" or "vec3".
    pub ty: String,
    /// Uniform or variable name.
    pub name: String,
    /// Expression computing the value each frame.
    pub expr: Expr,
    /// 1-based line of the definition.
    pub line: u32,
}

/// Kind of a [`CustomUniform`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomUniformKind {
    /// `uniform.*`: uploaded to shaders.
    Uniform,
    /// `variable.*`: an intermediate value visible only to custom uniform expressions.
    Variable,
}
