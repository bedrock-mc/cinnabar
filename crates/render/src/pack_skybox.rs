//! Pack skybox presentation, separate from the required atmosphere carrier.

use render_scene::SkyboxSettings;

use assets::SkyboxDimension;
use bevy::math::Vec4;
use bevy::render::render_resource::ShaderType;
use bytemuck::{Pod, Zeroable};

use render_scene::{AtmosphereFrame, SkyKind};

/// Bedrock translates the cube before scaling its authored half-unit extent.
const NATIVE_SKYBOX_TRANSLATION: f32 = 256.0;
const NATIVE_SKYBOX_SCALE: f32 = 2000.0;
const SKYBOX_HALF_EXTENT: f32 = 0.5;

/// Translation in cube half-extents, face mask, visibility, and radius.
#[repr(C, align(16))]
#[derive(Clone, Copy, Pod, Zeroable, ShaderType)]
pub(crate) struct SkyboxUniform {
    pub(crate) parameters: Vec4,
}

impl SkyboxUniform {
    /// Builds the active dimension's transform without using the camera's position.
    pub(crate) fn new(frame: AtmosphereFrame, settings: SkyboxSettings, mask: u32) -> Self {
        let overworld = frame.sky_kind() == SkyKind::Overworld;
        let offset = if overworld && settings.native_vertical_offset {
            NATIVE_SKYBOX_TRANSLATION / (NATIVE_SKYBOX_SCALE * SKYBOX_HALF_EXTENT)
        } else {
            0.0
        };
        Self {
            parameters: Vec4::new(
                offset,
                mask as f32,
                u8::from(settings.visible) as f32,
                if overworld {
                    NATIVE_SKYBOX_SCALE * SKYBOX_HALF_EXTENT
                } else {
                    8.0
                },
            ),
        }
    }
}

/// Selects the numbered texture namespace independently of pass admission.
pub(crate) fn dimension(frame: AtmosphereFrame) -> SkyboxDimension {
    match frame.sky_kind() {
        SkyKind::Overworld => SkyboxDimension::Overworld,
        SkyKind::Nether => SkyboxDimension::Nether,
        SkyKind::End => SkyboxDimension::End,
    }
}

#[cfg(test)]
#[path = "pack_skybox/tests.rs"]
mod tests;
