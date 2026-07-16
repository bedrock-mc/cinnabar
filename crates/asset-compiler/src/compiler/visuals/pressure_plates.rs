use super::super::*;

pub(in crate::compiler) fn pressure_plate_quads(
    materials: [u32; 6],
    pressed: bool,
) -> [ModelQuad; 6] {
    // Visible geometry and UVs come from the local vanilla
    // pressure_plate_{up,down}.json models. The pressed side strip is
    // 15..15.5 pixels rather than the generic cuboid's 15.5..16 strip.
    let max_y = if pressed { 8 } else { 16 };
    let mut quads = cuboid_quads(materials, [16, 0, 16], [240, max_y, 240]);
    if pressed {
        for face in [
            BlockFace::West,
            BlockFace::East,
            BlockFace::North,
            BlockFace::South,
        ] {
            for uv in &mut quads[face as usize].uvs {
                uv[1] -= 128;
            }
        }
    }
    quads
}
