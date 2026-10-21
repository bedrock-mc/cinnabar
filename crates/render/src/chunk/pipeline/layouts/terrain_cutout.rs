use crate::chunk::*;
use bevy::render::render_resource::DepthBiasState;

/// Keeps two-sided cutout cubes behind coplanar opaque model faces. Reverse-Z
/// negates the conventional-depth slope offset; a constant offset would move
/// distant foliage even when the surfaces do not share a plane.
pub(super) fn apply(descriptor: &mut RenderPipelineDescriptor) {
    descriptor
        .depth_stencil
        .as_mut()
        .expect("cutout terrain depth state")
        .bias = DepthBiasState {
        slope_scale: -0.1,
        ..default()
    };
}
