//! Compiles movement and interaction facts once per block state.

use assets::block_shapes::{CompiledBlockShapes, PickRule, ShapeBox, ShapeFlags};
use assets::facts::BlockProperty;
use assets::{BlockPhysicsRecord, CollisionBox, ModelFamily, RegistryRecord};
use assets::{
    facts::{block::RESERVED_BLOCK_NAME, shape::ShapeTraits},
    registry::BlockTraits,
};

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

/// Compiles the final physics carrier from a complete movement seed and its bound identities.
pub fn compile_registry(
    protocol: u32,
    breg: &[u8],
    records: &[RegistryRecord],
    input: &assets::PhysicsRegistry,
) -> Result<Vec<u8>, assets::AssetError> {
    let compiled: Vec<_> = records
        .iter()
        .map(|record| {
            compile(
                record,
                input
                    .by_sequential_id(record.sequential_id)
                    .expect("complete seed registry"),
            )
        })
        .collect();
    assets::encode_physics_registry(protocol, breg, records, &compiled)
}

/// Compiles final geometry while retaining the input's independent movement scalars.
pub fn compile(record: &RegistryRecord, input: &BlockPhysicsRecord) -> BlockPhysicsRecord {
    let (traits, shapes) = crate::registry_policy::block_shape_facts(record);
    let mut effective = std::borrow::Cow::Borrowed(record);
    if traits != record.traits {
        effective.to_mut().traits = traits;
    }
    let record = effective.as_ref();
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
    flags.set(ShapeFlags::AIR, traits.contains(BlockTraits::AIR));
    flags.set(
        ShapeFlags::GRASS_OFFSET,
        assets::facts::block_render_fact(&record.name).kind
            == assets::facts::BlockRenderKind::GrassCross,
    );
    flags.set(
        ShapeFlags::HIDE_OVERLAY,
        shapes.contains(ShapeTraits::END_GATEWAY),
    );
    flags.set(
        ShapeFlags::CREATIVE_OVERLAY,
        shapes.contains(ShapeTraits::BARRIER),
    );
    flags.set(
        ShapeFlags::LIGHT_BLOCK,
        traits.contains(BlockTraits::LIGHT_BLOCK),
    );
    flags.set(
        ShapeFlags::DRIPSTONE_OFFSET,
        traits.contains(BlockTraits::DRIPSTONE_OFFSET),
    );
    flags.set(
        ShapeFlags::PORTAL,
        traits.contains(BlockTraits::PORTAL) && !traits.contains(BlockTraits::END_PORTAL),
    );
    flags.set(
        ShapeFlags::CHORUS_CONNECT,
        traits.contains(BlockTraits::CHORUS_CONNECT),
    );
    flags.set(
        ShapeFlags::END_STONE,
        shapes.contains(ShapeTraits::END_STONE),
    );
    flags |= support::flags(record);
    let mut pick_rule = pick_rule(record, shapes);
    let selection = (pick_rule == PickRule::Never)
        .then(|| Box::new([]) as Box<[ShapeBox]>)
        .or_else(|| hopper::selection(record))
        .or_else(|| vessels::selection(record))
        .or_else(|| stairs::selection(record))
        .or_else(|| piston::selection(record, shapes))
        .or_else(|| scaffolding::selection(record))
        .or_else(|| faces::multiface(record))
        .or_else(|| faces::vine(record))
        .or_else(|| faces::pale_moss(record, shapes))
        .unwrap_or_else(|| {
            outlines::gate_shape(record)
                .or_else(|| visual::shape(record, shapes))
                .or_else(|| furnishings::shape(record, shapes))
                .or_else(|| attachments::shape(record))
                .or_else(|| redstone::shape(record, shapes))
                .or_else(|| plants::shape(record, shapes))
                .or_else(|| selection::shape(record, shapes))
                .map_or_else(
                    || Box::new([UNIT]) as Box<[ShapeBox]>,
                    |shape| Box::new([shape]) as Box<[ShapeBox]>,
                )
        });
    if selection.is_empty()
        && (traits.contains(BlockTraits::MULTIFACE) || shapes.contains(ShapeTraits::PALE_MOSS))
    {
        pick_rule = PickRule::Never;
    }
    let (outline, connected_outline, outline_context) = outlines::compile(record, shapes);
    let (mut selection_context, contextual_selection) = support::selection(record);
    if let Some(context) = piston::context(shapes) {
        selection_context = context;
    }
    output.shapes = Some(CompiledBlockShapes {
        state_identity: state::State::identity(record),
        actor_facing: piston::facing(record),
        actor_visual: piston::visual(record, &selection, shapes),
        selection,
        selection_context,
        contextual_selection,
        outline,
        connected_outline,
        outline_context,
        pick_rule,
        flags,
        door: doors::state(record),
        flow: flow::facts(record, shapes),
        tags: tags::native_block_tags(record),
    });
    output
}

/// Classifies native eligibility separately from visual or movement geometry.
fn pick_rule(record: &RegistryRecord, shapes: ShapeTraits) -> PickRule {
    let traits = record.traits;
    if traits.contains(BlockTraits::LIGHT_BLOCK) {
        return PickRule::HeldLight;
    }
    if (traits.contains(BlockTraits::LADDER) || shapes.contains(ShapeTraits::WALL_BANNER))
        && state::State::read(record)
            .and_then(|state| state.number(BlockProperty::FacingDirection))
            .is_some_and(|facing| facing < 2)
    {
        return PickRule::Never;
    }
    if traits.intersects(BlockTraits::AIR | BlockTraits::FIRE)
        || shapes.contains(ShapeTraits::NEVER_PICK)
        || record.name.as_ref() == RESERVED_BLOCK_NAME
    {
        PickRule::Never
    } else if shapes.contains(ShapeTraits::BUBBLE_COLUMN) {
        PickRule::Liquid
    } else if traits.intersects(BlockTraits::WATER | BlockTraits::LAVA) {
        PickRule::LiquidSource
    } else if traits.contains(BlockTraits::PORTAL) && !traits.contains(BlockTraits::END_PORTAL) {
        PickRule::Creative
    } else {
        PickRule::Always
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
