use assets::{ModelFamily, RegistryRecord};

/// Door, gate and trapdoor types share the native one-way collision tag.
pub(super) fn native_block_tags(record: &RegistryRecord) -> Box<[Box<str>]> {
    match record.model_family {
        ModelFamily::Door | ModelFamily::Gate | ModelFamily::Trapdoor => {
            Box::new(["one_way_collidable".into()])
        }
        _ => Box::default(),
    }
}

/// Admitted families and these specialized blocks retain their cube placement intention.
pub(super) fn has_build_intention(record: &RegistryRecord) -> bool {
    matches!(
        record.model_family,
        ModelFamily::Cube
            | ModelFamily::Leaves
            | ModelFamily::Stair
            | ModelFamily::Slab
            | ModelFamily::Pane
            | ModelFamily::Fence
            | ModelFamily::Wall
            | ModelFamily::Carpet
    ) || matches!(
        record.name.as_ref(),
        "minecraft:soul_sand"
            | "minecraft:mud"
            | "minecraft:barrier"
            | "minecraft:chiseled_bookshelf"
    )
}
