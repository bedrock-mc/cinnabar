use super::{selection::bounds, state::State};
use assets::{RegistryRecord, block_shapes::ShapeBox};

/// Compiles visual plant boxes; their movement geometry remains independent.
pub(super) fn shape(record: &RegistryRecord) -> Option<ShapeBox> {
    let name = record.name.strip_prefix("minecraft:")?;
    let shape = match name {
        "powder_snow"
        | "structure_void"
        | "firefly_bush"
        | "cave_vines"
        | "cave_vines_body_with_berries"
        | "cave_vines_head_with_berries"
        | "twisting_vines"
        | "weeping_vines"
        | "pale_hanging_moss" => super::UNIT,
        "frog_spawn" => bounds([0.0; 3], [1.0, 0.015625, 1.0]),
        "pink_petals" | "wildflowers" => bounds([0.0; 3], [1.0, 0.1875, 1.0]),
        "leaf_litter" => bounds([0.0; 3], [1.0, 0.0625, 1.0]),
        "sea_pickle" => bounds([0.0; 3], [1.0, 0.5, 1.0]),
        "crimson_fungus" | "warped_fungus" => bounds(
            [0.5_f32 - 0.23, 0.0, 0.27],
            [
                0.73,
                if name == "crimson_fungus" { 0.75 } else { 0.58 },
                0.73,
            ],
        ),
        "kelp" => bounds([0.125, 0.0, 0.125], [0.875, 1.0, 0.875]),
        "seagrass" | "bamboo_sapling" => bounds([0.1, 0.0, 0.1], [0.9, 0.8, 0.9]),
        "small_dripleaf_block" | "open_eyeblossom" | "closed_eyeblossom" => {
            bounds([0.15, 0.0, 0.15], [0.7, 0.6, 0.7])
        }
        "golden_dandelion" | "crimson_roots" | "warped_roots" => {
            bounds([0.15, 0.0, 0.15], [0.7, 0.6, 0.7])
        }
        "pitcher_plant" => bounds(
            [0.3, 0.0, 0.3],
            [
                0.7,
                if State::read(record)?.bit("upper_block_bit")? {
                    0.6
                } else {
                    1.0
                },
                0.7,
            ],
        ),
        "hanging_roots" => bounds([0.15, 0.7, 0.15], [0.85, 1.0, 0.85]),
        "spore_blossom" => bounds([0.15, 0.75, 0.15], [0.85, 1.0, 0.85]),
        "sweet_berry_bush" => {
            if State::read(record)?.number("growth")? == 0 {
                bounds([0.5_f32 - 0.2, 0.0, 0.5_f32 - 0.2], [0.7, 0.8, 0.7])
            } else {
                bounds([0.5_f32 - 0.4, 0.0, 0.5_f32 - 0.4], [0.9, 1.0, 0.9])
            }
        }
        "mangrove_propagule" => {
            let state = State::read(record)?;
            if state.bit("hanging")? {
                let growth = state.number("propagule_stage")?.clamp(0, 4);
                let length =
                    (3 * (growth + 1) + i64::from(growth == 0 || growth >= 3)) as f32 / 16.0;
                bounds([0.375, 1.0 - length, 0.375], [0.625, 1.0, 0.625])
            } else {
                bounds([0.375, 0.0, 0.375], [0.625, 1.0, 0.625])
            }
        }
        "pumpkin_stem" | "melon_stem" => stem(record)?,
        "pitcher_crop" => {
            let state = State::read(record)?;
            let growth = state.number("growth")?;
            if state.bit("upper_block_bit")? {
                bounds(
                    [0.1875, 0.0, 0.1875],
                    [0.8125, if growth == 3 { 0.6875 } else { 0.9375 }, 0.8125],
                )
            } else if growth == 0 {
                bounds([0.3125, -0.0625, 0.3125], [0.6875, 0.1875, 0.6875])
            } else {
                bounds(
                    [0.1875, -0.0625, 0.1875],
                    [0.8125, if growth == 1 { 0.875 } else { 1.0 }, 0.8125],
                )
            }
        }
        _ if name.ends_with("_coral") => bounds([0.1, 0.0, 0.1], [0.9, 0.8, 0.9]),
        _ if name.ends_with("_coral_fan") => {
            bounds([0.5_f32 - 0.3, 0.0, 0.5_f32 - 0.3], [0.8, 0.4, 0.8])
        }
        _ if name.ends_with("_coral_wall_fan") => {
            bounds([0.5_f32 - 0.3, 0.3, 0.5_f32 - 0.3], [0.8, 0.7, 0.8])
        }
        _ => return None,
    };
    Some(shape)
}

/// Computes a stem's growth height and attachment extension.
fn stem(record: &RegistryRecord) -> Option<ShapeBox> {
    let state = State::read(record)?;
    let facing = state.number("facing_direction")?;
    let height = if facing == 0 {
        (2 * state.number("growth")? + 2) as f32 / 16.0
    } else {
        0.25
    };
    let mut shape = bounds([0.375, 0.0, 0.375], [0.625, height, 0.625]);
    match facing {
        2 => shape.min[2] = 0.0,
        3 => shape.max[2] = 1.0,
        4 => shape.min[0] = 0.0,
        5 => shape.max[0] = 1.0,
        _ => {}
    }
    Some(shape)
}
