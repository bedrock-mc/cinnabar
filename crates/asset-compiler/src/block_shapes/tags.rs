use assets::{RegistryRecord, registry::BlockTraits};

/// Reads the compiled one-way collision membership.
pub(super) fn native_block_tags(record: &RegistryRecord) -> Box<[Box<str>]> {
    if record.traits.contains(BlockTraits::ONE_WAY_COLLIDABLE) {
        Box::new(["one_way_collidable".into()])
    } else {
        Box::default()
    }
}

/// Reads the compiled cube placement intention.
pub(super) fn has_build_intention(record: &RegistryRecord) -> bool {
    record.traits.contains(BlockTraits::BUILD_INTENTION)
}
