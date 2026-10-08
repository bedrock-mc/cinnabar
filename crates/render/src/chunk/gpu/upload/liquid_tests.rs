use meshing::liquid::{LIQUID_FACE_INSET, LIQUID_TOP_INSET_BIT};
use meshing::{Face, PackedLiquidQuad};

use super::liquid_quad_centroid;

#[test]
fn liquid_centroids_match_native_inward_faces_and_conditional_top_height_mutation() {
    let inset = LIQUID_FACE_INSET;
    for top_emitted in [false, true] {
        let top_inset = if top_emitted { inset } else { 0.0 };
        let side_height = 4.5 - top_inset * 0.5;
        for (face, expected) in [
            (Face::NegativeX, [3.0 + inset, side_height, 5.5]),
            (Face::PositiveX, [4.0 - inset, side_height, 5.5]),
            (Face::NegativeY, [3.5, 4.0, 5.5]),
            (Face::PositiveY, [3.5, 5.0 - top_inset, 5.5]),
            (Face::NegativeZ, [3.5, side_height, 5.0 + inset]),
            (Face::PositiveZ, [3.5, side_height, 6.0 - inset]),
        ] {
            let heights = match face {
                Face::NegativeY => [0; 4],
                Face::PositiveY => [255; 4],
                _ => [0, 255, 255, 0],
            };
            let mut words =
                PackedLiquidQuad::try_pack([3, 4, 5], face, heights, 7, 11, [0, 0], false)
                    .expect("bounded liquid record")
                    .words();
            if top_emitted {
                words[2] |= LIQUID_TOP_INSET_BIT;
            }
            let quad = PackedLiquidQuad::try_from_words(words).expect("valid flagged record");
            let chunk_origin = [16, 32, 48];
            let actual = liquid_quad_centroid(chunk_origin, quad);
            for axis in 0..3 {
                assert!(
                    (actual[axis] - (expected[axis] + chunk_origin[axis] as f32)).abs() < 0.000_01,
                    "{face:?}, top_emitted={top_emitted}, axis={axis}: {actual:?}",
                );
            }
        }
    }
}
