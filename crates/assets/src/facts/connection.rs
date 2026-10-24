//! Pane and fence links that the neighbouring block grants on its own.
//!
//! Native connections accept any neighbour without a connection rule. Unit cubes carry
//! none, so panes and fences join them whatever their own family. Rendering and placement
//! collision both read this one classification so their shapes cannot diverge.

use std::sync::OnceLock;

use crate::{BlockFlags, ContributorRole, NetworkIdMode, RegistryRecord, facts::block_render_fact};

/// Whether panes and fences link to `record` as a unit cube: an opaque or leaf cube, glass,
/// or one of the see-through cubes the registry leaves without a cube model family
/// (ice, slime, honey, tinted glass, powder snow).
#[must_use]
pub fn links_as_unit_cube(record: &RegistryRecord) -> bool {
    record.contributor_role == ContributorRole::Primary
        && !record.flags.contains(BlockFlags::AIR)
        && (record
            .flags
            .intersects(BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL)
            || block_render_fact(&record.name).kind.is_translucent_cube())
}

/// [`links_as_unit_cube`] for a built-in block state. Custom and unknown states are `false`.
#[must_use]
pub fn pinned_links_as_unit_cube(mode: NetworkIdMode, runtime_id: u32) -> bool {
    static LINKS: OnceLock<Box<[bool]>> = OnceLock::new();
    let links = LINKS.get_or_init(|| {
        let records = crate::read_registry_for_protocol(
            crate::pinned_block_registry_bytes(),
            crate::active_content_registry_protocol(),
        )
        .expect("validated pinned block registry");
        let mut links = vec![false; records.len()];
        for record in &records {
            if let Some(link) = links.get_mut(record.sequential_id as usize) {
                *link = links_as_unit_cube(record);
            }
        }
        links.into_boxed_slice()
    });
    let id = match mode {
        NetworkIdMode::Sequential => Some(runtime_id),
        NetworkIdMode::Hashed => crate::pinned_block_sequential_id(runtime_id),
    };
    id.and_then(|id| links.get(id as usize).copied())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pinned(name: &str) -> RegistryRecord {
        crate::read_registry_for_protocol(
            crate::pinned_block_registry_bytes(),
            crate::active_content_registry_protocol(),
        )
        .unwrap()
        .into_iter()
        .find(|record| record.name.as_ref() == name)
        .unwrap_or_else(|| panic!("pinned registry has {name}"))
    }

    #[test]
    fn shipped_cubes_link_and_other_shapes_do_not() {
        for (name, links) in [
            ("minecraft:glass", true),
            ("minecraft:tinted_glass", true),
            ("minecraft:ice", true),
            ("minecraft:frosted_ice", true),
            ("minecraft:honey_block", true),
            ("minecraft:slime", true),
            ("minecraft:oak_leaves", true),
            ("minecraft:stone", true),
            ("minecraft:air", false),
            ("minecraft:poppy", false),
            ("minecraft:torch", false),
            ("minecraft:glass_pane", false),
            ("minecraft:oak_stairs", false),
        ] {
            let record = pinned(name);
            assert_eq!(links_as_unit_cube(&record), links, "{name}");
            for (mode, id) in [
                (NetworkIdMode::Sequential, record.sequential_id),
                (NetworkIdMode::Hashed, record.network_hash),
            ] {
                assert_eq!(
                    pinned_links_as_unit_cube(mode, id),
                    links,
                    "{name} {mode:?}"
                );
            }
        }
    }
}
