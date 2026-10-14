//! Beacon beam: two nested translucent columns rising from the beacon, with the texture
//! scrolling up the beam.
//!
//! Radii, alpha and scroll speed need native measurement.

use std::sync::Arc;

use assets::beacon::BeaconBeamSection;
use glam::Vec3;

use super::{atlas::BlockEntityAtlas, mesh::MeshBuilder, scene::SceneClock};

const INNER_RADIUS: f32 = 0.2;
const OUTER_RADIUS: f32 = 0.25;
const INNER_ALPHA: f32 = 0.8;
const OUTER_ALPHA: f32 = 0.3;
const SCROLL_TICKS_PER_LOOP: f64 = 40.0;

#[derive(Clone, Debug, PartialEq)]
pub struct BeaconModel {
    /// Change-driven column sections, from the beacon upward.
    pub sections: Arc<[BeaconBeamSection]>,
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
    if model.sections.is_empty() {
        return;
    }
    let Some(texture) = atlas.texture(
        assets::block_entity_geometry::textures::AtlasTexture::EntityBeaconBeam.path(),
        [16.0, 16.0],
    ) else {
        return;
    };
    let center = Vec3::new(
        block[0] as f32 + 0.5,
        block[1] as f32,
        block[2] as f32 + 0.5,
    );
    let scroll = scroll_fraction(clock.ticks);
    let rect = texture.rect;
    for (radius, alpha) in [(INNER_RADIUS, INNER_ALPHA), (OUTER_RADIUS, OUTER_ALPHA)] {
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
        let mut bottom = center.y;
        for section in model.sections.iter() {
            let high = bottom + section.height as f32;
            let tint = section.tint.map(|value| {
                if value <= 0.04045 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            });
            let color = [tint[0], tint[1], tint[2], alpha];
            for corners in sides(bottom, high) {
                builder.repeating_beam_quad(corners, rect, color, scroll);
            }
            bottom = high;
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
