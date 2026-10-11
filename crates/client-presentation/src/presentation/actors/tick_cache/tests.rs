use std::sync::Arc;

use assets::{RuntimeAssets, RuntimeEntityAssets};
use client_world::WorldAuthority;
use protocol::{
    ActorEvent, ActorKind, ActorMoveEvent, ActorPositionOrigin, ActorSpawnEvent, PlayerListEntry,
    PlayerListUpdateEvent, PlayerSkin, WorldBootstrap, WorldEvent,
};
use render::ActorArtworkPages;
use render_api::StandardSkin;

use super::super::{
    PoseConversions, actor_rig_presentation, actor_rig_presentation_cached,
    entity_rig_presentation, entity_rig_presentation_cached,
};

const PLAYER: u64 = 2;
const MOB: u64 = 3;

/// A player and a mob whose bodies swing every tick, so each tick has a new pose.
fn world() -> WorldAuthority {
    let entity = |identifier: &str| {
        format!(
            r#"{{"format_version":"1.10.0","minecraft:client_entity":{{"description":{{"identifier":"{identifier}","materials":{{"default":"entity"}},"textures":{{"default":"textures/entity/test"}},"geometry":{{"default":"geometry.test"}},"animations":{{"swing":"animation.test.swing"}},"scripts":{{"animate":["swing"]}},"render_controllers":["controller.render.test"]}}}}}}"#
        )
    };
    let geometry = br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.test","texture_width":64,"texture_height":64},"bones":[{"name":"body","pivot":[0,0,0],"cubes":[{"origin":[-4,0,-4],"size":[8,8,8],"uv":[0,0]}]}]}]}"#;
    let animation = br#"{"format_version":"1.8.0","animations":{"animation.test.swing":{"loop":true,"animation_length":1,"bones":{"body":{"rotation":["query.life_time * 90",0,0]}}}}}"#;
    let controller = br#"{"format_version":"1.8.0","render_controllers":{"controller.render.test":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#;
    let compiled = pack_compiler::compile_entity_pack(vec![
        (
            "entity/player.json".into(),
            entity("minecraft:player").into_bytes(),
        ),
        (
            "entity/mob.json".into(),
            entity("minecraft:test_mob").into_bytes(),
        ),
        ("models/entity/test.geo.json".into(), geometry.to_vec()),
        ("animations/test.animation.json".into(), animation.to_vec()),
        ("render_controllers/test.json".into(), controller.to_vec()),
    ])
    .unwrap()
    .unwrap();
    let mut world = WorldAuthority::new(
        WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.0, 64.0, 0.0],
            world_spawn_position: [0, 64, 0],
            air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        Some(Arc::new(
            RuntimeEntityAssets::from_compiled(compiled.assets).unwrap(),
        )),
        [0.0, 64.0, 0.0],
        None,
    );
    let mut sequence = 0;
    let mut apply = |world: &mut WorldAuthority, event| {
        sequence += 1;
        world.apply_ordered_event(event, Some(sequence)).unwrap();
    };
    apply(&mut world, profile(1));
    for (runtime_id, kind) in [
        (
            PLAYER,
            ActorKind::Player {
                uuid: [PLAYER as u8; 16],
                username: "cached".into(),
            },
        ),
        (
            MOB,
            ActorKind::Entity {
                identifier: "minecraft:test_mob".into(),
            },
        ),
    ] {
        apply(
            &mut world,
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: runtime_id as i64,
                runtime_id,
                kind,
                position: [runtime_id as f32, 64.0, 4.0],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 30.0,
                head_yaw: 30.0,
                body_yaw: 30.0,
                held_item: Default::default(),
                metadata: Arc::from([]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
        );
    }
    world
}

/// The player's profile, wearing a skin of uniform `shade`.
fn profile(shade: u8) -> WorldEvent {
    WorldEvent::Actor(ActorEvent::PlayerList(PlayerListUpdateEvent {
        entries: Arc::from([PlayerListEntry::Add {
            uuid: [PLAYER as u8; 16],
            unique_id: PLAYER as i64,
            username: "cached".into(),
            verified: true,
            skin: PlayerSkin::Standard(StandardSkin {
                width: render_model::STANDARD_SKIN_SIDE as u32,
                height: render_model::STANDARD_SKIN_SIDE as u32,
                rgba8: vec![shade; render_model::STANDARD_SKIN_BYTES].into(),
                cape: None,
                geometry: None,
            }),
        }]),
    }))
}

fn step(runtime_id: u64, x: f32, yaw: f32) -> WorldEvent {
    WorldEvent::Actor(ActorEvent::Move(ActorMoveEvent {
        dimension: 0,
        runtime_id,
        position: [Some(x), None, None],
        position_origin: ActorPositionOrigin::Feet,
        pitch: None,
        yaw: Some(yaw),
        head_yaw: Some(yaw + 20.0),
        on_ground: Some(true),
        teleported: false,
        player_mode: None,
        source_tick: None,
        interpolation: Default::default(),
    }))
}

/// Every frame of a tick reuses one presentation and re-places it, matching an uncached build
/// exactly; a new tick or a newly published skin rebuilds it.
#[test]
fn frames_between_ticks_only_re_place_the_tick_presentation() {
    let mut world = world();
    let artwork = ActorArtworkPages::default();
    let mut poses = PoseConversions::default();
    let mut sequence = 100;
    let mut last_player = None;
    for tick in 0..4 {
        world.advance_actor_interpolation_frame(1);
        let builds = poses.presentation_builds();
        let mut transforms = Vec::new();
        for (frame, partial_tick) in [0.0, 0.3, 0.6, 0.9].into_iter().enumerate() {
            poses.begin_frame();
            if frame == 2 {
                // Movement packets between ticks change the frame's identity, not its pose.
                for runtime_id in [PLAYER, MOB] {
                    sequence += 1;
                    let x = tick as f32 + runtime_id as f32 + 0.5;
                    world
                        .apply_ordered_event(
                            step(runtime_id, x, 40.0 + tick as f32),
                            Some(sequence),
                        )
                        .unwrap();
                }
            }
            let rig = world.actor_rig(PLAYER).unwrap();
            let actor = world.actor(PLAYER).unwrap();
            let profile = world.actor_player_profile(PLAYER);
            let cached =
                actor_rig_presentation_cached(&rig, actor, profile, partial_tick, &mut poses)
                    .unwrap();
            let fresh = actor_rig_presentation(&rig, actor, profile, partial_tick).unwrap();
            assert_eq!(
                format!("{cached:?}"),
                format!("{fresh:?}"),
                "tick {tick} frame {frame}"
            );
            transforms.push(cached.submission.world_from_actor);
            last_player = Some(cached);

            let rig = world.actor_rig(MOB).unwrap();
            let actor = world.actor(MOB).unwrap();
            let cached = entity_rig_presentation_cached(
                &rig,
                actor,
                &artwork,
                partial_tick,
                Some(&mut poses),
            )
            .unwrap();
            let fresh = entity_rig_presentation(&rig, actor, &artwork, partial_tick).unwrap();
            assert_eq!(
                format!("{cached:?}"),
                format!("{fresh:?}"),
                "tick {tick} frame {frame}"
            );
        }
        assert_eq!(
            poses.presentation_builds(),
            builds + 2,
            "one build per rig per tick"
        );
        transforms.dedup();
        if tick > 0 {
            assert!(transforms.len() > 1, "a walking body moves between frames");
        }
    }

    // A profile change retains the ready skin until the next simulation tick.
    let before = poses.presentation_builds();
    world
        .apply_ordered_event(profile(9), Some(sequence + 1))
        .unwrap();
    poses.begin_frame();
    let rig = world.actor_rig(PLAYER).unwrap();
    let actor = world.actor(PLAYER).unwrap();
    let profile = world.actor_player_profile(PLAYER);
    let cached = actor_rig_presentation_cached(&rig, actor, profile, 0.5, &mut poses).unwrap();
    assert_eq!(poses.presentation_builds(), before);
    assert_eq!(cached.skin_rgba8, last_player.as_ref().unwrap().skin_rgba8);

    world.advance_actor_interpolation_frame(1);
    poses.begin_frame();
    let rig = world.actor_rig(PLAYER).unwrap();
    let actor = world.actor(PLAYER).unwrap();
    let profile = world.actor_player_profile(PLAYER);
    let cached = actor_rig_presentation_cached(&rig, actor, profile, 0.5, &mut poses).unwrap();
    let fresh = actor_rig_presentation(&rig, actor, profile, 0.5).unwrap();
    assert_eq!(poses.presentation_builds(), before + 1);
    assert_eq!(format!("{cached:?}"), format!("{fresh:?}"));
    assert_ne!(cached.skin_rgba8, last_player.unwrap().skin_rgba8);
}
