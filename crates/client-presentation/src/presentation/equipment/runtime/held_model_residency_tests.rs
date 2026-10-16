//! Partial model artwork must keep the held item's available icon.

use super::*;
use render_model::{MIN_DYNAMIC_ATLAS_WIDTH, java_animation::JavaHand};

/// Builds a valid synthetic pot atlas with independently optional base and side textures.
fn pot_assets(base: bool, side: bool) -> RuntimeBlockEntityAssets {
    use assets::block_entity_geometry as geometry;
    let placements = [
        (base, geometry::POT_BASE_TEXTURE),
        (side, geometry::POT_SIDE_TEXTURE),
    ]
    .into_iter()
    .filter(|(present, _)| *present)
    .enumerate()
    .map(|(index, (_, (name, size)))| assets::BlockEntityPlacement {
        name: name.into(),
        x: 0,
        y: index as u32 * 64,
        width: size[0] as u32,
        height: size[1] as u32,
    })
    .collect::<Vec<_>>();
    let bytes = assets::encode_block_entity_catalog(
        b"{}",
        MIN_DYNAMIC_ATLAS_WIDTH,
        128,
        &vec![255; MIN_DYNAMIC_ATLAS_WIDTH as usize * 128 * 4],
        &placements,
    )
    .unwrap();
    RuntimeBlockEntityAssets::decode(&bytes).unwrap()
}

/// Creates a held pot with a resident icon and optional startup model artwork.
fn pot_fixture(
    artwork: Option<RuntimeBlockEntityAssets>,
) -> (
    EquipmentRuntime,
    ActorArtworkPages,
    ActorRigSubmission,
    WornItem,
) {
    let (base, body, mut item) = block_fixture();
    item.identifier = Arc::from("minecraft:decorated_pot");
    item.kind = HeldKind::Sprite;
    let icons = RuntimeIconCatalog::decode(
        &assets::encode_icon_catalog(
            base.assets.source_manifest_sha256(),
            &[IconSprite {
                width: 16,
                height: 16,
                rgba8: vec![255; 16 * 16 * 4].into(),
            }],
            &[assets::IconEntry {
                identifier: item.identifier.as_ref().into(),
                metadata: 0,
                sprite: 0,
            }],
        )
        .unwrap(),
    )
    .unwrap();
    let (mut runtime, pages, _) = EquipmentRuntime::build(
        Arc::clone(&base.assets),
        None,
        Arc::new(icons),
        None,
        artwork.map(Arc::new),
        ActorArtworkPages::default(),
    );
    runtime.register_skin_rig(body.input.rig, vec!["rightItem".into()]);
    (runtime, pages, body, item)
}

/// Resolves the item's main-hand, offhand, Java and third-person geometry choices.
fn view_rigs(
    runtime: &mut EquipmentRuntime,
    body: &ActorRigSubmission,
    item: &WornItem,
) -> [EntityRigId; 4] {
    let main = runtime
        .first_person_item(body, item, ItemAnimationState::default())
        .unwrap();
    let off = runtime.first_person_offhand(body, item).unwrap();
    let java = runtime
        .first_person_java_item(
            body,
            item,
            JavaHand {
                swing: 0.0,
                equip: 1.0,
                using: None,
            },
        )
        .unwrap();
    let third = third_person(runtime, body, item);
    [
        main.presentation.submission.input.rig,
        off.presentation.submission.input.rig,
        java.presentation.submission.input.rig,
        third.submission.input.rig,
    ]
}

#[test]
fn partial_startup_pot_artwork_keeps_the_icon_in_every_view() {
    let (mut icons_only, _, body, item) = pot_fixture(None);
    let icon = view_rigs(&mut icons_only, &body, &item);
    for (base, side) in [(false, false), (true, false), (false, true)] {
        let (mut runtime, _, body, item) = pot_fixture(Some(pot_assets(base, side)));
        assert_eq!(
            view_rigs(&mut runtime, &body, &item),
            icon,
            "base {base}, side {side}: incomplete models must retain the icon"
        );
    }
    let (mut runtime, _, body, item) = pot_fixture(Some(pot_assets(true, true)));
    let models = view_rigs(&mut runtime, &body, &item);
    assert_ne!(models[0], icon[0]);
    assert!(models.iter().all(|rig| *rig == models[0]));
}

#[test]
fn partial_artwork_reload_keeps_the_icon_and_complete_artwork_restores_the_model() {
    let (mut icons_only, _, body, item) = pot_fixture(None);
    let icon = view_rigs(&mut icons_only, &body, &item);
    let (mut runtime, pages, body, item) = pot_fixture(Some(pot_assets(true, true)));
    let complete = view_rigs(&mut runtime, &body, &item);
    assert_ne!(complete[0], icon[0]);
    for (base, side) in [(false, false), (true, false), (false, true)] {
        let artwork = render_model::BlockEntityScene::default()
            .artwork_preparer()
            .prepare(&pot_assets(base, side))
            .unwrap();
        let (_updated_pages, prepared) =
            EquipmentRuntime::prepare_block_entity_artwork(&pages, &artwork);
        runtime.set_block_entity_artwork(Some(&prepared));
        assert_eq!(
            view_rigs(&mut runtime, &body, &item),
            icon,
            "base {base}, side {side}: reload must not select partial geometry"
        );
        runtime.set_block_entity_artwork(None);
        assert_eq!(view_rigs(&mut runtime, &body, &item), complete);
    }
}
