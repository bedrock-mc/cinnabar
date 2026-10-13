use crate::{
    Face, FaceConnectivity, SIDE,
    contributors::{PaletteFacts, PaletteSource, ResolvedPaletteEntry},
};
use assets::BlockFlags;

const fn connectivity_open(entry: ResolvedPaletteEntry) -> bool {
    !entry.flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
}

/// Faces of the section joined through voxels that do not fully occlude. `open` answers a mixed
/// section's voxels and must agree with [`PaletteFacts::at`] lacking `OCCLUDES_FULL_FACE`.
pub(crate) fn cave_connectivity(
    facts: &PaletteFacts<'_>,
    open: impl Fn([usize; 3]) -> bool,
) -> FaceConnectivity {
    match &facts.source {
        PaletteSource::Air => FaceConnectivity::all(),
        PaletteSource::Uniform(contributors) => {
            if connectivity_open(contributors.geometry_entry()) {
                FaceConnectivity::all()
            } else {
                FaceConnectivity::none()
            }
        }
        PaletteSource::Mixed(_) => connectivity_through(open),
    }
}

/// Connects the faces each component of `open` voxels touches, walking every voxel at most once.
fn connectivity_through(open: impl Fn([usize; 3]) -> bool) -> FaceConnectivity {
    let mut connectivity = FaceConnectivity::none();
    let mut visited = [0_u64; 64];
    // Each voxel is pushed at most once, so the stack never outgrows the section.
    let mut stack = [0_u16; SIDE * SIDE * SIDE];

    for seed in 0..SIDE * SIDE * SIDE {
        if bit_is_set(&visited, seed) || !open(coordinate_from_linear(seed)) {
            continue;
        }
        set_bit(&mut visited, seed);
        stack[0] = seed as u16;
        let mut len = 1;
        let mut touched = 0_u8;

        while len != 0 {
            len -= 1;
            let [x, y, z] = coordinate_from_linear(usize::from(stack[len]));
            touched |= touched_faces(x, y, z);

            for neighbour in adjacent_coordinates(x, y, z).into_iter().flatten() {
                let neighbour_linear = linear_from_coordinate(neighbour);
                if bit_is_set(&visited, neighbour_linear) || !open(neighbour) {
                    continue;
                }
                set_bit(&mut visited, neighbour_linear);
                stack[len] = neighbour_linear as u16;
                len += 1;
            }
        }
        connectivity.connect_touched_faces(touched);
    }
    connectivity
}

const fn touched_faces(x: usize, y: usize, z: usize) -> u8 {
    let mut touched = 0_u8;
    if x == 0 {
        touched |= 1 << Face::NegativeX.index();
    }
    if x == SIDE - 1 {
        touched |= 1 << Face::PositiveX.index();
    }
    if y == 0 {
        touched |= 1 << Face::NegativeY.index();
    }
    if y == SIDE - 1 {
        touched |= 1 << Face::PositiveY.index();
    }
    if z == 0 {
        touched |= 1 << Face::NegativeZ.index();
    }
    if z == SIDE - 1 {
        touched |= 1 << Face::PositiveZ.index();
    }
    touched
}

fn adjacent_coordinates(x: usize, y: usize, z: usize) -> [Option<[usize; 3]>; 6] {
    [
        (x > 0).then_some([x.saturating_sub(1), y, z]),
        (x + 1 < SIDE).then_some([x + 1, y, z]),
        (y > 0).then_some([x, y.saturating_sub(1), z]),
        (y + 1 < SIDE).then_some([x, y + 1, z]),
        (z > 0).then_some([x, y, z.saturating_sub(1)]),
        (z + 1 < SIDE).then_some([x, y, z + 1]),
    ]
}

const fn linear_from_coordinate([x, y, z]: [usize; 3]) -> usize {
    (x << 8) | (z << 4) | y
}

const fn coordinate_from_linear(linear: usize) -> [usize; 3] {
    [linear >> 8, linear & 0x0f, (linear >> 4) & 0x0f]
}

fn bit_is_set(bits: &[u64; 64], index: usize) -> bool {
    bits[index / 64] & (1_u64 << (index % 64)) != 0
}

fn set_bit(bits: &mut [u64; 64], index: usize) {
    bits[index / 64] |= 1_u64 << (index % 64);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Faces each walk joins, for occupancy patterns whose components are known.
    #[test]
    fn walk_joins_exactly_the_faces_each_open_component_touches() {
        assert!(connectivity_through(|_| true).is_all_connected());
        assert!(connectivity_through(|_| false).is_empty());

        let wall = connectivity_through(|[x, _, _]| x != 8);
        assert!(!wall.is_connected(Face::NegativeX, Face::PositiveX));
        for side in [
            Face::NegativeY,
            Face::PositiveY,
            Face::NegativeZ,
            Face::PositiveZ,
        ] {
            assert!(wall.is_connected(Face::NegativeX, side));
            assert!(wall.is_connected(Face::PositiveX, side));
        }

        let tunnel = connectivity_through(|[x, y, _]| x == 3 && y == 3);
        assert!(tunnel.is_connected(Face::NegativeZ, Face::PositiveZ));
        assert!(!tunnel.is_connected(Face::NegativeZ, Face::NegativeX));
        assert!(!tunnel.is_connected(Face::NegativeY, Face::PositiveY));

        // An enclosed pocket touches no face and connects nothing.
        let pocket = connectivity_through(|[x, y, z]| (6..9).contains(&x) && y == 7 && z == 7);
        assert!(pocket.is_empty());
    }
}
