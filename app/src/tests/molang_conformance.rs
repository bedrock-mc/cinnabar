//! Independently authored expressions compiled end to end and read back through bone angles.
use assets::{RuntimeAssets, RuntimeEntityAssets, encode_entity_blob};
use client_world::WorldStream;
use protocol::{ActorEvent, ActorKind, ActorSpawnEvent, WorldBootstrap, WorldEvent};
use std::{fs, path::PathBuf, sync::Arc};

/// Each case: an expression and the value vanilla Molang gives it.
const CASES: [(&str, f32); 24] = [
    ("2 * 6 / 3", 4.0),
    ("math.round(-2.5)", -3.0),
    ("math.mod(-7, 3)", -1.0),
    ("math.clamp(10, 0, 5)", 5.0),
    ("math.min_angle(190)", -170.0),
    ("math.hermite_blend(0.5) * 10", 5.0),
    ("math.ease_out_quad(0, 100, 0.5)", 75.0),
    ("math.atan2(1, -1)", 135.0),
    ("1 / 0 + 1", 1.0),
    ("0 ? 7", 0.0),
    ("0 ? 1 : 2 ? 3 : 4", 3.0),
    ("v.loop_sum", 3.0),
    ("v.continued", 30.0),
    ("v.entries", 2.0),
    ("t.leftover ?? 7", 7.0),
    ("v.unset ?? 9", 9.0),
    ("v.zero ?? 9", 0.0),
    (
        "('abc' == 'abc') + ('abc' == 'ABC') * 10 + ('1' == 1) * 100",
        1.0,
    ),
    ("q.get_equipped_item_name == '' ? 11", 11.0),
    ("c.owning_entity->v.anything + 6", 6.0),
    ("q.is_alive + q.is_sneaking", 1.0),
    ("v.tmp = 4; return v.tmp * 3;", 12.0),
    ("v.ignored = 4;", 0.0),
    ("v.struct.depth.value", 13.0),
];

const PRE_ANIMATION: [&str; 6] = [
    "v.loop_sum = 0; loop(5, { v.loop_sum = v.loop_sum + 1; (v.loop_sum >= 3) ? break; });",
    "v.continued = 0; t.i = 0; loop(4, { t.i = t.i + 1; (t.i == 2) ? continue; v.continued = v.continued + 10; });",
    "v.entries = 1; t.leftover = 5;",
    "(v.entries == 1) ? {",
    "  v.entries = v.entries + 1; v.zero = 0;",
    "}; v.struct.depth.value = 13;",
];

struct Pack(PathBuf);

impl Drop for Pack {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_pack(entity: &str, files: Vec<(&str, serde_json::Value)>) -> Pack {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "molang-conformance-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&root);
    for (path, value) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    }
    for directory in ["animation_controllers", "textures/entity"] {
        fs::create_dir_all(root.join(directory)).unwrap();
    }
    image::RgbaImage::from_pixel(16, 16, image::Rgba([1, 2, 3, 255]))
        .save(root.join(format!("textures/entity/{entity}.png")))
        .unwrap();
    Pack(root)
}

fn entity_file(
    entity: &str,
    scripts: serde_json::Value,
    animations: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({"format_version":"1.10.0","minecraft:client_entity":{"description":{
        "identifier": format!("minecraft:{entity}"),
        "materials":{"default":"entity_alphatest"},
        "textures":{"default": format!("textures/entity/{entity}")},
        "geometry":{"default": format!("geometry.{entity}")},
        "scripts": scripts,
        "animations": animations,
        "render_controllers":[format!("controller.render.{entity}")]}}})
}

fn geometry_file(entity: &str, bones: Vec<serde_json::Value>) -> serde_json::Value {
    serde_json::json!({"format_version":"1.12.0","minecraft:geometry":[{
        "description":{"identifier": format!("geometry.{entity}"),"texture_width":16,"texture_height":16},
        "bones": bones}]})
}

fn render_file(entity: &str) -> serde_json::Value {
    serde_json::json!({"format_version":"1.8.0","render_controllers":{
        format!("controller.render.{entity}"):{"geometry":"Geometry.default",
            "materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}})
}

fn pack() -> Pack {
    let bones = (0..CASES.len())
        .map(|index| serde_json::json!({"name": format!("b{index}"), "pivot": [0, 0, 0]}))
        .collect::<Vec<_>>();
    let channels = CASES
        .iter()
        .enumerate()
        .map(|(index, (expression, _))| {
            (
                format!("b{index}"),
                serde_json::json!({"rotation": [expression, 0.0, 0.0]}),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    write_pack(
        "probe",
        vec![
            (
                "entity/probe.entity.json",
                entity_file(
                    "probe",
                    serde_json::json!({"pre_animation": PRE_ANIMATION, "animate":["probe"]}),
                    serde_json::json!({"probe":"animation.probe"}),
                ),
            ),
            (
                "models/entity/probe.geo.json",
                geometry_file("probe", bones),
            ),
            (
                "animations/probe.animation.json",
                serde_json::json!({"format_version":"1.8.0","animations":{
                    "animation.probe":{"loop":true,"bones":channels}}}),
            ),
            (
                "render_controllers/probe.render_controllers.json",
                render_file("probe"),
            ),
        ],
    )
}

fn spawned(entity: &str, pack: &Pack) -> (WorldStream, Arc<RuntimeEntityAssets>) {
    let manifest = include_bytes!("../../../assets/vanilla-source.json");
    let compiled = pack_compiler::compile_entity_assets(&pack.0, manifest).unwrap();
    let entities =
        Arc::new(RuntimeEntityAssets::decode(&encode_entity_blob(&compiled).unwrap()).unwrap());
    let mut world = WorldStream::new_with_asset_sets(
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
        Arc::clone(&entities),
        [0.0, 64.0, 0.0],
        None,
    );
    world
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: -5,
                runtime_id: 5,
                kind: ActorKind::Entity {
                    identifier: format!("minecraft:{entity}").into(),
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
            })),
        )
        .unwrap();
    (world, entities)
}

/// Authored X angle of a named bone, undoing the rig frame's mirrored X rotation.
fn authored_x_angle(world: &WorldStream, entities: &RuntimeEntityAssets, name: &str) -> f32 {
    let rig = world.actor_rig(5).unwrap();
    let geometry =
        &entities.geometries()[entities.rig_geometries()[rig.rig.0 as usize].geometry as usize];
    let bone = geometry
        .bones
        .iter()
        .position(|bone| bone.name.as_ref() == name)
        .unwrap();
    let rotation = rig.current[bone].rotation;
    -2.0 * rotation[0].atan2(rotation[3]).to_degrees()
}

#[test]
fn authored_expressions_evaluate_to_their_vanilla_values() {
    let pack = pack();
    let (mut world, entities) = spawned("probe", &pack);
    world.advance_actor_interpolation_ticks(1);
    for (index, (expression, expected)) in CASES.iter().enumerate() {
        let angle = authored_x_angle(&world, &entities, &format!("b{index}"));
        assert!(
            (angle - expected).abs() < 1.0e-3,
            "{expression} evaluated to {angle}, expected {expected}"
        );
    }
}

#[test]
fn a_state_leaves_once_its_clips_finish_and_the_next_state_starts_its_clip_at_zero() {
    let pack = write_pack(
        "clock",
        vec![
            (
                "entity/clock.entity.json",
                entity_file(
                    "clock",
                    serde_json::json!({"animate":["main"]}),
                    serde_json::json!({"main":"controller.animation.clock",
                        "wind":"animation.clock.wind","ring":"animation.clock.ring"}),
                ),
            ),
            (
                "models/entity/clock.geo.json",
                geometry_file(
                    "clock",
                    vec![serde_json::json!({"name":"hand","pivot":[0,0,0]})],
                ),
            ),
            (
                "animations/clock.animation.json",
                serde_json::json!({"format_version":"1.8.0","animations":{
                    "animation.clock.wind":{"animation_length":0.2,
                        "bones":{"hand":{"rotation":[10.0,0.0,0.0]}}},
                    "animation.clock.ring":{"loop":true,
                        "bones":{"hand":{"rotation":["q.anim_time * 400",0.0,0.0]}}}}}),
            ),
            (
                "animation_controllers/clock.animation_controllers.json",
                serde_json::json!({"format_version":"1.10.0","animation_controllers":{
                    "controller.animation.clock":{"initial_state":"wind","states":{
                        "wind":{"animations":["wind"],
                            "transitions":[{"ring":"q.all_animations_finished"}]},
                        "ring":{"animations":["ring"]}}}}}),
            ),
            (
                "render_controllers/clock.render_controllers.json",
                render_file("clock"),
            ),
        ],
    );
    let (mut world, entities) = spawned("clock", &pack);
    world.advance_actor_interpolation_ticks(3);
    assert!((authored_x_angle(&world, &entities, "hand") - 10.0).abs() < 1.0e-3);
    // The wind clip finishes on its fourth tick; the ring clip's clock then starts at zero.
    world.advance_actor_interpolation_ticks(1);
    assert!(authored_x_angle(&world, &entities, "hand").abs() < 1.0e-3);
    world.advance_actor_interpolation_ticks(1);
    assert!((authored_x_angle(&world, &entities, "hand") - 20.0).abs() < 1.0e-3);
}
