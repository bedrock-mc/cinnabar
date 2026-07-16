use super::super::*;

pub(in crate::compiler) fn fence_arm_quads(material: u32, mask: u32) -> Vec<ModelQuad> {
    debug_assert!(mask <= 15);
    let mut quads = Vec::with_capacity(mask.count_ones() as usize * 8);
    let directions = [
        (1, [112, 0, 0], [144, 0, 128]),
        (2, [128, 0, 112], [256, 0, 144]),
        (4, [112, 0, 128], [144, 0, 256]),
        (8, [0, 0, 112], [128, 0, 144]),
    ];
    for (bit, mut min, mut max) in directions {
        if mask & bit == 0 {
            continue;
        }
        let extension_axis = if bit == 1 || bit == 4 { 2 } else { 0 };
        for (min_y, max_y) in [(96, 144), (192, 240)] {
            min[1] = min_y;
            max[1] = max_y;
            for (face, quad) in cuboid_quads([material; 6], min, max)
                .into_iter()
                .enumerate()
            {
                let is_extension_cap = match extension_axis {
                    0 => matches!(face, 0 | 1),
                    _ => matches!(face, 4 | 5),
                };
                if !is_extension_cap {
                    quads.push(quad);
                }
            }
        }
    }
    quads
}
