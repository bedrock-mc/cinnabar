use super::state::State;
use assets::facts::BlockProperty;
use assets::{
    RegistryRecord,
    block_shapes::{SelectionContext, ShapeBox},
};
use assets::{facts::shape::ShapeTraits, registry::BlockTraits};

/// Compiles the three arm pieces before the owning piston supplies its progress.
pub(super) fn selection(record: &RegistryRecord, shapes: ShapeTraits) -> Option<Box<[ShapeBox]>> {
    if context(shapes)? != SelectionContext::PistonArm {
        return Some(Box::new([]));
    }
    let facing = usize::from(facing(record)?);
    let base = [
        ([0.3125, 0.75, 0.3125], [0.6875, 1.25, 0.6875]),
        ([0.3125, -0.25, 0.3125], [0.6875, 0.25, 0.6875]),
        ([0.3125, 0.3125, -0.25], [0.6875, 0.6875, 0.25]),
        ([0.3125, 0.3125, 0.75], [0.6875, 0.6875, 1.25]),
        ([-0.25, 0.3125, 0.3125], [0.25, 0.6875, 0.6875]),
        ([0.75, 0.3125, 0.3125], [1.25, 0.6875, 0.6875]),
    ][facing];
    let arm = [
        ([0.375, 0.25, 0.375], [0.625, 0.75, 0.625]),
        ([0.375, 0.375, 0.25], [0.625, 0.625, 0.75]),
        ([0.25, 0.375, 0.375], [0.75, 0.625, 0.625]),
    ][facing / 2];
    let front = [
        ([0.0; 3], [1.0, 0.25, 1.0]),
        ([0.0, 0.75, 0.0], [1.0; 3]),
        ([0.0, 0.0, 0.75], [1.0; 3]),
        ([0.0; 3], [1.0, 1.0, 0.25]),
        ([0.75, 0.0, 0.0], [1.0; 3]),
        ([0.0; 3], [0.25, 1.0, 1.0]),
    ][facing];
    Some(Box::new(
        [base, arm, front].map(|(min, max)| ShapeBox::new(min, max)),
    ))
}

/// Identifies the classes whose clipping depends on block-actor data.
pub(super) fn context(shapes: ShapeTraits) -> Option<SelectionContext> {
    if shapes.contains(ShapeTraits::PISTON_ARM) {
        Some(SelectionContext::PistonArm)
    } else if shapes.contains(ShapeTraits::MOVING_BLOCK) {
        Some(SelectionContext::MovingBlock)
    } else {
        None
    }
}

/// Compiles the native piston orientation for arms and their owning piston blocks.
pub(super) fn facing(record: &RegistryRecord) -> Option<u8> {
    if !record.traits.contains(BlockTraits::PISTON) {
        return None;
    }
    State::read(record)?
        .number(BlockProperty::FacingDirection)
        .and_then(|face| u8::try_from(face).ok())
        .filter(|face| *face < 6)
}

/// Supplies the state-only visual box, independently of specialized world clipping.
pub(super) fn visual(
    record: &RegistryRecord,
    selection: &[ShapeBox],
    shapes: ShapeTraits,
) -> Option<ShapeBox> {
    use assets::ModelFamily;
    if shapes.contains(ShapeTraits::SHELF) {
        return Some(ShapeBox::new([0.0; 3], [0.3125, 1.0, 1.0]));
    }
    if shapes.contains(ShapeTraits::MOVING_BLOCK) {
        return None;
    }
    if record.traits.contains(BlockTraits::AIR)
        || (selection.is_empty()
            && (record.traits.contains(BlockTraits::LADDER)
                || shapes.intersects(ShapeTraits::WALL_BANNER | ShapeTraits::PALE_MOSS)))
    {
        return None;
    }
    if matches!(
        record.model_family,
        ModelFamily::Stair
            | ModelFamily::Fence
            | ModelFamily::Wall
            | ModelFamily::Pane
            | ModelFamily::Door
            | ModelFamily::Gate
    ) || (record.model_family == ModelFamily::Vine
        || record.traits.intersects(
            BlockTraits::SCAFFOLDING
                | BlockTraits::MULTIFACE
                | BlockTraits::HOPPER
                | BlockTraits::CAULDRON
                | BlockTraits::COMPOSTER,
        )
        || (record.traits.contains(BlockTraits::PORTAL)
            && !record.traits.contains(BlockTraits::END_PORTAL))
        || shapes.intersects(ShapeTraits::CHORUS_PLANT | ShapeTraits::PISTON_ARM))
    {
        return Some(super::UNIT);
    }
    selection.first().copied().or(Some(super::UNIT))
}
