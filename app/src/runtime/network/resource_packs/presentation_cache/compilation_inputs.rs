//! Compares inputs by the mapping rules their presentation compilers consume.

use protocol::{CustomBlock, CustomBlockVisuals, CustomMaterialInstance, CustomVisualComponents};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub(super) fn inputs_mismatch(
    left: &client_session::PackInputs,
    right: &client_session::PackInputs,
) -> Option<&'static str> {
    if left.hashed != right.hashed {
        return Some("hashed_ids");
    }
    if left.blocks.skipped != right.blocks.skipped
        || (left.blocks.blocks != right.blocks.blocks
            && (left.blocks.blocks.len() != right.blocks.blocks.len()
                || !left
                    .blocks
                    .blocks
                    .iter()
                    .zip(right.blocks.blocks.iter())
                    .all(|(left, right)| same_block(left, right))))
    {
        return Some("custom_blocks");
    }
    if left.blocks.vanilla_blocks != right.blocks.vanilla_blocks
        && left.blocks.vanilla_blocks.iter().collect::<BTreeSet<_>>()
            != right.blocks.vanilla_blocks.iter().collect::<BTreeSet<_>>()
    {
        return Some("vanilla_definitions");
    }
    if left.icons != right.icons && icon_map(&left.icons) != icon_map(&right.icons) {
        return Some("icon_keys");
    }
    (left.block_items != right.block_items).then_some("block_items")
}

fn icon_map(pairs: &[(Arc<str>, Arc<str>)]) -> BTreeMap<&str, &str> {
    // Icon declarations use the last entry for each identifier.
    pairs
        .iter()
        .map(|(identifier, key)| (identifier.as_ref(), key.as_ref()))
        .collect()
}

fn same_block(left: &CustomBlock, right: &CustomBlock) -> bool {
    left.name == right.name
        && left.state_count == right.state_count
        && left.collides == right.collides
        && left.collision_box == right.collision_box
        && left.selection == right.selection
        && same_visuals(&left.visual, &right.visual)
}

fn same_visuals(left: &CustomBlockVisuals, right: &CustomBlockVisuals) -> bool {
    left.state_axes == right.state_axes
        && same_components(&left.base, &right.base)
        && left.permutations.len() == right.permutations.len()
        && left
            .permutations
            .iter()
            .zip(right.permutations.iter())
            .all(|(left, right)| {
                left.condition == right.condition
                    && same_components(&left.components, &right.components)
            })
}

fn same_components(left: &CustomVisualComponents, right: &CustomVisualComponents) -> bool {
    left.geometry == right.geometry
        && left.transformation == right.transformation
        && left.light_dampening == right.light_dampening
        && left.light_emission == right.light_emission
        && (left.materials == right.materials
            || material_map(left.materials.as_deref()) == material_map(right.materials.as_deref()))
}

fn material_map(
    materials: Option<&[CustomMaterialInstance]>,
) -> Option<BTreeMap<&str, &CustomMaterialInstance>> {
    materials.map(|materials| {
        let mut by_name = BTreeMap::new();
        for material in materials {
            // Face lookup resolves the first instance of a given name.
            by_name.entry(material.name.as_ref()).or_insert(material);
        }
        by_name
    })
}
