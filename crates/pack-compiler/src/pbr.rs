//! Authored material decoding and mip filtering for the optional Enhanced texture arrays.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use image::RgbaImage;

mod decode;
mod frames;
mod mips;
mod sources;

pub use mips::{PbrMipLayer, build_pbr_mips};

pub use assets::{
    PBR_REF_COLOR, PBR_REF_HEIGHT, PBR_REF_LABPBR, PBR_REF_MATERIAL, PBR_REF_NORMAL,
    PBR_REF_OCCLUSION, PBR_REF_SUBSURFACE,
};

/// Suffixes alone never identify the meaning of Java specular channels.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PbrFormat {
    #[default]
    Unspecified,
    LabPbr13,
    Legacy,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PbrNormalFormat {
    #[default]
    DirectX,
    OpenGl,
}

/// Describes the supplied color artwork; resolution alone does not establish its style.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PbrColorStyle {
    #[default]
    Unspecified,
    Photorealistic,
    PixelArt,
    Vanilla,
}

impl PbrColorStyle {
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "unspecified" => Some(Self::Unspecified),
            "photorealistic" => Some(Self::Photorealistic),
            "pixel_art" => Some(Self::PixelArt),
            "vanilla" => Some(Self::Vanilla),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unspecified => "unspecified",
            Self::Photorealistic => "photorealistic",
            Self::PixelArt => "pixel_art",
            Self::Vanilla => "vanilla",
        }
    }
}

impl PbrNormalFormat {
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "directx" | "direct-x" => Some(Self::DirectX),
            "opengl" | "open-gl" => Some(Self::OpenGl),
            _ => None,
        }
    }
}

impl PbrFormat {
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" | "unspecified" => Some(Self::Unspecified),
            "lab-pbr/1.3" | "labpbr/1.3" => Some(Self::LabPbr13),
            "old-pbr" | "oldpbr" | "legacy" | "seus-pbr" => Some(Self::Legacy),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PbrPack {
    root: PathBuf,
    format: PbrFormat,
    group: Option<String>,
    normal_format: PbrNormalFormat,
    color_style: PbrColorStyle,
    terrain_index: Result<BTreeMap<String, String>, String>,
    terrain_aliases: BTreeMap<String, Vec<String>>,
    prefer_complete_material: bool,
}

impl PbrPack {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>, override_format: Option<PbrFormat>) -> Self {
        let root = root.into();
        let format = override_format.unwrap_or_else(|| sources::declared_format(&root));
        let terrain_index = sources::terrain::read(&root);
        Self {
            root,
            format,
            group: None,
            normal_format: PbrNormalFormat::DirectX,
            color_style: PbrColorStyle::Unspecified,
            terrain_index,
            terrain_aliases: BTreeMap::new(),
            prefer_complete_material: false,
        }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub const fn format(&self) -> PbrFormat {
        self.format
    }

    /// Groups a Java albedo pack with its separately distributed material companion.
    #[must_use]
    pub fn with_group(mut self, group: impl Into<String>) -> Self {
        self.group = Some(group.into());
        self
    }

    #[must_use]
    pub fn with_normal_format(mut self, format: PbrNormalFormat) -> Self {
        self.normal_format = format;
        self
    }

    #[must_use]
    pub fn with_color_style(mut self, style: PbrColorStyle) -> Self {
        self.color_style = style;
        self
    }

    #[must_use]
    pub const fn color_style(&self) -> PbrColorStyle {
        self.color_style
    }

    /// Gives complete matched sets precedence over color-only candidates.
    #[must_use]
    pub fn with_complete_material_preference(mut self, enabled: bool) -> Self {
        self.prefer_complete_material = enabled;
        self
    }

    #[must_use]
    pub const fn prefers_complete_material(&self) -> bool {
        self.prefer_complete_material
    }

    /// Resolves the carrier's variant-zero aliases through this pack's terrain index.
    #[must_use]
    pub fn with_terrain_aliases<'a>(
        mut self,
        aliases: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Self {
        if let Ok(index) = &self.terrain_index {
            for (key, alias) in aliases {
                if let Some(path) = index.get(key) {
                    let paths = self.terrain_aliases.entry(alias.to_owned()).or_default();
                    if !paths.contains(path) {
                        paths.push(path.clone());
                    }
                }
            }
        }
        self
    }

    /// Conflicting overrides cannot share one compiled texture reference.
    pub fn terrain_path(&self, alias: &str) -> Result<Option<&str>, String> {
        self.terrain_index.as_ref().map_err(Clone::clone)?;
        match self.terrain_aliases.get(alias).map(Vec::as_slice) {
            None | Some([]) => Ok(None),
            Some([path]) => Ok(Some(path)),
            Some(_) => Err(format!(
                "{} assigns conflicting terrain paths to shared alias {alias}",
                self.root.display(),
            )),
        }
    }

    #[must_use]
    pub fn material_group(&self) -> Option<&str> {
        self.group.as_deref()
    }

    #[must_use]
    pub const fn normal_format(&self) -> PbrNormalFormat {
        self.normal_format
    }
}

/// Normal RG is unit tangent XY in increasing-V convention, B is AO, and A is height.
/// Material RGBA is MER plus subsurface, or LabPBR F0/metal, emission, roughness, porosity/SSS.
#[derive(Clone, Debug)]
pub struct PbrSurface {
    pub color: RgbaImage,
    pub normal: RgbaImage,
    pub material: RgbaImage,
    pub flags: u32,
}

#[derive(Clone, Debug)]
pub struct PbrTexture {
    surface: PbrSurface,
    timeline: Vec<u32>,
    height: Option<RgbaImage>,
    source: PbrTextureSource,
}

/// Image paths refer to pack inputs, before frame extraction and upload resizing.
#[derive(Clone, Debug)]
pub struct PbrTextureSource {
    pub color: Option<PathBuf>,
    pub normal: Option<PathBuf>,
    pub material: Option<PathBuf>,
    pub height: Option<PathBuf>,
    pub native_color_size: [u32; 2],
    pub native_normal_size: [u32; 2],
    pub native_material_size: [u32; 2],
    pub color_style: PbrColorStyle,
}

impl PbrTexture {
    #[must_use]
    pub const fn source(&self) -> &PbrTextureSource {
        &self.source
    }

    /// Largest authored frame side, excluding strip length and uniform defaults.
    #[must_use]
    pub fn native_frame_size(&self) -> u32 {
        [
            Some(&self.surface.color),
            Some(&self.surface.normal),
            Some(&self.surface.material),
            self.height.as_ref(),
        ]
        .into_iter()
        .flatten()
        .map(|image| image.width().min(image.height()))
        .max()
        .unwrap_or(1)
    }

    #[must_use]
    pub fn has_complete_material(&self) -> bool {
        self.source.normal.is_some() && self.surface.flags & PBR_REF_MATERIAL != 0
    }
    /// Returns the flags admitted from the authored map set.
    #[must_use]
    pub const fn flags(&self) -> u32 {
        self.surface.flags
    }

    /// Returns whether a standalone height map was supplied by the pack.
    #[must_use]
    pub fn has_height_map(&self) -> bool {
        self.height.is_some()
    }

    /// Maps authored frame order onto the carrier's slots; the carrier retains its clock.
    /// Custom frame counts and durations are sampled into those slots, not scheduled independently.
    pub fn frame(
        &self,
        timeline_index: usize,
        timeline_count: usize,
    ) -> Result<PbrSurface, String> {
        frames::extract(
            &self.surface,
            self.height.as_ref(),
            &self.timeline,
            timeline_index,
            timeline_count,
        )
    }
}

/// Pack priority is first-to-last; a Bedrock texture set never borrows another pack's images.
pub fn load_pbr_texture(packs: &[PbrPack], alias: &str) -> Result<Option<PbrTexture>, String> {
    sources::load(packs, alias)
}

#[cfg(test)]
mod tests;
