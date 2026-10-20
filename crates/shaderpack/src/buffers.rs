//! Colour buffers: names, formats and the `const` directives that configure them.

use crate::preprocess::ConstDirective;
use std::collections::BTreeMap;

/// A pack colour buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BufferId {
    /// `colortexN` (also the legacy `gcolor`, `gdepth`, `gnormal`, `composite`, `gaux1`–`gaux4`).
    Color(u32),
    /// `shadowcolorN` (`shadowcolor` is `shadowcolor0`).
    ShadowColor(u32),
}

/// Legacy OptiFine buffer names and their colortex index.
const LEGACY_COLOR_NAMES: &[(&str, u32)] = &[
    ("gcolor", 0),
    ("gdepth", 1),
    ("gnormal", 2),
    ("composite", 3),
    ("gaux1", 4),
    ("gaux2", 5),
    ("gaux3", 6),
    ("gaux4", 7),
];

impl BufferId {
    /// Parses `colortex7`, `gaux2`, `shadowcolor1`, `shadowcolor`.
    pub fn parse(name: &str) -> Option<BufferId> {
        if let Some((_, i)) = LEGACY_COLOR_NAMES.iter().find(|(n, _)| *n == name) {
            return Some(BufferId::Color(*i));
        }
        if name == "shadowcolor" {
            return Some(BufferId::ShadowColor(0));
        }
        let index = |rest: &str| {
            (!rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
                .then(|| rest.parse().ok())
                .flatten()
        };
        if let Some(rest) = name.strip_prefix("colortex") {
            return index(rest).map(BufferId::Color);
        }
        name.strip_prefix("shadowcolor")
            .and_then(index)
            .map(BufferId::ShadowColor)
    }
}

/// Uses one key for buffer aliases and for the compatibility texture uniform.
pub(crate) fn canonical_sampler_name(name: &str) -> String {
    match BufferId::parse(name) {
        Some(BufferId::Color(index)) => format!("colortex{index}"),
        Some(BufferId::ShadowColor(index)) => format!("shadowcolor{index}"),
        None if name == "texture" => "gtexture".to_string(),
        None => name.to_string(),
    }
}

macro_rules! buffer_formats {
    ($($variant:ident => $name:literal),* $(,)?) => {
        /// A colour buffer internal format, named as in OptiFine/Iris directives.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum BufferFormat {
            $($variant),*
        }

        impl BufferFormat {
            /// Every format.
            pub const ALL: &[BufferFormat] = &[$(BufferFormat::$variant),*];

            /// The directive spelling, e.g. `RGBA16F`.
            pub fn name(self) -> &'static str {
                match self {
                    $(BufferFormat::$variant => $name),*
                }
            }
        }
    };
}

buffer_formats! {
    Rgba => "RGBA", R8 => "R8", Rg8 => "RG8", Rgb8 => "RGB8", Rgba8 => "RGBA8",
    R8Snorm => "R8_SNORM", Rg8Snorm => "RG8_SNORM", Rgb8Snorm => "RGB8_SNORM", Rgba8Snorm => "RGBA8_SNORM",
    R16 => "R16", Rg16 => "RG16", Rgb16 => "RGB16", Rgba16 => "RGBA16",
    R16Snorm => "R16_SNORM", Rg16Snorm => "RG16_SNORM", Rgb16Snorm => "RGB16_SNORM", Rgba16Snorm => "RGBA16_SNORM",
    R16f => "R16F", Rg16f => "RG16F", Rgb16f => "RGB16F", Rgba16f => "RGBA16F",
    R32f => "R32F", Rg32f => "RG32F", Rgb32f => "RGB32F", Rgba32f => "RGBA32F",
    R8i => "R8I", Rg8i => "RG8I", Rgb8i => "RGB8I", Rgba8i => "RGBA8I",
    R8ui => "R8UI", Rg8ui => "RG8UI", Rgb8ui => "RGB8UI", Rgba8ui => "RGBA8UI",
    R16i => "R16I", Rg16i => "RG16I", Rgb16i => "RGB16I", Rgba16i => "RGBA16I",
    R16ui => "R16UI", Rg16ui => "RG16UI", Rgb16ui => "RGB16UI", Rgba16ui => "RGBA16UI",
    R32i => "R32I", Rg32i => "RG32I", Rgb32i => "RGB32I", Rgba32i => "RGBA32I",
    R32ui => "R32UI", Rg32ui => "RG32UI", Rgb32ui => "RGB32UI", Rgba32ui => "RGBA32UI",
    Rgba2 => "RGBA2", Rgba4 => "RGBA4", R3G3B2 => "R3_G3_B2", Rgb5A1 => "RGB5_A1", Rgb565 => "RGB565",
    Rgb10A2 => "RGB10_A2", Rgb10A2ui => "RGB10_A2UI", R11fG11fB10f => "R11F_G11F_B10F", Rgb9E5 => "RGB9_E5",
}

impl BufferFormat {
    /// Iris's format for buffers without a format directive.
    pub const DEFAULT: BufferFormat = BufferFormat::Rgba;

    /// Parses a directive spelling, case-insensitively.
    pub fn parse(name: &str) -> Option<BufferFormat> {
        BufferFormat::ALL
            .iter()
            .copied()
            .find(|f| f.name().eq_ignore_ascii_case(name.trim()))
    }

    /// The GLSL storage-image qualifier, or `None` for render-only formats.
    pub fn glsl_qualifier(self) -> Option<String> {
        use BufferFormat::*;
        match self {
            Rgba => Some("rgba8".to_string()),
            R8 | Rg8 | Rgba8 | R8Snorm | Rg8Snorm | Rgba8Snorm | R16 | Rg16 | Rgba16 | R16Snorm
            | Rg16Snorm | Rgba16Snorm | R16f | Rg16f | Rgba16f | R32f | Rg32f | Rgba32f | R8i
            | Rg8i | Rgba8i | R8ui | Rg8ui | Rgba8ui | R16i | Rg16i | Rgba16i | R16ui | Rg16ui
            | Rgba16ui | R32i | Rg32i | Rgba32i | R32ui | Rg32ui | Rgba32ui | Rgb10A2
            | Rgb10A2ui | R11fG11fB10f => Some(self.name().to_ascii_lowercase()),
            Rgb8 | Rgb8Snorm | Rgb16 | Rgb16Snorm | Rgb16f | Rgb32f | Rgb8i | Rgb8ui | Rgb16i
            | Rgb16ui | Rgb32i | Rgb32ui | Rgba2 | Rgba4 | R3G3B2 | Rgb5A1 | Rgb565 | Rgb9E5 => {
                None
            }
        }
    }
}

/// Settings of one buffer from `const` directives such as
/// `const int colortex3Format = RGBA16F;` or `const bool colortex5Clear = false;`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BufferSettings {
    pub format: Option<BufferFormat>,
    pub clear: Option<bool>,
    /// The `<buffer>ClearColor` expression as written, e.g. `vec4(0.0, 0.0, 0.0, 1.0)`.
    pub clear_color: Option<String>,
    pub mipmap: Option<bool>,
}

/// Buffer settings collected across a pack's programs; later directives win.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BufferDirectives {
    pub buffers: BTreeMap<BufferId, BufferSettings>,
    /// Directive values that did not parse, as (name, value).
    pub rejected: Vec<(String, String)>,
}

impl BufferDirectives {
    /// Applies one `const` directive if it configures a buffer.
    pub fn accept(&mut self, c: &ConstDirective) {
        const SUFFIXES: &[&str] = &["ClearColor", "MipmapEnabled", "Format", "Clear"];
        let Some((buffer, suffix)) = SUFFIXES
            .iter()
            .find_map(|s| c.name.strip_suffix(s).map(|b| (b, *s)))
        else {
            return;
        };
        let Some(id) = BufferId::parse(buffer) else {
            return;
        };
        let settings = self.buffers.entry(id).or_default();
        let ok = match suffix {
            "Format" => BufferFormat::parse(&c.value).map(|f| settings.format = Some(f)),
            "Clear" => parse_bool(&c.value).map(|b| settings.clear = Some(b)),
            "MipmapEnabled" => parse_bool(&c.value).map(|b| settings.mipmap = Some(b)),
            _ => {
                settings.clear_color = Some(c.value.clone());
                Some(())
            }
        };
        if ok.is_none() {
            self.rejected.push((c.name.clone(), c.value.clone()));
        }
    }

    /// The buffer's format, or [`BufferFormat::DEFAULT`].
    pub fn format(&self, id: BufferId) -> BufferFormat {
        self.buffers
            .get(&id)
            .and_then(|s| s.format)
            .unwrap_or(BufferFormat::DEFAULT)
    }
}

/// Reads a trimmed GLSL boolean constant.
fn parse_bool(v: &str) -> Option<bool> {
    match v.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates an integer directive fixture with the supplied name and value.
    fn c(name: &str, value: &str) -> ConstDirective {
        ConstDirective {
            ty: "int".into(),
            name: name.into(),
            value: value.into(),
        }
    }

    #[test]
    fn sampler_compatibility_names_keep_buffer_identity() {
        for (name, index) in LEGACY_COLOR_NAMES {
            assert_eq!(
                BufferId::parse(&canonical_sampler_name(name)),
                Some(BufferId::Color(*index))
            );
        }
        assert_eq!(
            BufferId::parse(&canonical_sampler_name("shadowcolor")),
            Some(BufferId::ShadowColor(0))
        );
        assert_eq!(canonical_sampler_name("texture"), "gtexture");
    }

    #[test]
    fn reflection_image_qualifiers_parse() {
        for format in BufferFormat::ALL {
            let qualifier = format.glsl_qualifier();
            let Some(qualifier) = qualifier else { continue };
            let source = format!(
                "#version 450\nlayout(set=0,binding=0,{qualifier}) uniform image2D image;\nvoid main() {{}}"
            );
            let result = naga::front::glsl::Frontend::default().parse(
                &naga::front::glsl::Options::from(naga::ShaderStage::Compute),
                &source,
            );
            assert!(result.is_ok(), "{format:?}: {result:?}");
        }
    }

    #[test]
    fn buffer_names_include_legacy_aliases() {
        assert_eq!(BufferId::parse("colortex12"), Some(BufferId::Color(12)));
        assert_eq!(BufferId::parse("gaux2"), Some(BufferId::Color(5)));
        assert_eq!(
            BufferId::parse("shadowcolor"),
            Some(BufferId::ShadowColor(0))
        );
        assert_eq!(
            BufferId::parse("shadowcolor1"),
            Some(BufferId::ShadowColor(1))
        );
        assert_eq!(BufferId::parse("colortex"), None);
        assert_eq!(BufferId::parse("colortexA"), None);
    }

    #[test]
    fn format_and_clear_directives_configure_buffers() {
        let mut d = BufferDirectives::default();
        d.accept(&c("colortex3Format", "RGBA16F"));
        d.accept(&c("gaux1Format", "r11f_g11f_b10f"));
        d.accept(&c("colortex3Clear", "false"));
        d.accept(&c("colortex9Format", "NOT_A_FORMAT"));
        d.accept(&c("shadowMapResolution", "2048"));
        assert_eq!(d.format(BufferId::Color(3)), BufferFormat::Rgba16f);
        assert_eq!(d.format(BufferId::Color(4)), BufferFormat::R11fG11fB10f);
        assert_eq!(d.format(BufferId::Color(0)), BufferFormat::DEFAULT);
        assert_eq!(d.buffers[&BufferId::Color(3)].clear, Some(false));
        assert_eq!(
            d.rejected,
            [("colortex9Format".to_string(), "NOT_A_FORMAT".to_string())]
        );
        assert_eq!(
            BufferFormat::R11fG11fB10f.glsl_qualifier(),
            Some("r11f_g11f_b10f".to_string())
        );
    }
}
