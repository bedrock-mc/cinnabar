//! Held heads retain the placed model's volume instead of extruding its inventory icon.

use super::*;
use crate::EntityRigId;

#[test]
fn held_head_items_resolve_models_from_the_pinned_block_facts() {
    let registry = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let mut checked = std::collections::BTreeSet::new();
    for block in &registry {
        let Some(kind) = SkullKind::from_block_identifier(&block.name) else {
            continue;
        };
        let Some((texture, logical)) = assets::block_entity_geometry::skull_texture(kind as u8)
        else {
            continue;
        };
        if !checked.insert(kind as u8) {
            continue;
        }
        let width = logical[0] as u32;
        let height = logical[1] as u32;
        let bytes = assets::encode_block_entity_catalog(
            b"{}",
            width,
            height,
            &vec![255; (width * height * 4) as usize],
            &[assets::BlockEntityPlacement {
                name: texture.into(),
                x: 0,
                y: 0,
                width,
                height,
            }],
        )
        .unwrap();
        let atlas = BlockEntityAtlas::from_assets(
            &assets::RuntimeBlockEntityAssets::decode(&bytes).unwrap(),
        );
        let held =
            HeldBlockEntity::from_item(&block.name).expect("head items need a held model route");
        let model = held_block_entity_model(EntityRigId(1), &atlas, held)
            .expect("resident head texture must produce held geometry");
        for axis in 0..3 {
            let (min, max) = model.geometry.vertices.iter().fold(
                (f32::INFINITY, f32::NEG_INFINITY),
                |(min, max), vertex| {
                    (
                        min.min(vertex.position[axis]),
                        max.max(vertex.position[axis]),
                    )
                },
            );
            assert!(
                max - min >= 0.5,
                "{} needs depth on axis {axis}",
                block.name
            );
        }
    }
    assert_eq!(checked.len(), SkullKind::packed_kinds().count());
}
