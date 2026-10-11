use super::*;

/// More remote appearances than the draw cap still share their source pixels with the hand.
fn profiles(legacy: bool) -> Vec<PlayerProfile> {
    let side = render_api::CLASSIC_SKIN_SIDE;
    (0..=MAX_RENDERED_PLAYERS)
        .map(|index| PlayerProfile {
            unique_id: 2,
            username: "fixture".into(),
            verified: true,
            skin: PlayerSkin::Standard(render_api::StandardSkin {
                width: side as u32,
                height: if legacy { side / 2 } else { side } as u32,
                rgba8: vec![index as u8; side * if legacy { side / 2 } else { side } * 4].into(),
                cape: None,
                geometry: None,
            }),
        })
        .collect()
}

#[test]
fn native_skin_working_set_with_local_hand_shares_square_source_pixels() {
    let actor = super::glide_tests::player([0.0; 3], 0);
    let profiles = profiles(false);
    for _ in 0..3 {
        for profile in &profiles {
            let PlayerSkin::Standard(source) = &profile.skin else {
                unreachable!()
            };
            let (_, skin) =
                player_route_and_skin(&actor, Some(profile), EntityRigFallback::GeometryOnly);
            assert!(
                Arc::ptr_eq(skin.unwrap().pixels(), source.rgba8.pixels()),
                "square skins must never expand before native publication"
            );
        }
    }
}

#[test]
fn native_skin_working_set_with_local_hand_retains_legacy_expansions() {
    let actor = super::glide_tests::player([0.0; 3], 0);
    let profiles = profiles(true);
    let skins: Vec<_> = profiles
        .iter()
        .map(|profile| {
            player_route_and_skin(&actor, Some(profile), EntityRigFallback::GeometryOnly)
                .1
                .unwrap()
        })
        .collect();
    let bytes = render_api::CLASSIC_SKIN_SIDE * render_api::CLASSIC_SKIN_SIDE * 4;
    for _ in 0..3 {
        for (profile, prepared) in profiles.iter().zip(&skins) {
            let skin =
                player_route_and_skin(&actor, Some(profile), EntityRigFallback::GeometryOnly)
                    .1
                    .unwrap();
            assert_eq!(
                skin.len(),
                bytes,
                "legacy expansion keeps its native dimensions"
            );
            assert!(
                Arc::ptr_eq(skin.pixels(), prepared.pixels()),
                "a draw-cap plus hand working set must not churn the legacy cache"
            );
        }
    }
}
