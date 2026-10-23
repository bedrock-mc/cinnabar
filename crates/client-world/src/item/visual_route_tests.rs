use super::*;
use assets::{ItemTextureReference, ItemVisualAlias, ItemVisualDefinition};

#[test]
fn sprite_items_keep_their_visual_when_the_stack_carries_a_placed_block() {
    let sprite = ItemVisualRoute::Compiled(ItemVisualId(7));
    for block_runtime_id in [1, i32::MIN] {
        assert_eq!(classify_retained_block(sprite, block_runtime_id), sprite);
    }
    let block = ItemVisualRoute::BlockItem(BlockVisualId(3));
    assert_eq!(classify_retained_block(block, 1), block);
    assert_eq!(
        classify_retained_block(ItemVisualRoute::Missing, i32::MIN),
        ItemVisualRoute::RetainedBlock {
            block_runtime_id: i32::MIN,
        }
    );
}

const TERRACOTTA: &str = "minecraft:blue_terracotta";

/// Builds a small catalog, mapping alias metadata to dense visual indices.
/// Sprite routes share one synthetic raster source.
fn item_assets(
    identifier: &str,
    visuals: &[(u32, ItemVisualDefinitionRoute)],
    aliases: &[(u32, u32)],
) -> Arc<RuntimeEntityAssets> {
    let mut compiled = assets::carriers::startup::test_carriers::synthetic_entity_assets(1);
    let mut sources = compiled.sources.into_vec();
    let raster_source = sources.len() as u32;
    sources.push(assets::EntityAssetSource {
        path: "textures/items/fixture.png".into(),
        ..sources[0].clone()
    });
    compiled.sources = sources.into();
    compiled.block_visual_count = visuals
        .iter()
        .filter_map(|(_, route)| match route {
            ItemVisualDefinitionRoute::BlockItem { block_visual } => Some(block_visual.0 + 1),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    compiled.item_visuals = visuals
        .iter()
        .map(|&(metadata, route)| ItemVisualDefinition {
            key: ItemVisualKey {
                identifier: identifier.into(),
                metadata,
            },
            source: 0,
            route: match route {
                ItemVisualDefinitionRoute::Sprite { texture } => {
                    ItemVisualDefinitionRoute::Sprite {
                        texture: ItemTextureReference {
                            source: raster_source,
                            ..texture
                        },
                    }
                }
                route => route,
            },
            grip: assets::item::grip::ItemGrip::Unresolved,
            glint: assets::item::glint::ItemGlint::Unresolved,
            fuel: assets::item::fuel::ItemFuel::Unresolved,
            armour: assets::item::armour::ItemArmour::Unresolved,
            durability: assets::item::durability::ItemDurability::Unresolved,
            first_person: assets::ItemDisplayTransform::identity(),
            third_person: assets::ItemDisplayTransform::identity(),
            dropped: assets::ItemDisplayTransform::identity(),
        })
        .collect();
    compiled.item_visual_aliases = aliases
        .iter()
        .map(|&(metadata, visual)| ItemVisualAlias {
            key: ItemVisualKey {
                identifier: identifier.into(),
                metadata,
            },
            visual: ItemVisualId(visual),
        })
        .collect();
    Arc::new(RuntimeEntityAssets::from_compiled(compiled.with_unresolved_item_use()).unwrap())
}

/// Servers may still send a flattened block item with its legacy colour as the aux value;
/// the item keeps drawing the block it places instead of an unresolved route.
#[test]
fn legacy_aux_values_without_retained_blocks_draw_the_base_block() {
    let store = ItemStateStore::with_assets(item_assets(
        TERRACOTTA,
        &[(
            0,
            ItemVisualDefinitionRoute::BlockItem {
                block_visual: BlockVisualId(0),
            },
        )],
        &[],
    ));
    let network_id = protocol::vanilla_item_registry()
        .iter()
        .find(|entry| entry.identifier.as_ref() == TERRACOTTA)
        .unwrap()
        .network_id;
    for metadata in [0, 11] {
        let stack = NetworkItemStack {
            network_id,
            metadata,
            stack_network_id: -1,
            count: 64,
            nbt_digest: Sha256::digest([]).into(),
            block_runtime_id: 0,
            extra_data: Arc::from([]),
        };
        assert_eq!(
            store.canonicalize(&stack).unwrap().visual,
            ItemVisualRoute::BlockItem(BlockVisualId(0)),
            "aux {metadata} without a retained block"
        );
    }
}

/// Missing metadata routes must keep the block identity supplied by the stack.
#[test]
fn legacy_aux_values_preserve_a_retained_block() {
    let identifier = "minecraft:oak_log";
    let store = ItemStateStore::with_assets(item_assets(
        identifier,
        &[(
            0,
            ItemVisualDefinitionRoute::BlockItem {
                block_visual: BlockVisualId(0),
            },
        )],
        &[],
    ));
    let network_id = protocol::vanilla_item_registry()
        .iter()
        .find(|entry| entry.identifier.as_ref() == identifier)
        .unwrap()
        .network_id;
    for block_runtime_id in [1, i32::MIN] {
        let stack = NetworkItemStack {
            network_id,
            metadata: 1,
            stack_network_id: -1,
            count: 1,
            nbt_digest: Sha256::digest([]).into(),
            block_runtime_id,
            extra_data: Arc::from([]),
        };
        assert_eq!(
            store.canonicalize(&stack).unwrap().visual,
            ItemVisualRoute::RetainedBlock { block_runtime_id },
        );
    }
}

#[test]
fn legacy_block_fallback_does_not_select_a_base_sprite() {
    let identifier = "minecraft:arrow";
    let store = ItemStateStore::with_assets(item_assets(
        identifier,
        &[(
            0,
            ItemVisualDefinitionRoute::Sprite {
                texture: ItemTextureReference {
                    source: 0,
                    variant: 0,
                },
            },
        )],
        &[],
    ));
    assert_eq!(
        store.resolve_visual(identifier, 0, 0),
        ItemVisualRoute::Compiled(ItemVisualId(0))
    );
    assert_eq!(
        store.resolve_visual(identifier, 65, 0),
        ItemVisualRoute::Missing
    );
}

#[test]
fn exact_alias_keeps_priority_over_a_base_block_visual() {
    let store = ItemStateStore::with_assets(item_assets(
        TERRACOTTA,
        &[
            (
                0,
                ItemVisualDefinitionRoute::BlockItem {
                    block_visual: BlockVisualId(0),
                },
            ),
            (
                2,
                ItemVisualDefinitionRoute::Sprite {
                    texture: ItemTextureReference {
                        source: 0,
                        variant: 0,
                    },
                },
            ),
        ],
        &[(11, 1)],
    ));
    assert_eq!(
        store.resolve_visual(TERRACOTTA, 11, 0),
        ItemVisualRoute::Compiled(ItemVisualId(1))
    );
}
