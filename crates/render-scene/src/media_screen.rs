use bevy::prelude::{Quat, Resource, Vec3};
use std::sync::Arc;
pub const MAX_MEDIA_SCREENS: usize = 4;

/// Screens to draw this frame; at most `MAX_MEDIA_SCREENS` are used, in order.
#[derive(Resource, bevy::render::extract_resource::ExtractResource, Clone, Default)]
#[extract_app(bevy::render::RenderApp)]
pub struct MediaScreenScene {
    pub screens: Vec<MediaScreen>,
    /// Texture bytes all screens together may allocate.
    pub gpu_budget_bytes: u64,
}

#[derive(Clone)]
pub struct MediaScreen {
    /// Keys the retained texture across frames.
    pub id: u64,
    pub center: [f32; 3],
    pub half_right: [f32; 3],
    pub half_up: [f32; 3],
    pub frame: Option<MediaFrame>,
}

/// sRGB RGBA8, row-major, top row first; uploaded once per `serial`.
#[derive(Clone)]
pub struct MediaFrame {
    pub serial: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

/// Centre and half-extent axes of a quad of `size` in its local XY plane facing local +Z, after
/// scale, xyzw rotation and translation.
pub fn media_screen_axes(
    translation: [f32; 3],
    rotation: [f32; 4],
    scale: [f32; 3],
    size: [f32; 2],
) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let rotation = Quat::from_array(rotation);
    let right = rotation * Vec3::new(scale[0] * size[0] * 0.5, 0.0, 0.0);
    let up = rotation * Vec3::new(0.0, scale[1] * size[1] * 0.5, 0.0);
    (translation, right.to_array(), up.to_array())
}
