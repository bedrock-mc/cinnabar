//! Finite, camera-facing clouds for the optional world style.

use crate::{CloudFace, cloud_viewport::ViewportCloudQuad};
use assets::AtmosphereTexture;

pub const CELL_BLOCKS: f32 = 12.0;
pub const THICKNESS: f32 = 4.0;
pub const SCROLL_PER_TICK: f64 = 0.030_000_001;
pub const Z_OFFSET: f32 = 3.96;
pub const INSIDE_FACE: u32 = 1 << 4;

/// An immutable, bounded cell window and camera position relative to its layer.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct JavaCloudViewport {
    pub center: [i32; 2],
    radius: i32,
    vertical: i8,
}

impl JavaCloudViewport {
    /// Rejects non-finite cameras and windows too large for a bounded mesh allocation.
    pub fn try_new(sample: [f64; 2], camera_y: f32, bottom_y: f32, range: f32) -> Option<Self> {
        if !camera_y.is_finite()
            || !bottom_y.is_finite()
            || !range.is_finite()
            || !(1.0..=2048.0).contains(&range)
        {
            return None;
        }
        let radius = (range / CELL_BLOCKS).ceil() as i32;
        let cells = sample.map(|coordinate| (coordinate / f64::from(CELL_BLOCKS)).floor());
        if cells.iter().any(|&cell| {
            !cell.is_finite()
                || cell < f64::from(i32::MIN + radius + 1)
                || cell > f64::from(i32::MAX - radius - 1)
        }) {
            return None;
        }
        Some(Self {
            center: cells.map(|cell| cell as i32),
            radius,
            vertical: if camera_y > bottom_y + THICKNESS {
                1
            } else if camera_y < bottom_y {
                -1
            } else {
                0
            },
        })
    }

    /// Tests whether a cached mesh has the same layer side, radius and adjacent center.
    pub fn is_neighbour_of(self, other: Self) -> bool {
        self.radius == other.radius
            && self.vertical == other.vertical
            && self
                .center
                .iter()
                .zip(other.center)
                .all(|(old, new)| old.abs_diff(new) <= 1)
    }

    /// Produces one neighbouring window for bounded speculative meshing.
    pub fn shifted(self, x: i32, z: i32) -> Self {
        Self {
            center: [self.center[0] + x, self.center[1] + z],
            ..self
        }
    }
}

/// Emits near-to-far cell faces, retaining only exposed sides facing the camera.
pub fn mesh(texture: &AtmosphereTexture, viewport: JavaCloudViewport) -> Vec<ViewportCloudQuad> {
    let width = texture.width as i32;
    let height = texture.height as i32;
    if width == 0 || height == 0 || texture.rgba8.len() != width as usize * height as usize * 4 {
        return Vec::new();
    }
    let pixel = |x: i32, z: i32| {
        let offset =
            (z.rem_euclid(height) as usize * width as usize + x.rem_euclid(width) as usize) * 4;
        &texture.rgba8[offset..offset + 4]
    };
    let occupied = |x, z| pixel(x, z)[3] >= 10;
    let mut records = Vec::new();
    for ring in 0..=viewport.radius * 2 {
        for x in -ring..=ring {
            let z = ring - x.abs();
            if z > viewport.radius || x * x + z * z > viewport.radius * viewport.radius {
                continue;
            }
            for z in [(z != 0).then_some(-z), Some(z)].into_iter().flatten() {
                let cell = [viewport.center[0] + x, viewport.center[1] + z];
                if !occupied(cell[0], cell[1]) {
                    continue;
                }
                let rgba = pixel(cell[0], cell[1]);
                let colour = u32::from_le_bytes([rgba[0], rgba[1], rgba[2], 255]);
                let mut push = |face: CloudFace, inside: bool| {
                    records.push(ViewportCloudQuad {
                        cell,
                        face: face as u32 | if inside { INSIDE_FACE } else { 0 },
                        colour,
                    })
                };
                if viewport.vertical >= 0 {
                    push(CloudFace::Up, false);
                }
                if viewport.vertical <= 0 {
                    push(CloudFace::Down, false);
                }
                for (face, delta, facing) in [
                    (CloudFace::North, [0, -1], z > 0),
                    (CloudFace::South, [0, 1], z < 0),
                    (CloudFace::West, [-1, 0], x > 0),
                    (CloudFace::East, [1, 0], x < 0),
                ] {
                    if facing && !occupied(cell[0] + delta[0], cell[1] + delta[1]) {
                        push(face, false);
                    }
                }
                if x.abs() <= 1 && z.abs() <= 1 {
                    for face in [
                        CloudFace::Down,
                        CloudFace::Up,
                        CloudFace::North,
                        CloudFace::South,
                        CloudFace::West,
                        CloudFace::East,
                    ] {
                        push(face, true);
                    }
                }
            }
        }
    }
    records
}

#[cfg(test)]
mod tests {
    use super::*;
    use assets::AtmosphereRole;

    #[test]
    fn occupied_threshold_caps_and_interior_winding_are_independent() {
        let texture = AtmosphereTexture {
            role: AtmosphereRole::Clouds,
            source_path: "test".into(),
            source_bytes: 4,
            source_sha256: [0; 32],
            pixels_sha256: [0; 32],
            width: 1,
            height: 1,
            rgba8: vec![255, 255, 255, 10].into_boxed_slice(),
        };
        let view = JavaCloudViewport::try_new([0.0, 0.0], 0.0, 100.0, 12.0).unwrap();
        let records = mesh(&texture, view);
        assert_eq!(
            records
                .iter()
                .filter(|r| r.face == CloudFace::Down as u32)
                .count(),
            5
        );
        assert_eq!(
            records
                .iter()
                .filter(|r| r.face == CloudFace::Up as u32)
                .count(),
            0
        );
        assert_eq!(
            records.iter().filter(|r| r.face & INSIDE_FACE != 0).count(),
            30
        );
        let mut transparent = texture.clone();
        transparent.rgba8[3] = 9;
        assert!(mesh(&transparent, view).is_empty());
    }

    #[test]
    fn mesh_preserves_wrapped_texel_rgb_on_every_face() {
        let texture = AtmosphereTexture {
            role: AtmosphereRole::Clouds,
            source_path: "fixture/coloured-clouds.png".into(),
            source_bytes: 16,
            source_sha256: [0; 32],
            pixels_sha256: [0; 32],
            width: 2,
            height: 2,
            rgba8: vec![
                32, 96, 192, 10, 16, 64, 128, 255, 0, 0, 0, 9, 224, 160, 80, 128,
            ]
            .into_boxed_slice(),
        };
        for camera_y in [99.0, 102.0, 105.0] {
            let view = JavaCloudViewport::try_new([-0.1, -12.1], camera_y, 100.0, 36.0).unwrap();
            let records = mesh(&texture, view);
            assert!(!records.is_empty());
            for record in records {
                let [x, z] = record
                    .cell
                    .map(|coordinate| coordinate.rem_euclid(2) as usize);
                let pixel = &texture.rgba8[(z * 2 + x) * 4..][..4];
                assert!(pixel[3] >= 10, "transparent cells must remain empty");
                assert_eq!(
                    record.colour.to_le_bytes(),
                    [pixel[0], pixel[1], pixel[2], 255]
                );
            }
        }
    }

    #[test]
    fn view_rejects_invalid_coordinates_and_range() {
        assert!(JavaCloudViewport::try_new([f64::NAN, 0.0], 0.0, 100.0, 12.0).is_none());
        assert!(JavaCloudViewport::try_new([0.0; 2], 0.0, 100.0, f32::INFINITY).is_none());
        assert_eq!(
            JavaCloudViewport::try_new([-0.1, -12.1], 0.0, 100.0, 12.0)
                .unwrap()
                .center,
            [-1, -2]
        );
    }
}
