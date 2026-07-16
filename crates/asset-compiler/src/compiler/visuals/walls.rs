use super::super::*;

pub(in crate::compiler) const fn wall_state_is_valid(connections: u32) -> bool {
    connections & !0x1ff == 0
        && connections & 3 <= 2
        && (connections >> 2) & 3 <= 2
        && (connections >> 4) & 3 <= 2
        && (connections >> 6) & 3 <= 2
}

pub(in crate::compiler) fn wall_quads(materials: [u32; 6], connections: u32) -> Vec<ModelQuad> {
    debug_assert!(wall_state_is_valid(connections));
    let north = connections & 3;
    let east = (connections >> 2) & 3;
    let south = (connections >> 4) & 3;
    let west = (connections >> 6) & 3;
    let post = (connections >> 8) & 1;
    let height = |connection| match connection {
        1 => 224,
        2 => 256,
        _ => unreachable!("wall connection is checked before geometry generation"),
    };
    let mut quads = Vec::with_capacity(30);
    // Visible extents come from the local vanilla
    // template_wall_{post,side,side_tall}.json render models. Dragonfly's
    // broader Wall::BBox components are collision-only and not render authority.
    if post != 0 {
        quads.extend(cuboid_quads(materials, [64, 0, 64], [192, 256, 192]));
    }
    if north != 0 {
        quads.extend(cuboid_quads(
            materials,
            [80, 0, 0],
            [176, height(north), 128],
        ));
    }
    if east != 0 {
        quads.extend(cuboid_quads(
            materials,
            [128, 0, 80],
            [256, height(east), 176],
        ));
    }
    if south != 0 {
        quads.extend(cuboid_quads(
            materials,
            [80, 0, 128],
            [176, height(south), 256],
        ));
    }
    if west != 0 {
        quads.extend(cuboid_quads(
            materials,
            [0, 0, 80],
            [128, height(west), 176],
        ));
    }
    quads
}
