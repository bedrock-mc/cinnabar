//! Conduits: a shell cube, plus a spinning cage and a viewer-facing eye while active.
//!
//! Box sizes follow the conduit textures (6x6x6 shell, 8x8x8 cage, 16x16x16 wind cube); wind
//! frame timing, wind spin, bob and eye size need native measurement.

use assets::block_entity_geometry as geometry;
use bevy::math::{Mat4, Vec3};

use super::{
    atlas::{AtlasRect, BlockEntityAtlas, TextureRef},
    mesh::{BoxSpec, Layer, MeshBuilder, WHITE, model_matrix},
    scene::SceneClock,
};

const SPIN_DEGREES_PER_TICK: f64 = 2.0;
const BOB_PIXELS: f32 = 1.6;
const BOB_PERIOD_TICKS: f64 = 40.0;
const EYE_HALF_PIXELS: f32 = 2.0;
const ACTIVE_HEIGHT_PIXELS: f32 = 8.0;
/// The wind strips stack 22 frames of a 64x32 cube unwrap.
const WIND_FRAMES: u32 = 22;
const WIND_FRAME_ROWS: f32 = 32.0;
const WIND_TICKS_PER_FRAME: f64 = 2.0;

/// The rect of wind frame `frame` in a strip placed at `strip`; `frame` wraps.
#[must_use]
pub fn wind_frame_rect(strip: AtlasRect, frame: u32) -> AtlasRect {
    let height = strip.height / WIND_FRAMES as f32;
    AtlasRect {
        x: strip.x,
        y: strip.y + height * (frame % WIND_FRAMES) as f32,
        width: strip.width,
        height,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConduitModel {
    pub active: bool,
    /// Whether a hostile target is being attacked; opens the eye.
    pub hunting: bool,
    /// Yaw that turns the eye quad toward the viewer.
    pub viewer_yaw_degrees: f32,
}

pub(super) fn emit(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    block: [i32; 3],
    model: &ConduitModel,
    clock: SceneClock,
) {
    let phase = (clock.ticks / BOB_PERIOD_TICKS).fract() as f32;
    let bob = if model.active {
        BOB_PIXELS * (std::f32::consts::TAU * phase).sin()
    } else {
        0.0
    };
    let center = model_matrix(block, [0.5, 0.0, 0.5], 0.0)
        * Mat4::from_translation(Vec3::new(0.0, ACTIVE_HEIGHT_PIXELS + bob, 0.0));
    if let Some(shell) = atlas.texture(geometry::CONDUIT_TEXTURE.0, geometry::CONDUIT_TEXTURE.1) {
        builder.cuboid(
            Layer::Solid,
            &shell,
            center,
            BoxSpec::from(geometry::CONDUIT_SHELL),
            WHITE,
        );
    }
    if !model.active {
        return;
    }
    if let Some(cage) = atlas.texture("textures/blocks/conduit_cage", [32.0, 16.0]) {
        let spin = Mat4::from_rotation_y(
            ((clock.ticks * SPIN_DEGREES_PER_TICK) % 360.0).to_radians() as f32,
        );
        builder.cuboid(
            Layer::Solid,
            &cage,
            center * spin,
            BoxSpec::new([-4.0, -4.0, -4.0], [8.0, 8.0, 8.0], [0.0, 0.0]),
            WHITE,
        );
    }
    let frame = (clock.ticks / WIND_TICKS_PER_FRAME) as u32;
    for (strip, spin_scale) in [
        ("textures/blocks/conduit_wind_horizontal", 1.0),
        ("textures/blocks/conduit_wind_vertical", -1.3),
    ] {
        if let Some(strip) = atlas.texture(strip, [64.0, 704.0]) {
            let texture = TextureRef {
                rect: wind_frame_rect(strip.rect, frame),
                logical: [64.0, WIND_FRAME_ROWS],
            };
            let spin = Mat4::from_rotation_y(
                ((clock.ticks * SPIN_DEGREES_PER_TICK * spin_scale) % 360.0).to_radians() as f32,
            );
            builder.cuboid(
                Layer::Overlay,
                &texture,
                center * spin,
                BoxSpec::new([-8.0, -8.0, -8.0], [16.0, 16.0, 16.0], [0.0, 0.0]),
                WHITE,
            );
        }
    }
    let eye = if model.hunting {
        "textures/blocks/conduit_open"
    } else {
        "textures/blocks/conduit_closed"
    };
    if let Some(eye) = atlas.texture(eye, [8.0, 8.0]) {
        let facing = center * Mat4::from_rotation_y(model.viewer_yaw_degrees.to_radians());
        let corners = [
            [EYE_HALF_PIXELS, EYE_HALF_PIXELS, -3.5],
            [-EYE_HALF_PIXELS, EYE_HALF_PIXELS, -3.5],
            [-EYE_HALF_PIXELS, -EYE_HALF_PIXELS, -3.5],
            [EYE_HALF_PIXELS, -EYE_HALF_PIXELS, -3.5],
        ]
        .map(|corner| facing.transform_point3(Vec3::from_array(corner)).to_array());
        builder.textured_quad(Layer::Solid, corners, eye.rect, WHITE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wind_frames_stack_down_the_strip_and_wrap() {
        let strip = AtlasRect {
            x: 10.0,
            y: 100.0,
            width: 64.0,
            height: 704.0,
        };
        assert_eq!(wind_frame_rect(strip, 0).y, 100.0);
        assert_eq!(wind_frame_rect(strip, 3).y, 100.0 + 3.0 * 32.0);
        assert_eq!(
            wind_frame_rect(strip, WIND_FRAMES),
            wind_frame_rect(strip, 0)
        );
        assert_eq!(wind_frame_rect(strip, 1).height, 32.0);
    }
    #[test]
    fn review_render_wind_uses_replacement_frame_height() {
        let strip = AtlasRect {
            x: 5.0,
            y: 7.0,
            width: 128.0,
            height: WIND_FRAMES as f32 * 64.0,
        };
        let frame = wind_frame_rect(strip, 3);
        assert_eq!(frame.y, 7.0 + 3.0 * 64.0);
        assert_eq!(frame.height, 64.0);
        assert_eq!(wind_frame_rect(strip, WIND_FRAMES + 3), frame);
    }
}
