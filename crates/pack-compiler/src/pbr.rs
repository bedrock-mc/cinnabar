//! Authored material decoding and mip filtering for the optional Enhanced texture arrays.

use std::path::{Path, PathBuf};

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
}

impl PbrPack {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>, override_format: Option<PbrFormat>) -> Self {
        let root = root.into();
        let format = override_format.unwrap_or_else(|| sources::declared_format(&root));
        Self {
            root,
            format,
            group: None,
            normal_format: PbrNormalFormat::DirectX,
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
}

impl PbrTexture {
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
