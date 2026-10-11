//! Beacon beam: two nested translucent columns rising from the beacon, with the texture
//! scrolling up the beam.
//!
//! Radii, alpha and scroll speed need native measurement.

use glam::Vec3;

use super::{
    atlas::BlockEntityAtlas,
    mesh::{Layer, MeshBuilder},
    scene::SceneClock,
};

const INNER_RADIUS: f32 = 0.2;
const OUTER_RADIUS: f32 = 0.25;
const INNER_ALPHA: f32 = 0.8;
const OUTER_ALPHA: f32 = 0.3;
const SCROLL_TICKS_PER_LOOP: f64 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeaconModel {
    /// Beam height in blocks above the beacon block; zero draws nothing.
    pub height: u32,
    /// Linear RGB tint, blended from the stained glass the beam passes through.
    pub tint: [f32; 3],
}

/// Fraction of one texture tile the beam has scrolled at `ticks`.
#[must_use]
pub fn scroll_fraction(ticks: f64) -> f32 {
    (ticks / SCROLL_TICKS_PER_LOOP).rem_euclid(1.0) as f32
}

pub(super) fn emit(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    block: [i32; 3],
    model: &BeaconModel,
    clock: SceneClock,
) {
    if model.height == 0 {
        return;
    }
    let Some(texture) = atlas.texture("textures/entity/beacon_beam", [16.0, 16.0]) else {
        return;
    };
    let center = Vec3::new(
        block[0] as f32 + 0.5,
        block[1] as f32 + 1.0,
        block[2] as f32 + 0.5,
    );
    let scroll = scroll_fraction(clock.ticks);
    let rect = texture.rect;
    for (radius, alpha) in [(INNER_RADIUS, INNER_ALPHA), (OUTER_RADIUS, OUTER_ALPHA)] {
        let color = [model.tint[0], model.tint[1], model.tint[2], alpha];
        let (x0, x1) = (center.x - radius, center.x + radius);
        let (z0, z1) = (center.z - radius, center.z + radius);
        let sides = |y_low: f32, y_high: f32| {
            [
                [
                    [x1, y_high, z0],
                    [x0, y_high, z0],
                    [x0, y_low, z0],
                    [x1, y_low, z0],
                ],
                [
                    [x0, y_high, z1],
                    [x1, y_high, z1],
                    [x1, y_low, z1],
                    [x0, y_low, z1],
                ],
                [
                    [x1, y_high, z1],
                    [x1, y_high, z0],
                    [x1, y_low, z0],
                    [x1, y_low, z1],
                ],
                [
                    [x0, y_high, z0],
                    [x0, y_high, z1],
                    [x0, y_low, z1],
                    [x0, y_low, z0],
                ],
            ]
        };
        for step in 0..model.height {
            let bottom = center.y + step as f32;
            // A tile-high segment wraps the scrolled texture once: the top part shows the
            // tile's start, the bottom part its end.
            let split = bottom + (1.0 - scroll);
            let parts = [
                (
                    bottom,
                    split,
                    rect.y + scroll * rect.height,
                    rect.y + rect.height,
                ),
                (split, bottom + 1.0, rect.y, rect.y + scroll * rect.height),
            ];
            for (low, high, v_top, v_bottom) in parts {
                if high - low <= 1.0e-4 {
                    continue;
                }
                for corners in sides(low, high) {
                    builder.quad(
                        Layer::Overlay,
                        corners,
                        [rect.x, v_top, rect.x + rect.width, v_bottom],
                        color,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_wraps_each_loop_and_stays_in_the_unit_range() {
        assert_eq!(scroll_fraction(0.0), 0.0);
        assert!((scroll_fraction(SCROLL_TICKS_PER_LOOP * 0.25) - 0.25).abs() < 1.0e-6);
        assert!(scroll_fraction(SCROLL_TICKS_PER_LOOP * 3.0).abs() < 1.0e-6);
        assert!((0.0..1.0).contains(&scroll_fraction(-7.3)));
    }
}
