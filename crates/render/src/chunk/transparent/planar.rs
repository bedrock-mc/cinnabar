//! Transparent water whose faces can never overlap on screen blends the same in any order.
use crate::chunk::*;

/// Index of the axis a face's plane is perpendicular to.
const fn face_axis(face: Face) -> usize {
    match face {
        Face::NegativeX | Face::PositiveX => 0,
        Face::NegativeY | Face::PositiveY => 1,
        Face::NegativeZ | Face::PositiveZ => 2,
    }
}

/// Whether one liquid face is a rectangle in its plane: a level top or bottom, or a side
/// whose two top corners and two bottom corners are level.
fn liquid_face_is_rectangular(face: Face, heights: [u8; 4]) -> bool {
    match face {
        Face::NegativeY | Face::PositiveY => heights.iter().all(|&height| height == heights[0]),
        // Side corners run bottom, top, top, bottom.
        _ => heights[0] == heights[3] && heights[1] == heights[2],
    }
}

/// Returns whether quads occupy distinct cells in one plane with identical facing, heights and inset.
/// Such faces share exact whole edges, so each raster sample belongs to at most one face.
pub(in crate::chunk) fn liquid_quads_are_order_independent(quads: &[PackedLiquidQuad]) -> bool {
    let Some(&first) = quads.first() else {
        return true;
    };
    let (face, heights, inset) = (first.face(), first.heights(), first.has_top_height_inset());
    if !liquid_face_is_rectangular(face, heights) {
        return false;
    }
    let axis = face_axis(face);
    let plane = first.origin()[axis];
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let mut cells = [0_u64; 4];
    quads.iter().all(|quad| {
        let origin = quad.origin();
        let cell = usize::from(origin[u]) * 16 + usize::from(origin[v]);
        let (word, bit) = (cell / 64, 1_u64 << (cell % 64));
        let fresh = cells[word] & bit == 0;
        cells[word] |= bit;
        fresh
            && quad.face() == face
            && quad.heights() == heights
            && quad.has_top_height_inset() == inset
            && origin[axis] == plane
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Packs a liquid face with neutral lighting for planarity fixtures.
    fn quad(origin: [u8; 3], face: Face, heights: [u8; 4]) -> PackedLiquidQuad {
        PackedLiquidQuad::try_pack(origin, face, heights, 0, 0, [0; 2], false).unwrap()
    }

    /// Builds a complete level top surface at the selected packed height.
    fn surface(height: u8) -> Vec<PackedLiquidQuad> {
        (0..16)
            .flat_map(|z| (0..16).map(move |x| quad([x, 15, z], Face::PositiveY, [height; 4])))
            .collect()
    }

    #[test]
    fn level_surface_and_level_wall_need_no_face_order() {
        assert!(liquid_quads_are_order_independent(&[]));
        assert!(liquid_quads_are_order_independent(&surface(224)));
        let wall = (0..16)
            .flat_map(|y| (0..16).map(move |z| quad([3, y, z], Face::PositiveX, [0, 255, 255, 0])))
            .collect::<Vec<_>>();
        assert!(liquid_quads_are_order_independent(&wall));
    }

    #[test]
    fn faces_that_can_overlap_on_screen_keep_their_sort() {
        let mut shore = surface(224);
        shore.push(quad([0, 15, 0], Face::NegativeX, [0, 224, 224, 0]));
        let mut stepped = surface(224);
        stepped[17] = quad([1, 15, 1], Face::PositiveY, [200; 4]);
        let mut two_levels = surface(224);
        two_levels[17] = quad([1, 14, 1], Face::PositiveY, [224; 4]);
        let mut sloped = surface(224);
        sloped.iter_mut().for_each(|face| {
            *face = quad(face.origin(), Face::PositiveY, [224, 224, 200, 200]);
        });
        let mut duplicate = surface(224);
        duplicate.push(duplicate[5]);
        let mut both_sides = surface(224);
        both_sides.push(quad([4, 15, 4], Face::NegativeY, [224; 4]));
        let ragged_wall = [
            quad([3, 0, 0], Face::PositiveX, [0, 255, 255, 0]),
            quad([3, 0, 1], Face::PositiveX, [0, 255, 200, 0]),
        ];
        let mut inset = surface(224);
        inset[3] = PackedLiquidQuad::try_from_words({
            let mut words = inset[3].words();
            words[2] |= meshing::liquid::LIQUID_TOP_INSET_BIT;
            words
        })
        .unwrap();
        for quads in [
            &shore[..],
            &stepped,
            &two_levels,
            &sloped,
            &duplicate,
            &both_sides,
            &ragged_wall,
            &inset,
        ] {
            assert!(!liquid_quads_are_order_independent(quads));
        }
    }
}
