use super::super::*;

#[derive(Clone, Copy)]
pub(in crate::compiler) struct FlowerBedQuad {
    positions: [[i16; 3]; 4],
    uvs: [[u16; 2]; 4],
    stem: bool,
}

#[derive(Clone, Copy)]
pub(in crate::compiler) struct FlowerBedPatch {
    quads: &'static [FlowerBedQuad],
}

const FLOWERBED_PATCH_1: [FlowerBedQuad; 7] = [
    flowerbed_quad(
        [[0, 48, 0], [128, 48, 0], [128, 48, 128], [0, 48, 128]],
        [[0, 0], [2048, 0], [2048, 2048], [0, 2048]],
        false,
    ),
    flowerbed_quad(
        [[77, 0, 19], [66, 0, 30], [66, 48, 30], [77, 48, 19]],
        stem_uv(1024),
        true,
    ),
    flowerbed_quad(
        [[66, 0, 19], [77, 0, 30], [77, 48, 30], [66, 48, 19]],
        stem_uv(1024),
        true,
    ),
    flowerbed_quad(
        [[29, 0, 81], [18, 0, 93], [18, 48, 93], [29, 48, 81]],
        stem_uv(1024),
        true,
    ),
    flowerbed_quad(
        [[18, 0, 81], [29, 0, 93], [29, 48, 93], [18, 48, 81]],
        stem_uv(1024),
        true,
    ),
    flowerbed_quad(
        [[109, 0, 98], [97, 0, 110], [97, 48, 110], [109, 48, 98]],
        stem_uv(1024),
        true,
    ),
    flowerbed_quad(
        [[97, 0, 98], [109, 0, 110], [109, 48, 110], [97, 48, 98]],
        stem_uv(1024),
        true,
    ),
];

const FLOWERBED_PATCH_2: [FlowerBedQuad; 3] = [
    flowerbed_quad(
        [[0, 16, 128], [128, 16, 128], [128, 16, 256], [0, 16, 256]],
        [[0, 2048], [2048, 2048], [2048, 4096], [0, 4096]],
        false,
    ),
    flowerbed_quad(
        [[67, 0, 179], [78, 0, 190], [78, 16, 190], [67, 16, 179]],
        stem_uv(1536),
        true,
    ),
    flowerbed_quad(
        [[78, 0, 179], [67, 0, 190], [67, 16, 190], [78, 16, 179]],
        stem_uv(1536),
        true,
    ),
];

const FLOWERBED_PATCH_3: [FlowerBedQuad; 7] = [
    flowerbed_quad(
        [
            [128, 32, 128],
            [256, 32, 128],
            [256, 32, 256],
            [128, 32, 256],
        ],
        [[2048, 2048], [4096, 2048], [4096, 4096], [2048, 4096]],
        false,
    ),
    flowerbed_quad(
        [[186, 0, 218], [198, 0, 229], [198, 32, 229], [186, 32, 218]],
        stem_uv(1280),
        true,
    ),
    flowerbed_quad(
        [[198, 0, 218], [186, 0, 229], [186, 32, 229], [198, 32, 218]],
        stem_uv(1280),
        true,
    ),
    flowerbed_quad(
        [[238, 0, 162], [226, 0, 173], [226, 32, 173], [238, 32, 162]],
        stem_uv(1280),
        true,
    ),
    flowerbed_quad(
        [[226, 0, 162], [238, 0, 173], [238, 32, 173], [226, 32, 162]],
        stem_uv(1280),
        true,
    ),
    flowerbed_quad(
        [[157, 0, 146], [146, 0, 157], [146, 32, 157], [157, 32, 146]],
        stem_uv(1280),
        true,
    ),
    flowerbed_quad(
        [[146, 0, 146], [157, 0, 157], [157, 32, 157], [146, 32, 146]],
        stem_uv(1280),
        true,
    ),
];

const FLOWERBED_PATCH_4: [FlowerBedQuad; 3] = [
    flowerbed_quad(
        [[128, 32, 0], [256, 32, 0], [256, 32, 128], [128, 32, 128]],
        [[2048, 0], [4096, 0], [4096, 2048], [2048, 2048]],
        false,
    ),
    flowerbed_quad(
        [[189, 0, 50], [177, 0, 62], [177, 32, 62], [189, 32, 50]],
        stem_uv(1280),
        true,
    ),
    flowerbed_quad(
        [[177, 0, 50], [189, 0, 62], [189, 32, 62], [177, 32, 50]],
        stem_uv(1280),
        true,
    ),
];

const FLOWERBED_PATCHES: [FlowerBedPatch; 4] = [
    FlowerBedPatch {
        quads: &FLOWERBED_PATCH_1,
    },
    FlowerBedPatch {
        quads: &FLOWERBED_PATCH_2,
    },
    FlowerBedPatch {
        quads: &FLOWERBED_PATCH_3,
    },
    FlowerBedPatch {
        quads: &FLOWERBED_PATCH_4,
    },
];

pub(in crate::compiler) const fn flowerbed_quad(
    positions: [[i16; 3]; 4],
    uvs: [[u16; 2]; 4],
    stem: bool,
) -> FlowerBedQuad {
    FlowerBedQuad {
        positions,
        uvs,
        stem,
    }
}

pub(in crate::compiler) const fn stem_uv(min_v: u16) -> [[u16; 2]; 4] {
    [[0, 1792], [256, 1792], [256, min_v], [0, min_v]]
}

pub(in crate::compiler) fn flowerbed_quads(
    materials: [u32; 2],
    growth: u32,
    orientation: u32,
) -> Result<Vec<ModelQuad>, AssetError> {
    let patch_count = usize::try_from(growth + 1).map_err(|_| AssetError::BlobSizeOverflow {
        section: "flowerbed patch count",
    })?;
    let patches =
        FLOWERBED_PATCHES
            .get(..patch_count)
            .ok_or_else(|| AssetError::InvalidCompiledAssets {
                detail: format!("flowerbed growth {growth} is not a normal state").into(),
            })?;
    if orientation > 3 {
        return Err(AssetError::InvalidCompiledAssets {
            detail: format!("flowerbed orientation {orientation} is not cardinal").into(),
        });
    }
    let quad_count = patches.iter().map(|patch| patch.quads.len()).sum();
    if quad_count > 32 {
        return Err(AssetError::InvalidCompiledAssets {
            detail: format!("flowerbed template has {quad_count} quads").into(),
        });
    }
    let mut quads = Vec::with_capacity(quad_count);
    for source in patches.iter().flat_map(|patch| patch.quads) {
        let mut positions = source.positions;
        for position in &mut positions {
            *position = rotate_flowerbed_position(*position, orientation)?;
            if position[1] >= 64 {
                return Err(AssetError::InvalidCompiledAssets {
                    detail: "flowerbed template exceeded the near-ground bound".into(),
                });
            }
        }
        quads.push(ModelQuad {
            positions,
            uvs: source.uvs,
            material: materials[usize::from(source.stem)],
            flags: MODEL_QUAD_FLAG_TWO_SIDED,
        });
    }
    Ok(quads)
}

pub(in crate::compiler) fn rotate_flowerbed_position(
    [x, y, z]: [i16; 3],
    orientation: u32,
) -> Result<[i16; 3], AssetError> {
    if !(0..=256).contains(&x) || !(0..=256).contains(&z) {
        return Err(AssetError::InvalidCompiledAssets {
            detail: format!("flowerbed source position ({x}, {z}) is outside one block").into(),
        });
    }
    let complement = |value: i16| {
        i16::try_from(256_i32 - i32::from(value)).map_err(|_| AssetError::BlobSizeOverflow {
            section: "flowerbed rotated position",
        })
    };
    match orientation {
        0 => Ok([complement(x)?, y, complement(z)?]),
        1 => Ok([z, y, complement(x)?]),
        2 => Ok([x, y, z]),
        3 => Ok([complement(z)?, y, x]),
        _ => Err(AssetError::InvalidCompiledAssets {
            detail: format!("flowerbed orientation {orientation} is not cardinal").into(),
        }),
    }
}
