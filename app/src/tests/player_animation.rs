//! Independently authored player-shaped pack compiled end to end: rig scripts, a nested
//! root controller, expression channels, and actor-state queries drive the pose.
use assets::{EntityRigFallback, RuntimeAssets, RuntimeEntityAssets, encode_entity_blob};
use chunk_pipeline::WorldStream;
use client_world::{BoneTransform, LocalPlayerFeed};
use protocol::{
    ActorActionEvent, ActorActionKind, ActorEvent, ActorKind, ActorMetadata,
    ActorMetadataUpdateEvent, ActorMetadataValue, ActorSpawnEvent, CapeImage, ItemActorEvent,
    MovePlayerEvent, MovePlayerMode, PlayerListEntry, PlayerListUpdateEvent, PlayerSkin,
    SkinGeometrySource, StandardSkin, WorldBootstrap, WorldEvent,
};
use render::{ACTOR_LAYER_BODY, ActorRenderScene, ActorRigRejects, ActorRigRoute};
use render_model::STANDARD_SKIN_BYTES;
use std::{fs, path::PathBuf, sync::Arc};

const ENTITY: &str = r#"{"format_version":"1.26.0","minecraft:client_entity":{"description":{
 "identifier":"minecraft:player",
 "materials":{"default":"entity_alphatest"},
 "textures":{"default":"textures/entity/steve"},
 "geometry":{"default":"geometry.humanoid.custom","cape":"geometry.cape"},
 "scripts":{"scale":"0.9375",
  "initialize":["variable.is_holding_right = 0.0;"],
  "pre_animation":["variable.tcos0 = (Math.cos(query.modified_distance_moved * 38.17) * query.modified_move_speed / variable.gliding_speed_value) * 57.3;","variable.first_person_rotation_factor = math.sin((1 - variable.attack_time) * 180.0);"],
  "animate":["root"]},
 "animations":{"root":"controller.animation.player.root","look":"controller.animation.humanoid.look_at_target","look_default":"animation.humanoid.look_at_target.default","legs":"animation.player.move.legs","attack":"animation.player.attack.rotations","sneak":"animation.player.sneaking","fp_base":"animation.player.first_person.base_pose","fp_swap":"animation.player.first_person.swap_item","fp_attack":"animation.player.first_person.attack_rotation","unused":"controller.animation.player.base"},
 "render_controllers":[{"controller.render.player.first_person":"variable.is_first_person"},{"controller.render.player.third_person":"!variable.is_first_person"}]}}}"#;

const GEOMETRY: &str = r#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.humanoid.custom","texture_width":64,"texture_height":64},"bones":[
 {"name":"root","pivot":[0,0,0]},
 {"name":"body","parent":"root","pivot":[0,24,0],"cubes":[{"origin":[-4,12,-2],"size":[8,12,4],"uv":[16,16]}]},
 {"name":"head","parent":"body","pivot":[0,24,0],"cubes":[{"origin":[-4,24,-4],"size":[8,8,8],"uv":[0,0]}]},
 {"name":"rightArm","parent":"body","pivot":[-5,22,0],"cubes":[{"origin":[-8,12,-2],"size":[4,12,4],"uv":[40,16]}]},
 {"name":"leftArm","parent":"body","pivot":[5,22,0],"cubes":[{"origin":[4,12,-2],"size":[4,12,4],"uv":[32,48]}]},
 {"name":"rightLeg","parent":"root","pivot":[-1.9,12,0],"cubes":[{"origin":[-3.9,0,-2],"size":[4,12,4],"uv":[0,16]}]},
 {"name":"leftLeg","parent":"root","pivot":[1.9,12,0],"cubes":[{"origin":[-0.1,0,-2],"size":[4,12,4],"uv":[16,48]}]}]},
 {"description":{"identifier":"geometry.cape","texture_width":64,"texture_height":32},"bones":[
 {"name":"body","pivot":[0,24,0]},
 {"name":"cape","parent":"body","pivot":[0,24,3],"rotation":[0,180,0],"cubes":[{"origin":[-5,8,3],"size":[10,16,1],"uv":[0,0]}]}]}]}"#;

const ANIMATIONS: &str = r#"{"format_version":"1.8.0","animations":{
 "animation.humanoid.look_at_target.default":{"loop":true,"bones":{"head":{"relative_to":{"rotation":"entity"},"rotation":["query.target_x_rotation","query.target_y_rotation",0.0]}}},
 "animation.player.move.legs":{"loop":true,"bones":{"leftleg":{"rotation":["variable.tcos0 * -1.4",0.0,0.0]},"rightleg":{"rotation":["variable.tcos0 * 1.4",0.0,0.0]}}},
 "animation.player.attack.rotations":{"loop":true,"bones":{"rightarm":{"rotation":["-math.sin(variable.attack_time * 180) * 30",0.0,0.0]}}},
 "animation.player.sneaking":{"loop":true,"bones":{"root":{"rotation":["28.0 - this",0.0,0.0]}}},
 "animation.player.first_person.base_pose":{"loop":true,"bones":{"body":{"rotation":["query.target_x_rotation","query.target_y_rotation",0.0]}}},
 "animation.player.first_person.swap_item":{"loop":true,"bones":{"rightarm":{"position":[0.0,"-10.0 * (1.0 - variable.player_arm_height)",0.0]}}},
 "animation.player.first_person.attack_rotation":{"loop":true,"bones":{"rightarm":{"rotation":["math.sin(variable.first_person_item_rotation_factor * (1.0 - variable.attack_time) * (1.0 - variable.attack_time) * 280.0) * -60.0",0.0,0.0]}}}}}"#;

const CONTROLLERS: &str = r#"{"format_version":"1.10.0","animation_controllers":{
 "controller.animation.player.root":{"initial_state":"first_person","states":{
  "first_person":{"animations":["fp_base","fp_swap",{"fp_attack":"variable.attack_time > 0.0"}],"transitions":[{"third_person":"!variable.is_first_person"}]},
  "third_person":{"animations":[{"look":"!query.is_sleeping && !query.is_emoting"},"legs",{"attack":"variable.attack_time > 0.0"},{"sneak":"query.is_sneaking"},{"missing_clip":"query.get_equipped_item_name == 'bow'"}],
   "transitions":[{"first_person":"variable.is_first_person"}]}}},
 "controller.animation.humanoid.look_at_target":{"initial_state":"default","states":{"default":{"animations":["look_default"]}}}}}"#;

const RENDER: &str = r#"{"format_version":"1.8.0","render_controllers":{
 "controller.render.player.first_person":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]},
 "controller.render.player.third_person":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#;

struct Pack(PathBuf);

impl Pack {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "player-animation-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        for (path, contents) in [
            ("entity/player.entity.json", ENTITY),
            ("models/entity/player.geo.json", GEOMETRY),
            ("animations/player.animation.json", ANIMATIONS),
            (
                "animation_controllers/player.animation_controllers.json",
                CONTROLLERS,
            ),
            ("render_controllers/player.render_controllers.json", RENDER),
        ] {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        fs::create_dir_all(root.join("textures/entity")).unwrap();
        image::RgbaImage::from_pixel(64, 64, image::Rgba([90, 60, 40, 255]))
            .save(root.join("textures/entity/steve.png"))
            .unwrap();
        Self(root)
    }
}

impl Drop for Pack {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn entities() -> Arc<RuntimeEntityAssets> {
    let pack = Pack::new();
    let manifest = include_bytes!("../../../assets/vanilla-source.json");
    let compiled = pack_compiler::compile_entity_assets(&pack.0, manifest).unwrap();
    Arc::new(RuntimeEntityAssets::decode(&encode_entity_blob(&compiled).unwrap()).unwrap())
}

fn stream(entities: Arc<RuntimeEntityAssets>) -> WorldStream {
    WorldStream::new_with_asset_sets(
        WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0, 64.0, 0.0],
            world_spawn_position: [0, 64, 0],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        entities,
        [0.0, 64.0, 0.0],
        None,
    )
}

fn spawn_player() -> WorldEvent {
    WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
        dimension: 0,
        unique_id: -42,
        runtime_id: 42,
        kind: ActorKind::Player {
            uuid: [7; 16],
            username: "remote".into(),
        },
        position: [0.0, 64.0, 0.0],
        velocity: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        body_yaw: 0.0,
        held_item: Default::default(),
        metadata: Arc::from([]),
        attributes: Arc::from([]),
        properties: Arc::from([]),
        links: Arc::from([]),
    }))
}

fn move_player(x: f32, yaw: f32, pitch: f32, tick: u64) -> WorldEvent {
    WorldEvent::MovePlayer(MovePlayerEvent {
        runtime_id: 42,
        position: [x, 64.0 + protocol::PLAYER_NETWORK_OFFSET, 0.0],
        pitch,
        yaw,
        head_yaw: yaw,
        mode: MovePlayerMode::Normal,
        on_ground: true,
        teleported: false,
        source_tick: tick,
    })
}

fn bone(world: &WorldStream, entities: &RuntimeEntityAssets, name: &str) -> BoneTransform {
    bone_of(world, entities, 42, name)
}

fn bone_of(
    world: &WorldStream,
    entities: &RuntimeEntityAssets,
    runtime_id: u64,
    name: &str,
) -> BoneTransform {
    let rig = world.authority().actor_rig(runtime_id).unwrap();
    let geometry = entities.rig_geometries()[rig.rig.0 as usize].geometry as usize;
    let index = entities.geometries()[geometry]
        .bones
        .iter()
        .position(|bone| bone.name.eq_ignore_ascii_case(name))
        .unwrap();
    rig.current[index]
}

fn turned(transform: BoneTransform) -> bool {
    transform.rotation[3] < 0.9999
}

#[test]
fn vanilla_shaped_player_rig_resolves_animated_with_its_authored_scale() {
    let entities = entities();
    let mut world = stream(Arc::clone(&entities));
    world.submit(1, spawn_player()).unwrap();
    let rig = world.authority().actor_rig(42).unwrap();
    assert_eq!(rig.fallback, EntityRigFallback::Skip);
    assert_eq!(rig.scale, 0.9375);
    world.prepare_actor_appearance_fixture();
    world.advance_actor_interpolation_ticks(2);
    for name in ["leftLeg", "rightLeg", "rightArm", "head", "root"] {
        assert!(!turned(bone(&world, &entities, name)), "{name} rests");
    }
}

#[test]
fn walking_swings_the_legs_in_opposition_and_stopping_settles_them() {
    let entities = entities();
    let mut world = stream(Arc::clone(&entities));
    world.submit(1, spawn_player()).unwrap();
    for step in 1..=6_u64 {
        world
            .submit(step + 1, move_player(step as f32 * 0.25, -90.0, 0.0, step))
            .unwrap();
        world.prepare_actor_appearance_fixture();
        world.advance_actor_interpolation_ticks(1);
    }
    let left = bone(&world, &entities, "leftLeg");
    let right = bone(&world, &entities, "rightLeg");
    assert!(turned(left) && turned(right));
    assert!(left.rotation[0] * right.rotation[0] < 0.0, "legs oppose");
    world.prepare_actor_appearance_fixture();
    world.advance_actor_interpolation_ticks(60);
    assert!(!turned(bone(&world, &entities, "leftLeg")));
}

#[test]
fn head_pitch_turns_the_head_through_the_nested_look_controller() {
    let entities = entities();
    let mut world = stream(Arc::clone(&entities));
    world.submit(1, spawn_player()).unwrap();
    world.submit(2, move_player(0.0, 0.0, 30.0, 1)).unwrap();
    world.prepare_actor_appearance_fixture();
    world.advance_actor_interpolation_ticks(4);
    assert!(turned(bone(&world, &entities, "head")));
    assert!(!turned(bone(&world, &entities, "leftLeg")));
}

#[test]
fn arm_swing_action_animates_the_attack_then_returns_to_rest() {
    let entities = entities();
    let mut world = stream(Arc::clone(&entities));
    world.submit(1, spawn_player()).unwrap();
    world.prepare_actor_appearance_fixture();
    world.advance_actor_interpolation_ticks(1);
    world
        .submit(
            2,
            WorldEvent::ItemActor(ItemActorEvent::Action(ActorActionEvent {
                actor_runtime_ids: Arc::from([42_u64]),
                kind: ActorActionKind::SwingArm,
                data: 0.0,
                swing_source: None,
            })),
        )
        .unwrap();
    world.prepare_actor_appearance_fixture();
    world.advance_actor_interpolation_ticks(3);
    assert!(turned(bone(&world, &entities, "rightArm")));
    world.prepare_actor_appearance_fixture();
    world.advance_actor_interpolation_ticks(6);
    assert!(!turned(bone(&world, &entities, "rightArm")));
}

#[test]
fn sneaking_flag_overrides_the_root_tilt_through_this() {
    let entities = entities();
    let mut world = stream(Arc::clone(&entities));
    world.submit(1, spawn_player()).unwrap();
    world
        .submit(
            2,
            WorldEvent::Actor(ActorEvent::Metadata(ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 42,
                metadata: Arc::from([ActorMetadata {
                    key: 0,
                    value: ActorMetadataValue::Flags(1 << 1),
                }]),
                properties: Arc::from([]),
                tick: 2,
            })),
        )
        .unwrap();
    world.prepare_actor_appearance_fixture();
    world.advance_actor_interpolation_ticks(2);
    let root = bone(&world, &entities, "root");
    let angle = 2.0 * root.rotation[3].clamp(-1.0, 1.0).acos().to_degrees();
    assert!((angle - 28.0).abs() < 1.0e-3, "root tilt {angle}");
}

fn rotate(rotation: [f32; 4], vector: [f32; 3]) -> [f32; 3] {
    let quat = bevy::math::Quat::from_xyzw(rotation[0], rotation[1], rotation[2], rotation[3]);
    (quat * bevy::math::Vec3::from_array(vector)).to_array()
}

#[test]
fn head_faces_the_reported_head_yaw_and_pitch_in_the_world() {
    let entities = entities();
    let mut world = stream(Arc::clone(&entities));
    world.submit(1, spawn_player()).unwrap();
    world.submit(2, move_player(0.0, 40.0, 20.0, 1)).unwrap();
    // Rotation reaches the packet target over the three interpolation steps.
    world.prepare_actor_appearance_fixture();
    world.advance_actor_interpolation_ticks(3);
    let rig = world.authority().actor_rig(42).unwrap();
    assert!(rig.body_yaw.abs() < 40.0, "the body lags the head");
    let model = crate::presentation::actors::rig_world_from_actor([0.0; 3], rig.body_yaw, 1.0);
    let forward = rotate(bone(&world, &entities, "head").rotation, [0.0, 0.0, -1.0]);
    let world_forward: [f32; 3] =
        std::array::from_fn(|row| (0..3).map(|axis| model[row][axis] * forward[axis]).sum());
    let (yaw, pitch) = (40.0_f32.to_radians(), 20.0_f32.to_radians());
    let expected = [
        -yaw.sin() * pitch.cos(),
        -pitch.sin(),
        yaw.cos() * pitch.cos(),
    ];
    for axis in 0..3 {
        assert!(
            (world_forward[axis] - expected[axis]).abs() < 1.0e-3,
            "{world_forward:?} vs {expected:?}"
        );
    }
}

#[test]
fn sneaking_keeps_the_heads_entity_relative_look_direction() {
    let entities = entities();
    for (yaw, pitch) in [(0.0, 0.0), (40.0, 20.0), (-35.0, -25.0)] {
        let mut world = stream(Arc::clone(&entities));
        world.submit(1, spawn_player()).unwrap();
        world.submit(2, move_player(0.0, yaw, pitch, 1)).unwrap();
        world.prepare_actor_appearance_fixture();
        world.advance_actor_interpolation_ticks(3);
        let standing_head = bone(&world, &entities, "head");
        let world_direction = |world: &WorldStream, rotation, direction| {
            let rig = world.authority().actor_rig(42).unwrap();
            let model =
                crate::presentation::actors::rig_world_from_actor([0.0; 3], rig.body_yaw, 1.0);
            let vector = rotate(rotation, direction);
            std::array::from_fn::<f32, 3, _>(|row| {
                (0..3).map(|axis| model[row][axis] * vector[axis]).sum()
            })
        };
        let directions = [[0.0, 0.0, -1.0], [0.0, 1.0, 0.0]];
        let standing_directions =
            directions.map(|direction| world_direction(&world, standing_head.rotation, direction));
        for (revision, flags) in [(3, 1 << 1), (4, 0)] {
            world
                .submit(
                    revision,
                    WorldEvent::Actor(ActorEvent::Metadata(ActorMetadataUpdateEvent {
                        dimension: 0,
                        runtime_id: 42,
                        metadata: Arc::from([ActorMetadata {
                            key: 0,
                            value: ActorMetadataValue::Flags(flags),
                        }]),
                        properties: Arc::from([]),
                        tick: revision,
                    })),
                )
                .unwrap();
            world.prepare_actor_appearance_fixture();
            world.advance_actor_interpolation_ticks(1);
            let head = bone(&world, &entities, "head");
            for (direction, expected) in directions.into_iter().zip(standing_directions) {
                let actual = world_direction(&world, head.rotation, direction);
                for axis in 0..3 {
                    assert!(
                        (actual[axis] - expected[axis]).abs() < 1.0e-3,
                        "yaw {yaw}, pitch {pitch}, flags {flags}: {actual:?} vs {expected:?}"
                    );
                }
            }
            if flags != 0 {
                assert!(turned(bone(&world, &entities, "root")));
                assert!(
                    head.translation_scale[1] < standing_head.translation_scale[1],
                    "the head pivot still follows the crouching root"
                );
            } else {
                assert!(!turned(bone(&world, &entities, "root")));
            }
        }
    }
}

#[test]
fn rig_frame_front_faces_the_yaw_and_its_right_side_faces_the_models_right() {
    let at = |yaw: f32, vector: [f32; 3]| {
        let model = crate::presentation::actors::rig_world_from_actor([0.0; 3], yaw, 2.0);
        std::array::from_fn::<f32, 3, _>(|row| {
            (0..3)
                .map(|axis| model[row][axis] * vector[axis])
                .sum::<f32>()
        })
    };
    let close = |left: [f32; 3], right: [f32; 3]| {
        left.iter().zip(right).all(|(a, b)| (a - b).abs() < 1.0e-5)
    };
    assert!(
        close(at(0.0, [0.0, 0.0, -1.0]), [0.0, 0.0, 2.0]),
        "yaw 0 faces south"
    );
    assert!(
        close(at(0.0, [1.0, 0.0, 0.0]), [-2.0, 0.0, 0.0]),
        "right side is west"
    );
    assert!(
        close(at(90.0, [0.0, 0.0, -1.0]), [-2.0, 0.0, 0.0]),
        "yaw 90 faces west"
    );
    assert!(
        close(at(0.0, [0.0, 1.0, 0.0]), [0.0, 2.0, 0.0]),
        "scaled about the feet"
    );
}

fn skinned_player_list(skin: u8, cape: u8) -> WorldEvent {
    player_list_with(skin, Some(cape), None)
}

fn player_list_with(skin: u8, cape: Option<u8>, geometry: Option<(&str, &str)>) -> WorldEvent {
    WorldEvent::Actor(ActorEvent::PlayerList(PlayerListUpdateEvent {
        entries: Arc::from([PlayerListEntry::Add {
            uuid: [7; 16],
            unique_id: -42,
            username: "remote".into(),
            verified: true,
            skin: PlayerSkin::Standard(StandardSkin {
                geometry: geometry.map(|(resource_patch, geometry_data)| {
                    Arc::new(SkinGeometrySource {
                        animations: Arc::from([]),
                        resource_patch: resource_patch.into(),
                        geometry_data: geometry_data.into(),
                    })
                }),
                cape: cape.map(|cape| CapeImage {
                    width: 64,
                    height: 32,
                    rgba8: vec![cape; 64 * 32 * 4].into(),
                }),
                width: render_model::STANDARD_SKIN_SIDE as u32,
                height: render_model::STANDARD_SKIN_SIDE as u32,
                rgba8: vec![skin; STANDARD_SKIN_BYTES].into(),
            }),
        }]),
    }))
}

// Players draw from their own skin, never pack artwork: the body samples its skin layer and the
// cape samples the layer appended after it.
#[test]
fn skinned_player_publishes_a_drawable_body_and_cape_on_the_skin_page() {
    use crate::presentation::{actors, cape};
    let entities = entities();
    let mut world = stream(Arc::clone(&entities));
    world.submit(1, skinned_player_list(200, 90)).unwrap();
    world.submit(2, spawn_player()).unwrap();
    world.prepare_actor_appearance_fixture();
    world.advance_actor_interpolation_ticks(2);
    let rig = world.authority().actor_rig(42).unwrap();
    let body = actors::actor_rig_presentation(
        &rig,
        world.authority().actor(42).unwrap(),
        world.authority().actor_player_profile(42),
        0.5,
    )
    .unwrap();
    let mut batch = actors::select_actor_presentations(1, false, None, [body]);
    let mut scene = ActorRenderScene::with_runtime_entity_assets(&entities).unwrap();
    let mut capes = cape::CapeState::default();
    let cape_rig = capes.rig(Some(&entities)).expect("cape geometry resolves");
    scene.insert_geometry(cape_rig.geometry.clone()).unwrap();
    cape::apply_capes(
        &mut batch,
        cape_rig,
        |runtime_id| world.authority().actor_rig(runtime_id),
        |runtime_id| world.authority().actor_player_profile(runtime_id),
        |_| None,
        |_| false,
    );
    let frame = actors::update_actor_rig_scene(&mut scene, 0.5, batch);
    assert_eq!(frame.rig.rejects, ActorRigRejects::default());
    let layers = frame
        .rig
        .manifest
        .iter()
        .zip(frame.rig.instances.iter())
        .map(|(entry, instance)| {
            let pixels = frame
                .player_skin(instance.texture_layer)
                .expect("resident skin");
            assert_eq!(pixels.len(), STANDARD_SKIN_BYTES);
            (
                entry.identity.layer,
                entry.route,
                pixels[0],
                pixels.iter().all(|byte| *byte == pixels[0]),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        layers,
        [
            (ACTOR_LAYER_BODY, ActorRigRoute::Compiled, 200, true),
            (cape::ACTOR_LAYER_CAPE, ActorRigRoute::Compiled, 90, true),
        ]
    );
}

fn local_feed(main_hand: Option<&str>) -> LocalPlayerFeed {
    LocalPlayerFeed {
        game_mode: None,
        uuid: [5; 16],
        prefer_client_skin: false,
        username: "local".into(),
        skin: PlayerSkin::Unavailable(protocol::PlayerSkinUnavailable::InvalidDimensions),
        position: [0.0, 64.0, 0.0],
        velocity: [0.0; 3],
        on_ground: true,
        yaw: 30.0,
        head_yaw: 30.0,
        pitch: 40.0,
        main_hand: main_hand.map(Arc::from),
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
        first_person: true,
        view_bobbing: true,
        sneaking: false,
        sprinting: false,
        item_use: Default::default(),
    }
}

#[test]
fn view_bobbing_toggle_stops_authored_hand_bob_but_preserves_attacks() {
    let pack = Pack::new();
    let entity = ENTITY.replace(
        "\"fp_base\":\"animation.player.first_person.base_pose\"",
        "\"fp_bob\":\"animation.test.hand_bob\",\"fp_base\":\"animation.player.first_person.base_pose\"",
    );
    let animation = r#"{"loop":true,"bones":{"rightarm":{"rotation":[0,0,"math.sin(query.life_time * 90) * 5 + query.modified_move_speed * 5"]}}}"#;
    let mut animations: serde_json::Value = serde_json::from_str(ANIMATIONS).unwrap();
    animations["animations"]["animation.test.hand_bob"] = serde_json::from_str(animation).unwrap();
    let controllers = CONTROLLERS.replace(
        "\"fp_base\",\"fp_swap\"",
        "\"fp_base\",\"fp_swap\",{\"fp_bob\":\"variable.bob_animation\"}",
    );
    fs::write(pack.0.join("entity/player.entity.json"), entity).unwrap();
    fs::write(
        pack.0.join("animations/player.animation.json"),
        animations.to_string(),
    )
    .unwrap();
    fs::write(
        pack.0
            .join("animation_controllers/player.animation_controllers.json"),
        controllers,
    )
    .unwrap();
    let compiled = pack_compiler::compile_entity_assets(
        &pack.0,
        include_bytes!("../../../assets/vanilla-source.json"),
    )
    .unwrap();
    let entities =
        Arc::new(RuntimeEntityAssets::decode(&encode_entity_blob(&compiled).unwrap()).unwrap());
    let mut world = stream(Arc::clone(&entities));
    let mut feed = local_feed(None);
    for enabled in [true, false, true, false] {
        feed.view_bobbing = enabled;
        for _ in 0..4 {
            feed.position[0] += 0.2;
            world.sync_local_player_pose(&feed);
            world.prepare_actor_appearance_fixture();
            world.advance_actor_interpolation_frame(1);
        }
        assert_eq!(turned(bone_of(&world, &entities, 1, "rightArm")), enabled);
    }
    world.start_local_player_swing(client_world::ACTOR_SWING_TICKS);
    world.prepare_actor_appearance_fixture();
    world.advance_actor_interpolation_frame(2);
    assert!(turned(bone_of(&world, &entities, 1, "rightArm")));
}

// First person ignores the view rotation (the camera carries it) and lowers the arm only while
// a newly selected item equips.
#[test]
fn first_person_pose_ignores_the_view_and_dips_the_arm_while_an_item_equips() {
    let entities = entities();
    let mut world = stream(Arc::clone(&entities));
    let step = |main_hand: Option<&str>, world: &mut WorldStream| {
        world.sync_local_player_pose(&local_feed(main_hand));
        world.prepare_actor_appearance_fixture();
        world.advance_actor_interpolation_ticks(1);
        bone_of(world, &entities, 1, "rightArm").translation_scale[1]
    };
    let rest = (0..3).map(|_| step(None, &mut world)).last().unwrap();
    assert!(!turned(bone_of(&world, &entities, 1, "body")));
    assert_eq!(rest, 22.0, "a settled arm keeps its authored height");
    let swap = (0..8)
        .map(|_| step(Some("minecraft:stick"), &mut world))
        .collect::<Vec<_>>();
    assert!(
        swap[..3].iter().any(|height| *height < rest - 5.0),
        "{swap:?}"
    );
    assert_eq!(swap.last(), Some(&rest), "{swap:?}");
}

// The rig reports the equip progress it animates the arm with, a tick behind as well as now,
// so the first-person item dips and rises in step with the arm.
#[test]
fn rig_reports_the_equip_progress_of_its_last_two_ticks() {
    let entities = entities();
    let mut world = stream(Arc::clone(&entities));
    for _ in 0..3 {
        world.sync_local_player_pose(&local_feed(None));
        world.prepare_actor_appearance_fixture();
        world.advance_actor_interpolation_ticks(1);
    }
    let rest = world.authority().actor_rig(1).unwrap().hand;
    assert_eq!(rest.map(|phase| phase.arm_height), [1.0; 2]);
    let hands = (0..8)
        .map(|_| {
            world.sync_local_player_pose(&local_feed(Some("minecraft:stick")));
            world.prepare_actor_appearance_fixture();
            world.advance_actor_interpolation_ticks(1);
            world.authority().actor_rig(1).unwrap().hand
        })
        .collect::<Vec<_>>();
    assert!(
        hands.iter().any(|[_, current]| current.arm_height < 0.5),
        "{hands:?}"
    );
    for pair in hands.windows(2) {
        assert_eq!(
            pair[1][0], pair[0][1],
            "the previous phase is the last tick's"
        );
    }
    assert_eq!(hands.last().unwrap()[1].arm_height, 1.0);
}

const NPC_PATCH: &str = r#"{"geometry":{"default":"geometry.npc"}}"#;
const NPC_GEOMETRY: &str = r#"{"format_version":"1.12.0","minecraft:geometry":[{
 "description":{"identifier":"geometry.npc","texture_width":128,"texture_height":128},
 "bones":[
  {"name":"root","pivot":[0,0,0]},
  {"name":"body","parent":"root","pivot":[0,24,0],"cubes":[{"origin":[-6,10,-3],"size":[12,14,6],"uv":[0,0]}]},
  {"name":"head","parent":"body","pivot":[0,24,0],"cubes":[{"origin":[-5,24,-5],"size":[10,10,10],"uv":[0,40]}]},
  {"name":"tail","parent":"body","pivot":[0,12,3],"META_BoneType":"base","cubes":[{"origin":[-1,10,3],"size":[2,2,8],"uv":{"north":{"uv":[64,0],"uv_size":[2,2]}}}]}]}]}"#;

// A skin carrying its own model is drawn with it: the player's animations drive its bones by
// name and the skin's own rig geometry reaches the frame.
#[test]
fn skin_geometry_replaces_the_default_model_and_keeps_the_player_animations() {
    use crate::presentation::{actors, skin_rig::SkinRigCache};
    let entities = entities();
    let mut world = stream(Arc::clone(&entities));
    world
        .submit(
            1,
            player_list_with(200, None, Some((NPC_PATCH, NPC_GEOMETRY))),
        )
        .unwrap();
    world.submit(2, spawn_player()).unwrap();
    world.submit(3, move_player(0.0, 0.0, 30.0, 1)).unwrap();
    world.prepare_actor_appearance_fixture();
    world.advance_actor_interpolation_ticks(4);
    let rig = world.authority().actor_rig(42).unwrap();
    let geometry = rig.skin_geometry.expect("the skin model resolves").clone();
    assert_eq!(
        rig.bone_names,
        ["root", "body", "head", "tail"].map(Box::<str>::from)
    );
    assert_eq!(rig.current.len(), 4);
    assert!(
        turned(rig.current[2]),
        "the look animation turns the skin's head"
    );
    assert!(
        !turned(rig.current[3]),
        "a bone the player rig lacks stays at rest"
    );
    let mut presentation = actors::actor_rig_presentation(
        &rig,
        world.authority().actor(42).unwrap(),
        world.authority().actor_player_profile(42),
        0.5,
    )
    .unwrap();
    let mut scene = ActorRenderScene::with_runtime_entity_assets(&entities).unwrap();
    let mut cache = SkinRigCache::default();
    cache.begin_frame();
    let id = cache
        .rig(&geometry, rig.skin_mesh, |built| {
            scene.insert_geometry(built).unwrap()
        })
        .unwrap();
    assert_eq!(
        cache.rig(&geometry, rig.skin_mesh, |_| panic!("the model is cached")),
        Some(id)
    );
    presentation.submission.input.rig = id;
    let batch = actors::select_actor_presentations(1, false, None, [presentation]);
    let frame = actors::update_actor_rig_scene(&mut scene, 0.5, batch);
    assert_eq!(frame.rig.rejects, ActorRigRejects::default());
    assert_eq!(frame.rig.manifest[0].rig, id);
    assert_eq!(frame.rig.manifest[0].bone_count, 4);
}

// An unusable skin model falls back to the default geometry instead of hiding the player.
#[test]
fn malformed_skin_geometry_falls_back_to_the_default_model() {
    let entities = entities();
    let mut world = stream(Arc::clone(&entities));
    world
        .submit(
            1,
            player_list_with(200, None, Some((NPC_PATCH, "{not json"))),
        )
        .unwrap();
    world.submit(2, spawn_player()).unwrap();
    world.prepare_actor_appearance_fixture();
    world.advance_actor_interpolation_ticks(2);
    let rig = world.authority().actor_rig(42).unwrap();
    assert!(rig.skin_geometry.is_none());
    assert_eq!(rig.bone_names.len(), 7);
    assert_eq!(
        world
            .authority()
            .actor_animation_stats()
            .invalid_skin_geometries,
        1
    );
}

// The server never echoes the local player's own swing, so a local attack swings the local rig:
// the first-person arm through the pack's attack rotation and the third-person arm alike.
#[test]
fn a_local_swing_animates_the_first_and_third_person_arm() {
    let entities = entities();
    for first_person in [true, false] {
        let mut world = stream(Arc::clone(&entities));
        let feed = LocalPlayerFeed {
            first_person,
            ..local_feed(None)
        };
        let arm = |world: &mut WorldStream| {
            world.sync_local_player_pose(&feed);
            world.prepare_actor_appearance_fixture();
            world.advance_actor_interpolation_ticks(1);
            bone_of(world, &entities, 1, "rightArm")
        };
        for _ in 0..3 {
            arm(&mut world);
        }
        assert!(
            !turned(arm(&mut world)),
            "first person {first_person}: rests"
        );
        world.start_local_player_swing(client_world::ACTOR_SWING_TICKS);
        let swing = (0..3).map(|_| turned(arm(&mut world))).collect::<Vec<_>>();
        assert!(
            swing.contains(&true),
            "first person {first_person}: {swing:?}"
        );
        for _ in 0..6 {
            arm(&mut world);
        }
        assert!(
            !turned(arm(&mut world)),
            "first person {first_person}: settles"
        );
    }
}

/// The vanilla resource pack the local carriers compile from, when fetched.
fn vanilla_entities() -> Option<Arc<RuntimeEntityAssets>> {
    let root = vanilla_pack_root();
    if !root.join("entity/player.entity.json").is_file() {
        eprintln!(
            "skipping: vanilla resource pack not fetched at {}",
            root.display()
        );
        return None;
    }
    let manifest = include_bytes!("../../../assets/vanilla-source.json");
    let compiled = pack_compiler::compile_entity_assets(&root, manifest).unwrap();
    Some(Arc::new(
        RuntimeEntityAssets::decode(&encode_entity_blob(&compiled).unwrap()).unwrap(),
    ))
}

fn vanilla_pack_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../.local")
        .join(assets::vanilla_source().installed_pack_dir("resource_pack"))
}

/// A local skin carrying the vanilla humanoid model as its own geometry.
fn vanilla_skin_geometry() -> Option<PlayerSkin> {
    let path = vanilla_pack_root().join("models/entity/humanoid.custom.geo.json");
    let geometry_data = fs::read_to_string(path).ok()?;
    Some(PlayerSkin::Standard(StandardSkin {
        geometry: Some(Arc::new(SkinGeometrySource {
            animations: Arc::from([]),
            resource_patch: r#"{"geometry":{"default":"geometry.humanoid.custom"}}"#.into(),
            geometry_data: geometry_data.into(),
        })),
        cape: None,
        width: render_model::STANDARD_SKIN_SIDE as u32,
        height: render_model::STANDARD_SKIN_SIDE as u32,
        rgba8: vec![128; STANDARD_SKIN_BYTES].into(),
    }))
}

// The vanilla pack's own player rig must swing the local arm in both perspectives, holding an
// item or not, on the default model and on a skin's own model.
#[test]
fn a_local_swing_animates_the_vanilla_pack_arm() {
    let Some(entities) = vanilla_entities() else {
        eprintln!(
            "skipping a_local_swing_animates_the_vanilla_pack_arm: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let skins = [
        PlayerSkin::Unavailable(protocol::PlayerSkinUnavailable::InvalidDimensions),
        vanilla_skin_geometry().expect("vanilla humanoid geometry"),
    ];
    for skin in skins {
        for (first_person, held) in [
            (true, None),
            (false, None),
            (true, Some("minecraft:diamond_sword")),
            (false, Some("minecraft:diamond_sword")),
        ] {
            let mut world = stream(Arc::clone(&entities));
            let feed = LocalPlayerFeed {
                first_person,
                skin: skin.clone(),
                ..local_feed(held)
            };
            let arm = |world: &mut WorldStream| {
                world.sync_local_player_pose(&feed);
                world.prepare_actor_appearance_fixture();
                world.advance_actor_interpolation_ticks(1);
                let rig = world.authority().actor_rig(1).unwrap();
                let index = rig
                    .bone_names
                    .iter()
                    .position(|name| &**name == "rightarm")
                    .unwrap();
                rig.current[index].rotation
            };
            for _ in 0..3 {
                arm(&mut world);
            }
            let rest = arm(&mut world);
            assert_eq!(
                world
                    .authority()
                    .actor_rig(1)
                    .unwrap()
                    .skin_geometry
                    .is_some(),
                matches!(skin, PlayerSkin::Standard(_))
            );
            world.start_local_player_swing(client_world::ACTOR_SWING_TICKS);
            let swing = (0..3).map(|_| arm(&mut world)).collect::<Vec<_>>();
            let item = world.authority().actor_rig(1).unwrap().item_animation;
            assert!(item[1].attack_time > item[0].attack_time);
            assert!(item.iter().all(|item| item.arm_height.is_finite()));
            assert!(
                swing
                    .iter()
                    .any(|rotation| rotation.iter().zip(rest).any(|(a, b)| (a - b).abs() > 1e-3)),
                "first person {first_person}, {held:?}: rest {rest:?}, swing {swing:?}"
            );
        }
    }
}

// A melee press end to end: the swing packet goes out first and the same accepted swing turns
// the local vanilla-pack arm.
#[test]
fn a_local_attack_sends_the_swing_and_swings_the_vanilla_pack_arm() {
    use crate::melee::{ActorHit, Crosshair, MeleeRuntime, PressContext, SwingTracker};
    let Some(entities) = vanilla_entities() else {
        eprintln!(
            "skipping a_local_attack_sends_the_swing_and_swings_the_vanilla_pack_arm: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let empty = protocol::NetworkItemStack::empty();
    let press = PressContext {
        tick: 101,
        player_position: [0.0, 65.62, 0.0],
        input_mode: protocol::PlayerInputMode::Mouse,
        local_runtime_id: 1,
        selection: Some(crate::mining::FrozenMiningSelection {
            slot: 0,
            item: protocol::VerifiedNetworkItemStack::try_new(empty.clone(), empty.nbt_digest)
                .unwrap(),
        }),
        swing_duration: 6,
        item_attack: None,
        now_millis: 1_000,
    };
    let target = Crosshair::Actor(ActorHit {
        runtime_id: 42,
        distance: 2.0,
        point: [0.0, 65.0, -2.0],
    });
    let mut world = stream(Arc::clone(&entities));
    let feed = local_feed(None);
    let arm = |world: &mut WorldStream| {
        world.sync_local_player_pose(&feed);
        world.prepare_actor_appearance_fixture();
        world.advance_actor_interpolation_ticks(1);
        let rig = world.authority().actor_rig(1).unwrap();
        let index = rig
            .bone_names
            .iter()
            .position(|name| &**name == "rightarm")
            .unwrap();
        rig.current[index].rotation
    };
    for _ in 0..3 {
        arm(&mut world);
    }
    let rest = arm(&mut world);

    let mut melee = MeleeRuntime::default();
    let mut swings = SwingTracker::default();
    melee.observe_input(true, true);
    let outcome = melee.resolve(target, &press, &mut swings);
    let names: Vec<_> = outcome
        .packets
        .iter()
        .map(|packet| format!("{:?}", packet.header.id))
        .collect();
    assert_eq!(names, ["AnimatePacket", "InventoryTransactionPacket"]);
    world.start_local_player_swing(swings.take_started().expect("the attack swings"));
    let swing = (0..3).map(|_| arm(&mut world)).collect::<Vec<_>>();
    assert!(
        swing
            .iter()
            .any(|rotation| rotation.iter().zip(rest).any(|(a, b)| (a - b).abs() > 1e-3)),
        "rest {rest:?}, swing {swing:?}"
    );
}

#[test]
fn animated_skin_uses_its_own_rectangular_texture_geometry_and_uv_frame() {
    use crate::presentation::{actors, skin_layers, skin_rig};
    let patch =
        r#"{"geometry":{"default":"geometry.humanoid.custom","animated_32x32":"geometry.cape"}}"#;
    let WorldEvent::Actor(ActorEvent::PlayerList(mut update)) =
        player_list_with(200, None, Some((patch, GEOMETRY)))
    else {
        panic!("player list fixture");
    };
    let PlayerListEntry::Add {
        skin: PlayerSkin::Standard(skin),
        ..
    } = &mut Arc::make_mut(&mut update.entries)[0]
    else {
        panic!("standard skin fixture");
    };
    Arc::make_mut(skin.geometry.as_mut().unwrap()).animations =
        Arc::from([protocol::SkinAnimation {
            kind: protocol::SkinAnimationKind::Body32,
            width: 24,
            height: 512,
            rgba8: vec![255; 24 * 512 * 4].into(),
            frames: 16,
            blinking: false,
        }]);
    let entities = entities();
    let mut world = stream(Arc::clone(&entities));
    world
        .submit(1, WorldEvent::Actor(ActorEvent::PlayerList(update)))
        .unwrap();
    world.submit(2, spawn_player()).unwrap();
    world.prepare_actor_appearance_fixture();
    world.advance_actor_interpolation_ticks(4);
    let rig = world.authority().actor_rig(42).unwrap();
    assert_eq!(rig.skin_layers.len(), 1);
    let body = actors::actor_rig_presentation(
        &rig,
        world.authority().actor(42).unwrap(),
        world.authority().actor_player_profile(42),
        0.5,
    )
    .unwrap();
    let mut batch = actors::select_actor_presentations(1, false, None, [body]);
    let mut scene = ActorRenderScene::with_runtime_entity_assets(&entities).unwrap();
    let mut layers = skin_layers::SkinLayerCache::default();
    let mut geometries = Vec::new();
    let artwork = layers
        .apply(
            &mut batch,
            &Default::default(),
            |id| world.authority().actor_rig(id),
            &mut skin_rig::SkinRigCache::default(),
            |geometry| geometries.push(geometry),
        )
        .unwrap();
    assert_eq!(artwork.pages()[0].dimensions(), (24, 512));
    for geometry in geometries {
        scene.insert_geometry(geometry).unwrap();
    }
    scene.configure_artwork(artwork);
    let layer = &batch.submissions[1];
    assert_eq!(layer.uv_anim, rig.skin_layers[0].uv_anim);
    assert_eq!(
        layer.input.current_bones.len(),
        rig.skin_layers[0].current.len()
    );
    assert_eq!(
        layer.world_from_actor,
        batch.submissions[0].world_from_actor
    );
    let frame = actors::update_actor_rig_scene(&mut scene, 0.5, batch);
    assert_eq!(frame.rig.rejects, ActorRigRejects::default());
    assert_eq!(frame.rig.manifest.len(), 2);
    assert_eq!(
        frame.rig.manifest[1].identity.layer,
        skin_layers::SKIN_LAYER_BASE + protocol::SkinAnimationKind::Body32.slot() as u8
    );
    assert_eq!(frame.rig.manifest[1].bone_count, 2);
    assert_ne!(frame.instance_pages()[1], 0);
}

/// Replays the pinned carrier's real blink controller into the animated face UV frame.
#[test]
fn persona_face_blinks_with_the_pinned_runtime_controller() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let path = root.join(crate::asset_startup::DEFAULT_ASSET_PATH);
    for fixture in [
        path.clone(),
        crate::asset_startup::entity_asset_path(&path),
        crate::asset_startup::atmosphere_asset_path(&path),
    ] {
        if !fixture.is_file() {
            eprintln!(
                "skipping persona_face_blinks_with_the_pinned_runtime_controller: missing pinned runtime fixture {}",
                fixture.display()
            );
            return;
        }
    }
    let loaded = crate::asset_startup::load_runtime_assets(crate::asset_startup::AssetSelection {
        path,
        source: crate::asset_startup::AssetPathSource::CommandLine,
    })
    .unwrap();
    let entities = Arc::clone(loaded.entities.runtime());
    assert!(entities.controllers().iter().any(|controller| {
        entities.symbols()[controller.symbol as usize]
            .identifier
            .as_ref()
            == "controller.animation.persona.blink"
    }));
    let patch =
        r#"{"geometry":{"default":"geometry.humanoid.custom","animated_face":"geometry.cape"}}"#;
    let WorldEvent::Actor(ActorEvent::PlayerList(mut update)) =
        player_list_with(200, None, Some((patch, GEOMETRY)))
    else {
        panic!("player list fixture");
    };
    let PlayerListEntry::Add {
        skin: PlayerSkin::Standard(skin),
        ..
    } = &mut Arc::make_mut(&mut update.entries)[0]
    else {
        panic!("standard skin fixture");
    };
    Arc::make_mut(skin.geometry.as_mut().unwrap()).animations =
        Arc::from([protocol::SkinAnimation {
            kind: protocol::SkinAnimationKind::Face,
            width: 32,
            height: 64,
            rgba8: vec![255; 32 * 64 * 4].into(),
            frames: 2,
            blinking: true,
        }]);
    let mut world = stream(entities);
    world
        .submit(1, WorldEvent::Actor(ActorEvent::PlayerList(update)))
        .unwrap();
    world.submit(2, spawn_player()).unwrap();
    let mut opened = false;
    let mut closed = false;
    for _ in 0..800 {
        world.prepare_actor_appearance_fixture();
        world.advance_actor_interpolation_ticks(1);
        let rig = world.authority().actor_rig(42).unwrap();
        assert_eq!(rig.skin_layers.len(), 1);
        let offset = rig.skin_layers[0].uv_anim[1];
        opened |= offset == 0.0;
        closed |= offset == 0.5;
        if opened && closed {
            break;
        }
    }
    assert!(opened && closed, "face must publish both blink frames");
}

#[test]
fn selected_custom_local_model_shares_preparation_in_first_and_third_person() {
    let entities = entities();
    let mut world = stream(Arc::clone(&entities));
    let model = serde_json::json!({"format_version":"1.12.0","minecraft:geometry":[{
        "description":{"identifier":"geometry.local.fixture","texture_width":64,"texture_height":64},
        "bones":[
            {"name":"root"},
            {"name":"body","parent":"root","cubes":[{"origin":[-12,0,-4],"size":[24,8,8],"uv":[0,0]}]},
            {"name":"rightArm","parent":"body","pivot":[-12,8,0],"cubes":[{"origin":[-20,0,-2],"size":[8,8,4],"uv":[0,20]}]}
        ]
    }]}).to_string();
    let geometry = launcher::skin_import::parse_skin_geometry(model.as_bytes(), None).unwrap();
    let skin = StandardSkin {
        width: protocol::CLASSIC_SKIN_SIDE as u32,
        height: protocol::CLASSIC_SKIN_SIDE as u32,
        rgba8: vec![200; protocol::CLASSIC_SKIN_SIDE * protocol::CLASSIC_SKIN_SIDE * 4].into(),
        cape: None,
        geometry: Some(geometry.clone()),
    };
    let mut local = crate::player_skin::LocalPlayerSkin::generated_default("fixture");
    local.set_selection(&skin, launcher::dressing_room::SkinModel::Custom);
    assert_eq!(
        local.to_client_skin().geometry.unwrap().geometry_data,
        model
    );
    world
        .submit(
            1,
            player_list_with(
                200,
                None,
                Some((&geometry.resource_patch, &geometry.geometry_data)),
            ),
        )
        .unwrap();
    world.submit(2, spawn_player()).unwrap();
    let mut feed = LocalPlayerFeed {
        skin: local.player_skin(),
        ..local_feed(None)
    };
    for first_person in [false, true] {
        feed.first_person = first_person;
        world.sync_local_player_pose(&feed);
        world.prepare_actor_appearance_fixture();
        world.advance_actor_interpolation_ticks(1);
        let local = world.authority().actor_rig(1).unwrap();
        let remote = world.authority().actor_rig(42).unwrap();
        assert_eq!(
            &*local.skin_geometry.unwrap().identifier,
            "geometry.local.fixture"
        );
        assert!(
            local
                .bone_names
                .iter()
                .any(|name| name.eq_ignore_ascii_case("rightArm"))
        );
        assert!(
            Arc::ptr_eq(local.skin_geometry.unwrap(), remote.skin_geometry.unwrap()),
            "both camera modes reuse the remote worker's model cache"
        );
        assert!(Arc::ptr_eq(
            &local.skin_mesh.unwrap().vertices,
            &remote.skin_mesh.unwrap().vertices
        ));
        let mut cache = crate::presentation::skin_rig::SkinRigCache::default();
        cache.begin_frame();
        let mut registered = Vec::new();
        let local_id = cache
            .rig(local.skin_geometry.unwrap(), local.skin_mesh, |mesh| {
                registered.push(mesh)
            })
            .unwrap();
        let remote_id = cache
            .rig(remote.skin_geometry.unwrap(), remote.skin_mesh, |mesh| {
                registered.push(mesh)
            })
            .unwrap();
        assert_eq!(local_id, remote_id);
        assert_eq!(registered.len(), 1);
    }
}
