use super::*;

#[test]
#[ignore = "requires PINNED_VANILLA_PACK pointing at the pinned official resource pack"]
fn login_skin_renders_local_manifest_without_player_list() {
    let pack = std::env::var_os("PINNED_VANILLA_PACK")
        .expect("set PINNED_VANILLA_PACK to the pinned official resource pack");
    let compiled = asset_compiler::compile_entity_assets(
        std::path::Path::new(&pack),
        include_bytes!("../../../../assets/vanilla-source.json"),
    )
    .unwrap();
    let bytes = assets::encode_entity_blob(&compiled).unwrap();
    let entity_assets = Arc::new(assets::RuntimeEntityAssets::decode(&bytes).unwrap());
    let mut stream = client_world::WorldStream::new_with_asset_sets(
        protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 42,
            player_position: [0.0, 64.0, 0.0],
            world_spawn_position: [0, 64, 0],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        Arc::clone(&entity_assets),
        [0.0, 64.0, 0.0],
        None,
    );
    stream.set_local_player_game_mode_authority(protocol::LocalPlayerGameModeAuthority::new(
        -9,
        ActorGameMode::Survival,
        ActorGameMode::Survival,
    ));
    stream.set_local_player_appearance_authority(
        protocol::LocalPlayerAppearanceAuthority::default_advertised(),
    );
    stream.advance_local_player_animation(client_world::LocalPlayerAnimationTickInput {
        tick: 1,
        velocity: [0.0; 3],
        on_ground: true,
        body_yaw: 0.0,
        head_yaw: 0.0,
        pitch: 0.0,
    });

    assert!(stream.local_player_profile().is_none());
    let local = crate::presentation::actors::local_player_skin_rig_presentation(
        &stream
            .local_player_rig()
            .expect("login-authority local rig"),
        stream
            .local_player_skin_authority()
            .expect("retained login skin"),
        LocalPlayerPresentationAuthority {
            actor_session_id: stream.actor_session_id(),
            dimension: 0,
            runtime_id: 42,
            pose_generation: 1,
            position: [0.0, 64.0, 0.0],
            yaw_degrees: 0.0,
        },
    )
    .expect("login skin local presentation");
    let batch = select_actor_presentations_for_view(42, true, Some(local), [], |_| true);
    let mut scene = ActorRenderScene::with_runtime_entity_assets(&entity_assets).unwrap();
    let frame = update_actor_rig_scene(&mut scene, 1.0, batch);
    assert_eq!(frame.rig.manifest.len(), 1);
    assert_eq!(frame.rig.manifest[0].identity.runtime_id, 42);
    assert!(!frame.skins_rgba8.is_empty());
}
