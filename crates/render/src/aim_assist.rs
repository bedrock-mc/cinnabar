//! Retained target highlights for camera aim assistance.

use std::sync::Arc;

use bevy::prelude::*;

mod gpu;
#[cfg(test)]
mod tests;

pub use gpu::AimAssistHighlightPlugin;

pub const AIM_ASSIST_TEXTURES: [&str; 2] = [
    "textures/ui/aimassist_block_highlight",
    "textures/ui/aimassist_entity_highlight",
];

/// Pack-owned RGBA pixels are shared between worlds without copying on extraction.
#[derive(Debug)]
pub struct AimAssistTexture {
    pub size: [u32; 2],
    pub rgba: Arc<[u8]>,
}

impl AimAssistTexture {
    /// Rejects unusable dimensions before a texture reaches GPU preparation.
    pub fn new(size: [u32; 2], rgba: Arc<[u8]>) -> Option<Self> {
        let bytes = u64::from(size[0]) * u64::from(size[1]) * 4;
        (size.iter().all(|side| (1..=4096).contains(side)) && bytes == rgba.len() as u64)
            .then_some(Self { size, rgba })
    }
}

/// One world-space unit quad; the texture selects the block or actor highlight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AimAssistHighlight {
    pub center: Vec3,
    pub right: Vec3,
    pub up: Vec3,
    pub texture: usize,
}

impl AimAssistHighlight {
    /// Block highlights follow the selected face, with horizontal faces oriented to player look.
    pub fn block(center: Vec3, face: u8, player_forward: Vec3) -> Option<Self> {
        let face_rotation = face_rotation(face)?;
        let rotation = if face < 2 {
            face_rotation_for_look(player_forward) * face_rotation
        } else {
            face_rotation
        };
        Some(Self {
            center,
            right: rotation.x_axis,
            up: rotation.y_axis,
            texture: 0,
        })
    }

    /// Actor highlights billboard against the displayed camera, including camera roll.
    pub fn actor(center: Vec3, camera_forward: Vec3, camera_up: Vec3) -> Option<Self> {
        let forward = camera_forward.try_normalize()?;
        let right = camera_up.cross(forward).try_normalize()?;
        Some(Self {
            center,
            right,
            up: forward.cross(right),
            texture: 1,
        })
    }

    /// The fixed uniform contains no geometry buffers and only changes with the target pose.
    fn record(self) -> [f32; 12] {
        [
            self.center.x,
            self.center.y,
            self.center.z,
            0.0,
            self.right.x,
            self.right.y,
            self.right.z,
            0.0,
            self.up.x,
            self.up.y,
            self.up.z,
            0.0,
        ]
    }
}

/// Shared textures stay resident when targeting stops; a missing texture draws nothing.
#[derive(Resource, bevy::render::extract_resource::ExtractResource, Clone, Default)]
pub struct AimAssistHighlightScene {
    pub target: Option<AimAssistHighlight>,
    pub textures: [Option<Arc<AimAssistTexture>>; 2],
}

/// Exact cardinal rotations keep face seams and ties independent of trigonometric rounding.
fn face_rotation(face: u8) -> Option<Mat3> {
    Some(match face {
        0 => Mat3::from_cols(Vec3::X, Vec3::NEG_Z, Vec3::Y),
        1 => Mat3::from_cols(Vec3::X, Vec3::Z, Vec3::NEG_Y),
        2 => Mat3::IDENTITY,
        3 => Mat3::from_cols(Vec3::NEG_X, Vec3::Y, Vec3::NEG_Z),
        4 => Mat3::from_cols(Vec3::NEG_Z, Vec3::Y, Vec3::X),
        5 => Mat3::from_cols(Vec3::Z, Vec3::Y, Vec3::NEG_X),
        _ => return None,
    })
}

/// Strict comparisons retain the earlier cardinal direction at equal projected magnitudes.
fn face_rotation_for_look(forward: Vec3) -> Mat3 {
    let z = forward.z.max(0.0);
    let mut face = if -forward.z > z { 3 } else { 2 };
    let best_z = (-forward.z).max(z);
    if forward.x > best_z {
        face = 4;
    }
    if -forward.x > forward.x.max(best_z) {
        face = 5;
    }
    face_rotation(face).expect("the selected cardinal face is valid")
}
