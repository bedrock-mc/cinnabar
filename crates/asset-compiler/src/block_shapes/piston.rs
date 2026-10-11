use super::state::State;
use assets::{
    RegistryRecord,
    block_shapes::{SelectionContext, ShapeBox},
};

/// Compiles the three arm pieces before the owning piston supplies its progress.
pub(super) fn selection(record: &RegistryRecord) -> Option<Box<[ShapeBox]>> {
    if context(record)? != SelectionContext::PistonArm {
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
pub(super) fn context(record: &RegistryRecord) -> Option<SelectionContext> {
    match record.name.as_ref() {
        "minecraft:piston_arm_collision" | "minecraft:sticky_piston_arm_collision" => {
            Some(SelectionContext::PistonArm)
        }
        "minecraft:moving_block" => Some(SelectionContext::MovingBlock),
        _ => None,
    }
}

/// Compiles the native piston orientation for arms and their owning piston blocks.
pub(super) fn facing(record: &RegistryRecord) -> Option<u8> {
    if !matches!(
        record.name.as_ref(),
        "minecraft:piston"
            | "minecraft:sticky_piston"
            | "minecraft:piston_arm_collision"
            | "minecraft:sticky_piston_arm_collision"
    ) {
        return None;
    }
    State::read(record)?
        .number("facing_direction")
        .and_then(|face| u8::try_from(face).ok())
        .filter(|face| *face < 6)
}

/// Supplies the state-only visual box, independently of specialized world clipping.
pub(super) fn visual(record: &RegistryRecord, selection: &[ShapeBox]) -> Option<ShapeBox> {
    use assets::ModelFamily;
    if record.name.ends_with("_shelf") {
        return Some(ShapeBox::new([0.0; 3], [0.3125, 1.0, 1.0]));
    }
    if record.name.as_ref() == "minecraft:moving_block" {
        return None;
    }
    if record.name.as_ref() == "minecraft:air"
        || (selection.is_empty()
            && matches!(
                record.name.as_ref(),
                "minecraft:ladder" | "minecraft:wall_banner" | "minecraft:pale_moss_carpet"
            ))
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
    ) || matches!(
        record.name.as_ref(),
        "minecraft:vine"
            | "minecraft:scaffolding"
            | "minecraft:chorus_plant"
            | "minecraft:portal"
            | "minecraft:glow_lichen"
            | "minecraft:sculk_vein"
            | "minecraft:resin_clump"
            | "minecraft:hopper"
            | "minecraft:cauldron"
            | "minecraft:composter"
            | "minecraft:piston_arm_collision"
            | "minecraft:sticky_piston_arm_collision"
    ) {
        return Some(super::UNIT);
    }
    selection.first().copied().or(Some(super::UNIT))
}
