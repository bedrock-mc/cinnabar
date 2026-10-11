use super::state::State;
use assets::{ModelFamily, RegistryRecord, block_shapes::ShapeFlags};

/// Compiles the distinct property and top-face support facts used by neighbour queries.
pub(super) fn flags(record: &RegistryRecord) -> ShapeFlags {
    let name = record.name.strip_prefix("minecraft:").unwrap_or("");
    let property = match name {
        "jigsaw" | "structure_block" | "invisible_bedrock" | "air" | "scaffolding" | "cauldron"
        | "composter" | "cactus" | "hopper" | "lectern" => false,
        "barrier" | "farmland" | "grass_path" | "mud" | "soul_sand" | "honey_block"
        | "sculk_sensor" => true,
        _ if name.ends_with("shulker_box") => false,
        _ if super::state::is_double_slab(record) => true,
        _ => matches!(
            record.model_family,
            ModelFamily::Cube | ModelFamily::Leaves | ModelFamily::ChiseledBookshelf
        ),
    };
    let mut flags = ShapeFlags::empty();
    flags.set(ShapeFlags::VINE_SUPPORT, property);
    flags.set(
        ShapeFlags::FULL_TOP_SUPPORT,
        record.flags.contains(assets::BlockFlags::FIRE_TOP_SUPPORT)
            || scaffold_support(record, property),
    );
    flags
}

/// Evaluates the full-top query used by scaffolding, including class and support components.
fn scaffold_support(record: &RegistryRecord, property: bool) -> bool {
    let name = record.name.strip_prefix("minecraft:").unwrap_or("");
    match name {
        "hopper" | "cauldron" | "composter" | "scaffolding" | "piston" | "sticky_piston" => true,
        "piston_arm_collision" | "sticky_piston_arm_collision" => {
            super::piston::facing(record) == Some(1)
        }
        "farmland"
        | "grass_path"
        | "cactus"
        | "daylight_detector"
        | "daylight_detector_inverted"
        | "enchanting_table"
        | "end_portal_frame"
        | "lectern"
        | "powder_snow"
        | "sculk_sensor"
        | "calibrated_sculk_sensor"
        | "sculk_shrieker"
        | "stonecutter_block"
        | "sea_pickle"
        | "campfire"
        | "soul_campfire"
        | "chorus_plant"
        | "chorus_flower"
        | "brewing_stand" => false,
        "snow_layer" => {
            State::read(record).and_then(|state| state.number("height"))
                == Some(i64::from(assets::TOP_SNOW_LAYER_COUNT) - 1)
        }
        _ if name.ends_with("shulker_box") => false,
        _ if name.ends_with("_trapdoor") || name == "trapdoor" => {
            State::read(record).is_some_and(|state| {
                state.bit("open_bit") == Some(false) && state.bit("upside_down_bit") == Some(true)
            })
        }
        _ => match record.model_family {
            ModelFamily::Leaves | ModelFamily::Chest | ModelFamily::Wall | ModelFamily::Pane => {
                false
            }
            ModelFamily::Slab => super::state::is_double_slab(record),
            ModelFamily::Stair => {
                State::read(record).and_then(|state| state.bit("upside_down_bit")) == Some(true)
            }
            _ => property,
        },
    }
}

/// Selects the neighbour rule and its compiled alternate geometry.
pub(super) fn selection(
    record: &RegistryRecord,
) -> (
    assets::block_shapes::SelectionContext,
    Box<[assets::block_shapes::ShapeBox]>,
) {
    use assets::block_shapes::{SelectionContext, ShapeBox};
    match record.name.as_ref() {
        "minecraft:vine"
            if State::read(record).and_then(|state| state.number("vine_direction_bits"))
                == Some(0) =>
        {
            (
                SelectionContext::VineSupport,
                Box::new([
                    ShapeBox::new([0.0, 0.9375, 0.0], [1.0; 3]),
                    ShapeBox::new([0.0; 3], [1.0, 0.0625, 1.0]),
                    super::UNIT,
                ]),
            )
        }
        "minecraft:scaffolding" => {
            let mut boxes = super::scaffolding::selection(record).unwrap().into_vec();
            boxes.extend([
                ShapeBox::new([0.125, 0.0, 0.0], [0.875, 0.125, 0.125]),
                ShapeBox::new([0.125, 0.0, 0.875], [0.875, 0.125, 1.0]),
                ShapeBox::new([0.0, 0.0, 0.125], [0.125, 0.125, 0.875]),
                ShapeBox::new([0.875, 0.0, 0.125], [1.0, 0.125, 0.875]),
            ]);
            (SelectionContext::ScaffoldSupport, boxes.into_boxed_slice())
        }
        "minecraft:portal"
            if State::read(record)
                .and_then(|state| state.text("portal_axis").map(|axis| axis == "unknown"))
                == Some(true) =>
        {
            (
                SelectionContext::PortalAxis,
                Box::new([ShapeBox::new([0.0, 0.0, 0.375], [1.0, 1.0, 0.625])]),
            )
        }
        _ => (SelectionContext::Static, Box::new([])),
    }
}
