use std::sync::Arc;

use assets::{RuntimeAssets, RuntimeEntityAssets};
use chunk_pipeline::WorldStream;
use client_world::{LocalItemUse, LocalPlayerFeed};
use protocol::{PlayerSkin, PlayerSkinUnavailable, WorldBootstrap};

fn stream() -> WorldStream {
    let entity = br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:player","materials":{"default":"entity"},"textures":{"default":"textures/entity/test"},"geometry":{"default":"geometry.test"},"animations":{"look":"animation.test.look"},"scripts":{"animate":["look"]},"render_controllers":["controller.render.test"]}}}"#;
    let geometry = br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.test","texture_width":64,"texture_height":64},"bones":[{"name":"body","pivot":[0,0,0]},{"name":"head","parent":"body","pivot":[0,24,0],"cubes":[{"origin":[-4,24,-4],"size":[8,8,8],"uv":[0,0]}]}]}]}"#;
    let animation = br#"{"format_version":"1.8.0","animations":{"animation.test.look":{"loop":true,"animation_length":1,"anim_time_update":"query.anim_time + query.delta_time","bones":{"body":{"rotation":["query.anim_time * 45",0,0]},"head":{"rotation":["query.head_x_rotation(0)","query.head_y_rotation(180)",0]}}}}}"#;
    let controller = br#"{"format_version":"1.8.0","render_controllers":{"controller.render.test":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#;
    let compiled = pack_compiler::compile_entity_pack(vec![
        ("entity/player.json".into(), entity.to_vec()),
        ("models/entity/test.geo.json".into(), geometry.to_vec()),
        ("animations/test.animation.json".into(), animation.to_vec()),
        ("render_controllers/test.json".into(), controller.to_vec()),
    ])
    .unwrap()
    .unwrap();
    WorldStream::new_with_asset_sets(
        WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.0, 64.0, 0.0],
            world_spawn_position: [0, 64, 0],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        Arc::new(RuntimeEntityAssets::from_compiled(compiled.assets).unwrap()),
        [0.0, 64.0, 0.0],
        None,
    )
}

fn feed(first_person: bool) -> LocalPlayerFeed {
    LocalPlayerFeed {
        game_mode: None,
        prefer_client_skin: false,
        uuid: [1; 16],
        username: "test".into(),
        skin: PlayerSkin::Unavailable(PlayerSkinUnavailable::InvalidDimensions),
        position: [0.0, 64.0, 0.0],
        velocity: [0.0; 3],
        on_ground: true,
        yaw: 0.0,
        head_yaw: 35.0,
        pitch: 30.0,
        main_hand: None,
        off_hand: None,
        main_hand_metadata: 0,
        main_hand_slot: 0,
        main_hand_stack_id: None,
        bedrock_swing_ticks: client_world::ACTOR_SWING_TICKS,
        java_swing_ticks: client_world::ACTOR_SWING_TICKS,
        flying: false,
        gliding: false,
        fall_fly_ticks: 0,
        teleported: false,
        first_person,
        view_bobbing: true,
        sneaking: false,
        sprinting: false,
        item_use: LocalItemUse::Unpredicted,
    }
}

#[test]
fn perspective_head_switch_uses_new_view_pose_without_a_tick_or_old_pose_blend() {
    let mut actual = stream();
    let mut expected = stream();
    actual.sync_local_player_pose(&feed(true));
    expected.sync_local_player_pose(&feed(false));
    actual.advance_actor_interpolation_frame(1);
    expected.advance_actor_interpolation_frame(1);
    let before = actual.authority().actor_rig(1).unwrap();
    let tick = before.completed_tick;
    let old = before.current.to_vec();
    let third = expected.authority().actor_rig(1).unwrap().current.to_vec();
    assert_ne!(
        old, third,
        "fixture must distinguish the hand and world head"
    );
    actual.sync_local_player_pose(&feed(false));
    actual.advance_actor_interpolation_frame(0);
    let switched = actual.authority().actor_rig(1).unwrap();
    assert_eq!(
        switched.completed_tick, tick,
        "F5 must not advance simulation"
    );
    assert_eq!(switched.current, third);
    assert_eq!(switched.previous, switched.current, "no first-person blend");
    let generation = switched.reset_generation;
    actual.advance_actor_interpolation_frame(0);
    assert_eq!(
        actual.authority().actor_rig(1).unwrap().reset_generation,
        generation
    );
    actual.sync_local_player_pose(&feed(true));
    actual.advance_actor_interpolation_frame(0);
    let restored = actual.authority().actor_rig(1).unwrap();
    assert_eq!(restored.current, old);
    assert_eq!(restored.previous, restored.current);
}

#[test]
fn perspective_head_tick_transition_discards_only_cross_view_interpolation() {
    let mut actual = stream();
    actual.sync_local_player_pose(&feed(true));
    actual.advance_actor_interpolation_frame(1);
    let before = actual.authority().actor_rig(1).unwrap();
    let tick = before.completed_tick;
    let generation = before.reset_generation;
    let mut cache = crate::presentation::actors::PoseConversions::default();
    let body = crate::presentation::actors::actor_rig_presentation_cached(
        &before,
        actual.authority().actor(1).unwrap(),
        None,
        0.5,
        &mut cache,
    )
    .unwrap();
    actual.sync_local_player_pose(&feed(false));
    actual.advance_actor_interpolation_frame(1);
    let after = actual.authority().actor_rig(1).unwrap();
    assert_eq!(after.completed_tick, tick + 1);
    assert_ne!(after.reset_generation, generation);
    assert_eq!(after.previous, after.current);
    let switched = crate::presentation::actors::actor_rig_presentation_cached(
        &after,
        actual.authority().actor(1).unwrap(),
        None,
        0.5,
        &mut cache,
    )
    .unwrap();
    assert_ne!(
        body.submission.input.current_bones,
        switched.submission.input.current_bones
    );
    assert_eq!(
        switched.submission.input.previous_bones,
        switched.submission.input.current_bones
    );
}
