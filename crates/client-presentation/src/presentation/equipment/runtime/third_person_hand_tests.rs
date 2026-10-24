use super::*;
use crate::presentation::equipment::{LAYER_MAIN_HAND, display::LAYER_OFF_HAND};
use render_model::equipment::{held_block_display_for_hand, held_sprite_display_for_hand};

/// Gives both item bones the same pose so only the hand's display transform separates the draws.
fn two_hand_fixture(
    kind: HeldKind,
    upright: bool,
) -> (EquipmentRuntime, ActorRigSubmission, WornItem) {
    let (mut runtime, mut body, mut item) = block_fixture();
    runtime.register_skin_rig(body.input.rig, vec!["rightItem".into(), "leftItem".into()]);
    let hand = body.input.current_bones[0];
    body.input.previous_bones = Arc::from([hand; 2]);
    body.input.current_bones = Arc::clone(&body.input.previous_bones);
    item.kind = kind;
    item.grip = assets::item::grip::ItemGrip::Session {
        upright,
        mirrored_art: false,
    };
    let sprite = IconSprite {
        width: 16,
        height: 16,
        rgba8: vec![255; 16 * 16 * 4].into(),
    };
    runtime.icons = Arc::new(
        RuntimeIconCatalog::decode(
            &assets::encode_icon_catalog(
                [1; 32],
                std::slice::from_ref(&sprite),
                &[assets::IconEntry {
                    identifier: item.identifier.as_ref().into(),
                    metadata: 0,
                    sprite: 0,
                }],
            )
            .unwrap(),
        )
        .unwrap(),
    );
    let atlas = SpriteAtlas::pack(&[sprite]);
    runtime.placements = atlas.placements;
    let (_, locations) = ActorArtworkPages::default().with_equipment_rasters(&atlas.layers);
    runtime.atlas_locations = locations;
    (runtime, body, item)
}

#[test]
fn third_person_offhand_uses_its_own_native_grip() {
    for (kind, upright) in [
        (HeldKind::Sprite, false),
        (HeldKind::Sprite, true),
        (HeldKind::Block(7), false),
    ] {
        let (mut runtime, body, item) = two_hand_fixture(kind, upright);
        let layers = runtime.layers_for(
            &body,
            &ActorEquipmentInput {
                main: Some(item.clone()),
                off: Some(item),
                ..Default::default()
            },
            None,
        );
        assert_eq!(layers.len(), 2);
        for (layer, off_hand) in [(LAYER_MAIN_HAND, false), (LAYER_OFF_HAND, true)] {
            let display = if matches!(kind, HeldKind::Block(_)) {
                held_block_display_for_hand(off_hand)
            } else {
                held_sprite_display_for_hand(upright, off_hand)
            };
            let expected = attach_to_bone(body.input.current_bones[0], display).unwrap();
            let actual = layers
                .iter()
                .find(|draw| draw.submission.input.identity.layer == layer)
                .unwrap();
            assert_eq!(&*actual.submission.input.current_bones, &[expected]);
            assert_eq!(&*actual.submission.input.previous_bones, &[expected]);
        }
    }
}

#[test]
fn native_and_icon_fallback_sprites_keep_the_offhand_grip() {
    for upright in [false, true] {
        let (mut runtime, body, item) = two_hand_fixture(HeldKind::Sprite, upright);
        let (_, bones) = runtime.body_bones_for(body.input.rig).unwrap();
        let mut layers = Vec::new();
        runtime.push_icon_attached(&body, &item, LAYER_OFF_HAND, Some(1), 1.0, &mut layers);
        runtime.push_native_held(
            &body,
            &item,
            LAYER_OFF_HAND,
            &bones,
            None,
            actor_animation::AttachableAnimationInput {
                off_hand: true,
                ..Default::default()
            },
            &mut layers,
        );
        assert_eq!(layers.len(), 2);
        let expected = attach_to_bone(
            body.input.current_bones[1],
            held_sprite_display_for_hand(upright, true),
        )
        .unwrap();
        for draw in layers {
            assert_eq!(&*draw.submission.input.current_bones, &[expected]);
            assert_eq!(&*draw.submission.input.previous_bones, &[expected]);
        }
    }
}
