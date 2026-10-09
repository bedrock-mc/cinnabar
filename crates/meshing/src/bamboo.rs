pub const STEM_SIDE_QUAD_MASK: u32 = (1 << assets::BlockFace::West as u32)
    | (1 << assets::BlockFace::East as u32)
    | (1 << assets::BlockFace::North as u32)
    | (1 << assets::BlockFace::South as u32);
pub const STEM_UV_STRIDE: f32 = 3.0 / 16.0;
pub const LEAF_PLANE_INSET: f32 = 0.01;
pub const POSITIVE_X_LEAF_QUAD: u32 = assets::BlockFace::ALL.len() as u32;
pub const POSITIVE_Z_LEAF_QUAD: u32 = POSITIVE_X_LEAF_QUAD + 2;

/// Resolves the packed variant shared by terrain and position-dependent overlays.
pub fn transform_for_template(flags: u32, variant: u32, position: [i32; 3]) -> u32 {
    if flags & assets::MODEL_TEMPLATE_FLAG_BAMBOO != 0 {
        block_transform::bamboo::column_transform(position[0], position[2])
    } else {
        variant
    }
}

/// Column-dependent U displacement retained when the stem uses a texture override.
pub fn stem_uv_offset(transform: u32, quad: u32) -> f32 {
    if quad < assets::BlockFace::ALL.len() as u32 && (STEM_SIDE_QUAD_MASK >> quad) & 1 != 0 {
        ((transform >> 8) & 3) as f32 * STEM_UV_STRIDE
    } else {
        0.0
    }
}

/// Block-local displacement for a bamboo model quad, including its leaf plane inset.
pub fn quad_offset(transform: u32, quad: u32) -> [f32; 3] {
    let mut offset = block_transform::bamboo::offset_from_transform(transform);
    if quad == POSITIVE_X_LEAF_QUAD {
        offset[2] += LEAF_PLANE_INSET;
    }
    if quad == POSITIVE_Z_LEAF_QUAD {
        offset[0] += LEAF_PLANE_INSET;
    }
    offset
}
