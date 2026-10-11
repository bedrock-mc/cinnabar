//! Compiles movement and interaction facts once per block state.

use assets::block_shapes::{CompiledBlockShapes, PickRule, ShapeBox, ShapeFlags};
use assets::{BlockPhysicsRecord, CollisionBox, ModelFamily, RegistryRecord};

mod attachments;
mod connected;
mod doors;
mod faces;
mod flow;
mod furnishings;
mod hopper;
mod outlines;
mod piston;
mod plants;
mod redstone;
mod scaffolding;
mod selection;
mod stairs;
mod state;
mod support;
mod tags;
mod vessels;
mod visual;

#[cfg(test)]
mod tests;

const UNIT: ShapeBox = ShapeBox::new([0.0; 3], [1.0; 3]);

/// Compiles final geometry while retaining the input's independent movement scalars.
pub fn compile(record: &RegistryRecord, input: &BlockPhysicsRecord) -> BlockPhysicsRecord {
    let mut output = input.clone();
    let collision = connected::shapes(record)
        .or_else(|| stairs::shapes(record))
        .or_else(|| scaffolding::shapes(record));
    if let Some(collision) = collision {
        output.boxes = collision.into_iter().map(collision_box).collect();
    }
    let mut flags = ShapeFlags::empty();
    flags.set(
        ShapeFlags::FULL_CUBE,
        record.model_family == ModelFamily::Cube && input.boxes.as_ref() == [collision_box(UNIT)],
    );
    flags.set(
        ShapeFlags::BUILD_INTENTION,
        tags::has_build_intention(record),
    );
    flags.set(
        ShapeFlags::BAMBOO_OFFSET,
        assets::bamboo::BambooState::from_record(record).is_some(),
    );
    flags.set(ShapeFlags::AIR, record.name.as_ref() == "minecraft:air");
    flags.set(
        ShapeFlags::CUSTOM_SURFACE,
        record.name.as_ref() == "minecraft:stone",
    );
    flags.set(
        ShapeFlags::HIDE_OVERLAY,
        record.name.as_ref() == "minecraft:end_gateway",
    );
    flags.set(
        ShapeFlags::CREATIVE_OVERLAY,
        record.name.as_ref() == "minecraft:barrier",
    );
    flags.set(ShapeFlags::LIGHT_BLOCK, is_light(record));
    flags.set(
        ShapeFlags::DRIPSTONE_OFFSET,
        matches!(
            record.name.as_ref(),
            "minecraft:pointed_dripstone" | "minecraft:sulfur_spike"
        ),
    );
    flags.set(
        ShapeFlags::PORTAL,
        record.name.as_ref() == "minecraft:portal",
    );
    flags.set(
        ShapeFlags::CHORUS_CONNECT,
        matches!(
            record.name.as_ref(),
            "minecraft:chorus_plant" | "minecraft:chorus_flower"
        ),
    );
    flags.set(
        ShapeFlags::END_STONE,
        record.name.as_ref() == "minecraft:end_stone",
    );
    flags |= support::flags(record);
    let mut pick_rule = pick_rule(record);
    let selection = (pick_rule == PickRule::Never)
        .then(|| Box::new([]) as Box<[ShapeBox]>)
        .or_else(|| hopper::selection(record))
        .or_else(|| vessels::selection(record))
        .or_else(|| stairs::selection(record))
        .or_else(|| piston::selection(record))
        .or_else(|| scaffolding::selection(record))
        .or_else(|| faces::multiface(record))
        .or_else(|| faces::vine(record))
        .or_else(|| faces::pale_moss(record))
        .unwrap_or_else(|| {
            visual::shape(record)
                .or_else(|| furnishings::shape(record))
                .or_else(|| attachments::shape(record))
                .or_else(|| redstone::shape(record))
                .or_else(|| plants::shape(record))
                .or_else(|| selection::shape(record))
                .map_or_else(
                    || Box::new([UNIT]) as Box<[ShapeBox]>,
                    |shape| Box::new([shape]) as Box<[ShapeBox]>,
                )
        });
    if selection.is_empty()
        && matches!(
            record.name.as_ref(),
            "minecraft:glow_lichen"
                | "minecraft:sculk_vein"
                | "minecraft:resin_clump"
                | "minecraft:pale_moss_carpet"
        )
    {
        pick_rule = PickRule::Never;
    }
    let (outline, connected_outline, outline_context) = outlines::compile(record);
    let (mut selection_context, contextual_selection) = support::selection(record);
    if let Some(context) = piston::context(record) {
        selection_context = context;
    }
    output.shapes = Some(CompiledBlockShapes {
        state_identity: state::State::identity(record),
        actor_facing: piston::facing(record),
        actor_visual: piston::visual(record, &selection),
        selection,
        selection_context,
        contextual_selection,
        outline,
        connected_outline,
        outline_context,
        pick_rule,
        flags,
        door: doors::state(record),
        flow: flow::facts(record),
        tags: tags::native_block_tags(record),
    });
    output
}

/// Classifies native eligibility separately from visual or movement geometry.
fn pick_rule(record: &RegistryRecord) -> PickRule {
    if is_light(record) {
        return PickRule::HeldLight;
    }
    match record.name.as_ref() {
        "minecraft:ladder" | "minecraft:wall_banner"
            if state::State::read(record)
                .and_then(|state| state.number("facing_direction"))
                .is_some_and(|facing| facing < 2) =>
        {
            PickRule::Never
        }
        "minecraft:air"
        | "minecraft:fire"
        | "minecraft:soul_fire"
        | "minecraft:invisible_bedrock"
        | "cinnabar:reserved" => PickRule::Never,
        "minecraft:water"
        | "minecraft:flowing_water"
        | "minecraft:lava"
        | "minecraft:flowing_lava" => PickRule::LiquidSource,
        "minecraft:bubble_column" => PickRule::Liquid,
        "minecraft:portal" => PickRule::Creative,
        _ => PickRule::Always,
    }
}

/// Quantizes state-determined movement boxes to the registry's coordinate grid.
fn collision_box(shape: ShapeBox) -> CollisionBox {
    let scale = f64::from(assets::block_shapes::COLLISION_UNITS_PER_BLOCK);
    let [min_x, min_y, min_z] = shape.min.map(|v| (v * scale).round() as i32);
    let [max_x, max_y, max_z] = shape.max.map(|v| (v * scale).round() as i32);
    CollisionBox {
        min_x,
        min_y,
        min_z,
        max_x,
        max_y,
        max_z,
    }
}

/// Recognizes the sixteen native light classes while compiling their interaction facts.
fn is_light(record: &RegistryRecord) -> bool {
    record
        .name
        .strip_prefix("minecraft:light_block_")
        .and_then(|level| level.parse::<u8>().ok())
        .is_some_and(|level| level <= 15)
}
