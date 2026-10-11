use super::{UNIT, selection::bounds, state::State};
use assets::{RegistryRecord, block_shapes::ShapeBox};

/// Compiles class visual shapes that differ from movement boxes or their quantization.
pub(super) fn shape(record: &RegistryRecord) -> Option<ShapeBox> {
    let name = record.name.strip_prefix("minecraft:")?;
    if record.model_family == assets::ModelFamily::Chest {
        return Some(bounds([0.025, 0.0, 0.025], [0.975, 0.95, 0.975]));
    }
    Some(match name {
        "border_block" | "mud" | "soul_sand" | "anvil" | "chipped_anvil" | "damaged_anvil"
        | "deprecated_anvil" | "cactus" | "brewing_stand" | "water" | "flowing_water" | "lava"
        | "flowing_lava" | "bubble_column" | "red_shrub" | "dragon_egg" | "chorus_plant" => UNIT,
        "honey_block" => bounds([0.0625, 0.0, 0.0625], [0.9375, 0.9375, 0.9375]),
        "farmland" | "grass_path" => bounds([0.0; 3], [1.0, 0.9375, 1.0]),
        "lectern" => bounds([0.0; 3], [1.0, 0.9, 1.0]),
        "conduit" => bounds([0.25, 0.0, 0.25], [0.75, 0.5, 0.75]),
        "end_portal_frame" => bounds([0.0; 3], [1.0, 0.8125, 1.0]),
        "portal" => match State::read(record)?.text("portal_axis")? {
            "x" => bounds([0.0, 0.0, 0.375], [1.0, 1.0, 0.625]),
            "z" | "unknown" => bounds([0.375, 0.0, 0.0], [0.625, 1.0, 1.0]),
            _ => return None,
        },
        "pointed_dripstone" | "sulfur_spike" => {
            let state = State::read(record)?;
            let (inset, bottom, top) = match state.text("dripstone_thickness")? {
                "tip" => (
                    0.3125_f32,
                    if state.bit("hanging")? { 0.3125 } else { 0.0 },
                    if state.bit("hanging")? { 1.0 } else { 0.6875 },
                ),
                "merge" => (0.3125, 0.0, 1.0),
                "frustum" => (0.25, 0.0, 1.0),
                "middle" => (0.1875, 0.0, 1.0),
                "base" => (0.125, 0.0, 1.0),
                _ => return None,
            };
            let [x, _, z] = block_transform::random_offset::POINTED_DRIPSTONE.offset([0; 3]);
            bounds(
                [inset + x, bottom, inset + z],
                [1.0 - inset + x, top, 1.0 - inset + z],
            )
        }
        "bamboo" => {
            let state = assets::bamboo::BambooState::from_record(record)?;
            let edge = if state.thick { 0.6875_f32 } else { 0.625_f32 };
            let [x, _, z] = block_transform::bamboo::ORIGIN_OFFSET;
            bounds([0.5 + x, 0.0, 0.5 + z], [edge + x, 1.0, edge + z])
        }
        "tall_dry_grass" => bounds([0.0625, 0.0, 0.0625], [0.9375, 1.0, 0.9375]),
        "wheat" | "carrots" | "potatoes" | "beetroot" | "nether_wart" | "torchflower_crop" => {
            bounds([0.0; 3], [1.0, 0.25, 1.0])
        }
        "chest" | "trapped_chest" | "ender_chest" => {
            bounds([0.025, 0.0, 0.025], [0.975, 0.95, 0.975])
        }
        "turtle_egg" => bounds(
            [0.5_f32 - 0.3, 0.0, f32::from_bits(0x3e4ccccc)],
            [0.8, 0.45, 0.8],
        ),
        "big_dripleaf" => {
            let state = State::read(record)?;
            if state.bit("big_dripleaf_head")? {
                bounds([0.0; 3], [1.0, 0.9375, 1.0])
            } else {
                let (min, max) = match state.direction()? {
                    0 => ([0.3125, 0.0, 0.125], [0.6875, 1.0, 0.5]),
                    1 => ([0.5, 0.0, 0.3125], [0.875, 1.0, 0.6875]),
                    2 => ([0.3125, 0.0, 0.5], [0.6875, 1.0, 0.875]),
                    3 => ([0.125, 0.0, 0.3125], [0.5, 1.0, 0.6875]),
                    _ => return None,
                };
                bounds(min, max)
            }
        }
        _ if name.starts_with("light_block_") => UNIT,
        _ if name.ends_with("_trapdoor") || name == "trapdoor" => {
            let state = State::read(record)?;
            let thickness = 0.1825_f32;
            if state.bit("open_bit")? {
                let (min, max) = match state.direction()? {
                    0 => ([0.0; 3], [thickness, 1.0, 1.0]),
                    1 => ([1.0 - thickness, 0.0, 0.0], [1.0; 3]),
                    2 => ([0.0; 3], [1.0, 1.0, thickness]),
                    3 => ([0.0, 0.0, 1.0 - thickness], [1.0; 3]),
                    _ => return None,
                };
                bounds(min, max)
            } else if state.bit("upside_down_bit")? {
                bounds([0.0, 1.0 - thickness, 0.0], [1.0; 3])
            } else {
                bounds([0.0; 3], [1.0, thickness, 1.0])
            }
        }
        _ if name.ends_with("_fence_gate") || name == "fence_gate" => UNIT,
        _ => return None,
    })
}
