//! Carried sensor bodies, inactive tendrils and calibration crystals.

use super::{GuiBlockQuad, cube_face};
use crate::BlockFace;

pub const SENSOR_TENDRIL_MATERIAL: u32 = BlockFace::ALL.len() as u32;
pub const SENSOR_CRYSTAL_MATERIAL: u32 = SENSOR_TENDRIL_MATERIAL + 1;
/// Inactive tendril vertex brightness after conversion to an atlas color byte.
pub const SENSOR_TENDRIL_BRIGHTNESS: u8 = 140;

/// Emits the sensor's carried surfaces and whether each receives ordinary face shading.
#[must_use]
pub fn sculk_sensor_quads(calibrated: bool) -> Vec<(GuiBlockQuad, bool)> {
    let mut out = Vec::new();
    for face in BlockFace::ALL {
        let (corners, mut uvs) = cube_face(face);
        if face.is_horizontal() {
            for uv in &mut uvs {
                uv[1] = 0.5 + uv[1] * 0.5;
            }
        }
        out.push((
            GuiBlockQuad {
                corners: corners.map(|[x, y, z]| [x, y * 0.5, z]),
                uvs,
                material: face as u32,
            },
            true,
        ));
    }
    for ([x, z], slope) in [
        ([3.0 / 16.0, 13.0 / 16.0], 1.0),
        ([13.0 / 16.0, 13.0 / 16.0], -1.0),
        ([3.0 / 16.0, 3.0 / 16.0], -1.0),
        ([13.0 / 16.0, 3.0 / 16.0], 1.0),
    ] {
        out.push((
            plane(
                [x - 0.36, z - slope * 0.36],
                [x + 0.36, z + slope * 0.36],
                SENSOR_TENDRIL_MATERIAL,
            ),
            false,
        ));
    }
    if calibrated {
        for (a, b) in [([0.05, 0.05], [0.95, 0.95]), ([0.05, 0.95], [0.95, 0.05])] {
            out.push((plane(a, b, SENSOR_CRYSTAL_MATERIAL), false));
        }
    }
    out
}

/// Creates a two-sided carried attachment plane above the half-height body.
fn plane([ax, az]: [f32; 2], [bx, bz]: [f32; 2], material: u32) -> GuiBlockQuad {
    GuiBlockQuad {
        corners: [[ax, 1.5, az], [ax, 0.5, az], [bx, 0.5, bz], [bx, 1.5, bz]],
        uvs: [[0., 0.], [0., 1.], [1., 1.], [1., 0.]],
        material,
    }
}
