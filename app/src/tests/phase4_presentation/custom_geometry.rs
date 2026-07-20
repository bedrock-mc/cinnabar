use super::*;

#[test]
fn exact_pinned_custom_geometry_fingerprint_is_renderable_but_mismatch_is_not() {
    let bones = [model_bone([0.0, 24.0, 0.0])];
    let mut custom_profile = profile(9, 37);
    let PlayerSkin::Standard(skin) = &mut custom_profile.skin else {
        unreachable!();
    };
    skin.geometry = protocol::PlayerSkinGeometry::Custom {
        identifier: "geometry.humanoid.custom".into(),
        data_sha256: [0; 32],
    };
    assert!(
        local_player_rig_presentation(
            &local_rig(&bones),
            &custom_profile,
            LocalPlayerPresentationAuthority {
                actor_session_id: 7,
                dimension: 0,
                runtime_id: 42,
                pose_generation: 12,
                position: [1.0, 64.0, 2.0],
                yaw_degrees: 0.0,
            },
        )
        .is_some()
    );
    custom_profile.unique_id = 9;
    let remote = actor_rig_presentation(
        &rig(9, &bones, &bones),
        &actor(9, 11),
        Some(&custom_profile),
        0.5,
    )
    .unwrap();
    assert_eq!(remote.submission.route, ActorRigRoute::StaticFallback);

    let PlayerSkin::Standard(skin) = &mut custom_profile.skin else {
        unreachable!();
    };
    let protocol::PlayerSkinGeometry::Custom { data_sha256, .. } = &mut skin.geometry else {
        unreachable!();
    };
    *data_sha256 = [1; 32];
    assert_eq!(
        actor_rig_presentation(
            &rig(9, &bones, &bones),
            &actor(9, 11),
            Some(&custom_profile),
            0.5,
        )
        .unwrap()
        .submission
        .route,
        ActorRigRoute::NoDraw
    );
}
