//! Original atlas generations exercise held artwork reload and rollback without game assets.

use super::*;
use crate::prepared_actor_artwork::{PreparedActorArtwork, session_resources};
use render_model::MIN_DYNAMIC_ATLAS_WIDTH;

/// A complete conduit texture whose atlas placement changes independently of its pixels.
fn conduit_carrier(color: [u8; 4], x: u32) -> assets::RuntimeBlockEntityAssets {
    let bytes = assets::encode_block_entity_catalog(
        b"{}",
        MIN_DYNAMIC_ATLAS_WIDTH,
        64,
        &color.repeat(MIN_DYNAMIC_ATLAS_WIDTH as usize * 64),
        &[assets::BlockEntityPlacement {
            name: assets::block_entity_geometry::CONDUIT_TEXTURE.0.into(),
            x,
            y: 0,
            width: assets::block_entity_geometry::CONDUIT_TEXTURE.1[0] as u32,
            height: assets::block_entity_geometry::CONDUIT_TEXTURE.1[1] as u32,
        }],
    )
    .unwrap();
    assets::RuntimeBlockEntityAssets::decode(&bytes).unwrap()
}

/// Reads the bound equipment layer's pixels rather than assuming a fixed page layout.
fn pixel(pages: &ActorArtworkPages, location: ActorArtworkLocation) -> [u8; 4] {
    let page = &pages.pages()[usize::from(location.page) - 1];
    let (width, height) = page.dimensions();
    let offset = location.layer as usize * usize::from(width) * usize::from(height) * 4;
    page.pixels()[offset..offset + 4].try_into().unwrap()
}

#[test]
fn held_block_entity_texture_reload_updates_hands_and_restores_startup() {
    let blue = [20, 80, 200, 255];
    let green = [40, 200, 80, 255];
    let (base, body, mut item) = block_fixture();
    item.identifier = Arc::from("minecraft:conduit");
    item.kind = HeldKind::Other;
    let carrier = conduit_carrier(blue, 0);
    let (mut runtime, pages, _) = EquipmentRuntime::build(
        Arc::clone(&base.assets),
        None,
        Arc::clone(&base.icons),
        None,
        Some(Arc::new(carrier)),
        ActorArtworkPages::default(),
    );
    runtime.register_skin_rig(body.input.rig, vec!["rightItem".into()]);
    let startup = runtime.take_pending_geometries();
    let art = Arc::new(
        render_model::BlockEntityScene::default()
            .artwork_preparer()
            .prepare(&conduit_carrier(green, 16))
            .unwrap(),
    );
    let prepared = PreparedActorArtwork::new_with_block_entity_artwork(&pages, None, Some(&art));
    let resources = session_resources(&pages, None, Some(&prepared));
    assert!(matches!(resources, std::borrow::Cow::Borrowed(_)));
    runtime.set_block_entity_artwork(resources.held_block_entities.as_ref());
    let main = runtime
        .first_person_item(&body, &item, ItemAnimationState::default())
        .unwrap();
    let off = runtime.first_person_offhand(&body, &item).unwrap();
    let third = third_person(&mut runtime, &body, &item);
    for location in [
        main.presentation.location,
        off.presentation.location,
        third.location,
    ] {
        assert_eq!(pixel(&resources.pages, location), green);
    }
    let rig = main.presentation.submission.input.rig;
    let changed = runtime.take_pending_geometries();
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].id, rig);
    assert_ne!(changed[0].vertices[0].uv, startup[0].vertices[0].uv);
    let allocations = test_alloc::thread_allocations();
    runtime.set_block_entity_artwork(resources.held_block_entities.as_ref());
    assert_eq!(test_alloc::thread_allocations(), allocations);
    assert!(runtime.take_pending_geometries().is_empty());

    runtime.set_block_entity_artwork(None);
    let restored = runtime
        .first_person_item(&body, &item, ItemAnimationState::default())
        .unwrap();
    assert_eq!(pixel(&pages, restored.presentation.location), blue);
    let restored_geometry = runtime.take_pending_geometries();
    assert_eq!(restored_geometry[0].vertices, startup[0].vertices);
}

#[test]
fn held_block_entity_preparation_rejects_a_different_source_generation() {
    let pages = ActorArtworkPages::default();
    let prepare = |color| {
        Arc::new(
            render_model::BlockEntityScene::default()
                .artwork_preparer()
                .prepare(&conduit_carrier(color, 0))
                .unwrap(),
        )
    };
    let first = prepare([20, 80, 200, 255]);
    let next = prepare([40, 200, 80, 255]);
    let prepared = PreparedActorArtwork::new_with_block_entity_artwork(&pages, None, Some(&first));
    assert!(prepared.matches_sources(&pages, None, Some(&first)));
    assert!(!prepared.matches_sources(&pages, None, Some(&next)));
    assert!(!prepared.matches_sources(&pages, None, None));
}

#[test]
fn unchanged_startup_artwork_reuses_pixels_bindings_and_registered_geometry() {
    let (base, body, mut item) = block_fixture();
    item.identifier = Arc::from("minecraft:conduit");
    item.kind = HeldKind::Other;
    let carrier = Arc::new(conduit_carrier([20, 80, 200, 255], 0));
    let (mut runtime, pages, _) = EquipmentRuntime::build(
        Arc::clone(&base.assets),
        None,
        Arc::clone(&base.icons),
        None,
        Some(Arc::clone(&carrier)),
        ActorArtworkPages::default(),
    );
    runtime.register_skin_rig(body.input.rig, vec!["rightItem".into()]);
    runtime.take_pending_geometries();
    let before = runtime
        .first_person_item(&body, &item, ItemAnimationState::default())
        .unwrap();
    let art = Arc::new(
        render_model::BlockEntityScene::default()
            .artwork_preparer()
            .prepare(&carrier)
            .unwrap(),
    );
    let prepared = PreparedActorArtwork::new_with_block_entity_artwork(&pages, None, Some(&art));
    let resources = session_resources(&pages, None, Some(&prepared));
    assert!(
        resources.pages.shares_storage_with(&pages),
        "no unchanged atlas may be copied, hashed, or appended"
    );
    assert_eq!(resources.pages.identity(), pages.identity());
    assert_eq!(resources.pages.pages().len(), pages.pages().len());
    let allocations = test_alloc::thread_allocations();
    runtime.set_block_entity_artwork(resources.held_block_entities.as_ref());
    assert_eq!(test_alloc::thread_allocations(), allocations);
    assert!(
        runtime.take_pending_geometries().is_empty(),
        "unchanged bindings retain registered meshes"
    );
    let after = runtime.first_person_offhand(&body, &item).unwrap();
    assert_eq!(after.presentation.location, before.presentation.location);
    assert_eq!(
        after.presentation.submission.input.rig,
        before.presentation.submission.input.rig
    );
}
