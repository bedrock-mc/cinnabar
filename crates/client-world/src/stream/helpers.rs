use super::*;

pub(super) fn prepare_sub_chunks(batch: SubChunkBatchEvent) -> Vec<PreparedSubChunk> {
    let dimension = batch.dimension;
    batch
        .entries
        .into_iter()
        .map(|entry| {
            let key = SubChunkKey::new(
                dimension,
                entry.position[0],
                entry.position[1],
                entry.position[2],
            );
            PreparedSubChunk {
                position: entry.position,
                result: match entry.result {
                    SubChunkResult::Success { payload } => {
                        PreparedSubChunkResult::Decoded(DecodedSubChunk::decode(key, &payload))
                    }
                    SubChunkResult::AllAir => PreparedSubChunkResult::AllAir,
                    SubChunkResult::Unavailable(unavailable) => {
                        PreparedSubChunkResult::Unavailable(unavailable)
                    }
                },
            }
        })
        .collect()
}

pub(super) fn block_entity_y_is_valid(dimension: i32, y: i32) -> bool {
    let Some(range) = vanilla_dimension_range(dimension) else {
        return false;
    };
    let sub_chunk_y = y.div_euclid(16);
    sub_chunk_y >= range.base_sub_chunk_y
        && sub_chunk_y
            < range.base_sub_chunk_y
                + i32::try_from(range.sub_chunk_count)
                    .expect("vanilla dimension subchunk counts fit i32")
}

pub(super) fn distance_squared(key: SubChunkKey, camera: [f32; 3]) -> f32 {
    let center = [
        key.x as f32 * 16.0 + 8.0,
        key.y as f32 * 16.0 + 8.0,
        key.z as f32 * 16.0 + 8.0,
    ];
    let dx = center[0] - camera[0];
    let dy = center[1] - camera[1];
    let dz = center[2] - camera[2];
    dx.mul_add(dx, dy.mul_add(dy, dz * dz))
}

pub(super) fn light_bounds(key: SubChunkKey) -> Option<LightBounds> {
    let min = BlockPos::new(
        key.x.checked_mul(16)?,
        key.y.checked_mul(16)?,
        key.z.checked_mul(16)?,
    );
    LightBounds::new(
        key.dimension,
        min,
        BlockPos::new(
            min.x.checked_add(15)?,
            min.y.checked_add(15)?,
            min.z.checked_add(15)?,
        ),
    )
    .ok()
}

pub(super) fn offset_sub_chunk_key(
    key: SubChunkKey,
    [dx, dy, dz]: [i32; 3],
) -> Option<SubChunkKey> {
    Some(SubChunkKey::new(
        key.dimension,
        key.x.checked_add(dx)?,
        key.y.checked_add(dy)?,
        key.z.checked_add(dz)?,
    ))
}

pub(super) fn light_face_changed(
    previous: Option<&SubChunkLight>,
    replacement: &SubChunkLight,
    previous_direct: Option<&DirectSkyMask>,
    replacement_direct: &DirectSkyMask,
    offset: [i32; 3],
) -> bool {
    let uniform_channel = |light: Option<&SubChunkLight>, channel| match light {
        None => Some(0),
        Some(light) if light.channel(channel).is_uniform() => light.get(channel, 0, 0, 0),
        Some(_) => None,
    };
    let uniform_direct = |direct: Option<&DirectSkyMask>| match direct {
        None | Some(DirectSkyMask::Uniform(false)) => Some(false),
        Some(DirectSkyMask::Uniform(true)) => Some(true),
        Some(DirectSkyMask::Packed(_)) => None,
    };
    if let (
        Some(block_before),
        Some(block_after),
        Some(sky_before),
        Some(sky_after),
        Some(direct_before),
        Some(direct_after),
    ) = (
        uniform_channel(previous, LightChannel::Block),
        uniform_channel(Some(replacement), LightChannel::Block),
        uniform_channel(previous, LightChannel::Sky),
        uniform_channel(Some(replacement), LightChannel::Sky),
        uniform_direct(previous_direct),
        uniform_direct(Some(replacement_direct)),
    ) {
        return block_before != block_after
            || sky_before != sky_after
            || direct_before != direct_after;
    }
    for a in 0_u8..16 {
        for b in 0_u8..16 {
            let [x, y, z] = match offset {
                [-1, 0, 0] => [0, a, b],
                [1, 0, 0] => [15, a, b],
                [0, -1, 0] => [a, 0, b],
                [0, 1, 0] => [a, 15, b],
                [0, 0, -1] => [a, b, 0],
                [0, 0, 1] => [a, b, 15],
                _ => return false,
            };
            for channel in [LightChannel::Block, LightChannel::Sky] {
                let before = previous
                    .and_then(|light| light.get(channel, x, y, z))
                    .unwrap_or(0);
                let after = replacement.get(channel, x, y, z).unwrap_or(0);
                if before != after {
                    return true;
                }
            }
            if previous_direct.is_some_and(|direct| direct.get(x, y, z))
                != replacement_direct.get(x, y, z)
            {
                return true;
            }
        }
    }
    false
}

pub(super) fn light_levels_equal(left: &SubChunkLight, right: &SubChunkLight) -> bool {
    left.channel(LightChannel::Block) == right.channel(LightChannel::Block)
        && left.channel(LightChannel::Sky) == right.channel(LightChannel::Sky)
}

pub(super) fn is_uniform_direct_sky(light: &SubChunkLight, direct: &DirectSkyMask) -> bool {
    light.channel(LightChannel::Block).is_uniform()
        && light.get(LightChannel::Block, 0, 0, 0) == Some(0)
        && light.channel(LightChannel::Sky).is_uniform()
        && light.get(LightChannel::Sky, 0, 0, 0) == Some(15)
        && matches!(direct, DirectSkyMask::Uniform(true))
}

pub(super) fn deterministic_sub_chunk_key_hash(keys: &BTreeSet<SubChunkKey>) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

    keys.iter()
        .flat_map(|key| [key.dimension, key.x, key.y, key.z])
        .flat_map(i32::to_le_bytes)
        .fold(FNV_OFFSET_BASIS, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME)
        })
}

pub(super) fn floor_to_i32(value: f32) -> i32 {
    if value.is_nan() {
        0
    } else if value <= i32::MIN as f32 {
        i32::MIN
    } else if value >= i32::MAX as f32 {
        i32::MAX
    } else {
        value.floor() as i32
    }
}
