use render::{BedModel, BlockEntityLight};

pub(super) fn light(
    model: BedModel,
    position: [i32; 3],
    mut sample: impl FnMut([f32; 3]) -> (u8, u8),
) -> BlockEntityLight {
    let center = position.map(|value| value as f32 + 0.5);
    let offset = model.other_half_offset();
    let other = std::array::from_fn(|axis| center[axis] + offset[axis] as f32);
    let (block, sky) = sample(center);
    let (other_block, other_sky) = sample(other);
    BlockEntityLight::Actor {
        block: block.max(other_block),
        sky: sky.max(other_sky),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bed_lighting_keeps_the_brightest_block_and_sky_channel_from_both_halves() {
        for (direction, offset) in [
            (0, [0.0, 0.0, -1.0]),
            (1, [1.0, 0.0, 0.0]),
            (2, [0.0, 0.0, 1.0]),
            (3, [-1.0, 0.0, 0.0]),
        ] {
            let head = [10.5, 64.5, -9.5];
            let foot = std::array::from_fn(|axis| head[axis] + offset[axis]);
            let mut sampled = Vec::new();
            let result = light(
                BedModel {
                    color: "red",
                    head: true,
                    direction,
                },
                [10, 64, -10],
                |position| {
                    sampled.push(position);
                    if position == head { (2, 15) } else { (12, 4) }
                },
            );
            assert_eq!(sampled, [head, foot]);
            assert_eq!(result, BlockEntityLight::Actor { block: 12, sky: 15 });
        }
    }
}
