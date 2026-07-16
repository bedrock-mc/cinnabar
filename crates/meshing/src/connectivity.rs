use std::collections::VecDeque;

use crate::{
    Face, FaceConnectivity, SIDE,
    contributors::{PaletteFacts, PaletteSource, ResolvedPaletteEntry},
};
use assets::BlockFlags;

const fn connectivity_open(entry: ResolvedPaletteEntry) -> bool {
    !entry.flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
}

pub(crate) fn cave_connectivity(facts: &PaletteFacts<'_>) -> FaceConnectivity {
    match &facts.source {
        PaletteSource::Air => return FaceConnectivity::all(),
        PaletteSource::Uniform(contributors) => {
            return if connectivity_open(contributors.geometry_entry()) {
                FaceConnectivity::all()
            } else {
                FaceConnectivity::none()
            };
        }
        PaletteSource::Mixed(_) => {}
    }

    let mut connectivity = FaceConnectivity::none();
    let mut visited = [0_u64; 64];
    let mut queue = VecDeque::new();

    for seed in 0..4096_usize {
        if bit_is_set(&visited, seed) {
            continue;
        }
        let coordinate = coordinate_from_linear(seed);
        if !connectivity_open(facts.at(coordinate[0], coordinate[1], coordinate[2])) {
            continue;
        }

        set_bit(&mut visited, seed);
        queue.push_back(seed as u16);
        let mut touched = 0_u8;

        while let Some(linear) = queue.pop_front() {
            let [x, y, z] = coordinate_from_linear(usize::from(linear));
            touched |= touched_faces(x, y, z);

            for neighbour in adjacent_coordinates(x, y, z).into_iter().flatten() {
                let neighbour_linear = linear_from_coordinate(neighbour);
                if bit_is_set(&visited, neighbour_linear)
                    || !connectivity_open(facts.at(neighbour[0], neighbour[1], neighbour[2]))
                {
                    continue;
                }
                set_bit(&mut visited, neighbour_linear);
                queue.push_back(neighbour_linear as u16);
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
