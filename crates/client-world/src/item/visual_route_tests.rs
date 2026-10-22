use super::*;
use assets::ItemVisualDefinition;

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

/// One block item drawn as block visual 0, with no artwork or installed packs.
fn block_item_assets() -> Arc<RuntimeEntityAssets> {
    let mut compiled = assets::carriers::startup::test_carriers::synthetic_entity_assets(1);
    compiled.block_visual_count = 1;
    compiled.item_visuals = vec![ItemVisualDefinition {
        key: ItemVisualKey {
            identifier: TERRACOTTA.into(),
            metadata: 0,
        },
        source: 0,
        route: ItemVisualDefinitionRoute::BlockItem {
            block_visual: BlockVisualId(0),
        },
        grip: assets::item::grip::ItemGrip::Unresolved,
        glint: assets::item::glint::ItemGlint::Unresolved,
        fuel: assets::item::fuel::ItemFuel::Unresolved,
        armour: assets::item::armour::ItemArmour::Unresolved,
        durability: assets::item::durability::ItemDurability::Unresolved,
        first_person: assets::ItemDisplayTransform::identity(),
        third_person: assets::ItemDisplayTransform::identity(),
        dropped: assets::ItemDisplayTransform::identity(),
    }]
    .into();
    Arc::new(RuntimeEntityAssets::from_compiled(compiled.with_unresolved_item_use()).unwrap())
}

/// Servers may still send a flattened block item with its legacy colour as the aux value;
/// the item keeps drawing the block it places instead of an unresolved route.
#[test]
fn legacy_aux_values_on_block_items_draw_the_base_block() {
    let store = ItemStateStore::with_assets(block_item_assets());
    let network_id = protocol::vanilla_item_registry()
        .iter()
        .find(|entry| entry.identifier.as_ref() == TERRACOTTA)
        .unwrap()
        .network_id;
    for (metadata, block_runtime_id) in [(0, 0), (11, 0), (11, 0x1234_5678)] {
        let stack = NetworkItemStack {
            network_id,
            metadata,
            stack_network_id: -1,
            count: 64,
            nbt_digest: Sha256::digest([]).into(),
            block_runtime_id,
            extra_data: Arc::from([]),
        };
        assert_eq!(
            store.canonicalize(&stack).unwrap().visual,
            ItemVisualRoute::BlockItem(BlockVisualId(0)),
            "aux {metadata} with block runtime id {block_runtime_id:#x}"
        );
    }
}
