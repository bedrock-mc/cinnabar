//! Independently authored producer-to-adapter fixture; no trusted raw page constructor.
use crate::presentation::actors::entity_rig_presentation;
use assets::{RuntimeActorCatalog, RuntimeAssets, RuntimeEntityAssets, encode_entity_blob};
use client_world::{ActorRigSnapshot, WorldStream};
use protocol::{ActorEvent, ActorKind, ActorSpawnEvent, WorldBootstrap, WorldEvent};
use render::{ActorArtworkPages, ActorRigRoute};
use std::{fs, path::PathBuf, sync::Arc};

pub(super) struct Pack(PathBuf);
impl Pack {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "neutral-actor-test-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn write(&self, name: &str, contents: &[u8]) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
}
impl Drop for Pack {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture() -> (Pack, ActorArtworkPages, Arc<RuntimeEntityAssets>) {
    weighted_fixture("query.ground_speed", 1)
}

fn weighted_fixture(
    weight: &str,
    repetitions: usize,
) -> (Pack, ActorArtworkPages, Arc<RuntimeEntityAssets>) {
    compiled_fixture(weight, repetitions, assets::ActorPoseMode::RestPose)
}

/// A one-entity pack (`minecraft:example`) whose wave clip is weighted by `weight`, compiled with
/// its rig drawn in `pose_mode`.
pub(super) fn compiled_fixture(
    weight: &str,
    repetitions: usize,
    pose_mode: assets::ActorPoseMode,
) -> (Pack, ActorArtworkPages, Arc<RuntimeEntityAssets>) {
    let pack = Pack::new();
    pack.write("entity/example.json", br#"{"format_version":"1.8.0","minecraft:client_entity":{"description":{"identifier":"minecraft:example","geometry":{"default":"geometry.example"},"materials":{"default":"entity_alphatest"},"textures":{"default":"textures/entity/example"},"animations":{"wave":"animation.example.wave"},"animation_controllers":[{"general":"controller.animation.example"}],"render_controllers":["controller.render.example"]}}}"#);
    pack.write("models/entity/example.json", br#"{"format_version":"1.8.0","geometry.base":{"texturewidth":16,"textureheight":16,"bones":[{"name":"root","pivot":[1,0,0],"rotation":[0,0,90]}]},"geometry.example:geometry.base":{"texturewidth":16,"textureheight":16,"bones":[{"name":"tail","parent":"root","pivot":[3,0,0],"cubes":[{"origin":[0,0,0],"size":[0,2,7],"uv":[0,0]}]}]}}"#);
    pack.write("animations/example.json", br#"{"format_version":"1.8.0","animations":{"animation.example.wave":{"loop":true,"animation_length":2,"bones":{"tail":{"position":{"0":[0,0,0],"1":[8,0,0],"2":[0,0,0]}}}}}}"#);
    let animations = vec![serde_json::json!({"wave":weight}); repetitions];
    pack.write("animation_controllers/example.json", &serde_json::to_vec(&serde_json::json!({"format_version":"1.10.0","animation_controllers":{"controller.animation.example":{"initial_state":"default","states":{"default":{"animations":animations}}}}})).unwrap());
    pack.write("render_controllers/example.json", br#"{"format_version":"1.8.0","render_controllers":{"controller.render.example":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#);
    fs::create_dir_all(pack.0.join("textures/entity")).unwrap();
    image::RgbaImage::from_pixel(16, 16, image::Rgba([20, 40, 60, 255]))
        .save(pack.0.join("textures/entity/example.png"))
        .unwrap();
    let manifest = include_bytes!("../../../assets/vanilla-source.json");
    let entities = pack_compiler::compile_entity_assets(&pack.0, manifest).unwrap();
    let bytes = encode_entity_blob(&entities).unwrap();
    let compiled = pack_compiler::compile_actor_assets(&pack.0, manifest).unwrap();
    // The compiler binds every rig as a compiled pose; re-encode with the requested route.
    let catalog = RuntimeActorCatalog::decode(&compiled.bytes, &bytes).unwrap();
    let mut bindings = catalog.bindings().to_vec();
    assert_eq!(bindings.len(), 1);
    bindings[0].pose_mode = pose_mode;
    let rest_bytes = assets::encode_actor_catalog(&bytes, catalog.textures(), &bindings).unwrap();
    let catalog = RuntimeActorCatalog::decode(&rest_bytes, &bytes).unwrap();
    (
        pack,
        ActorArtworkPages::new(&catalog),
        Arc::new(RuntimeEntityAssets::decode(&bytes).unwrap()),
    )
}

fn spawn(unique_id: i64) -> WorldEvent {
    spawn_runtime(42, unique_id)
}

fn spawn_runtime(runtime_id: u64, unique_id: i64) -> WorldEvent {
    WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
        dimension: 0,
        unique_id,
        runtime_id,
        kind: ActorKind::Entity {
            identifier: "minecraft:example".into(),
        },
        position: [0.0, 64.0, 0.0],
        velocity: [0.1, 0.0, 0.0],
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
pub(super) fn stream(entities: Arc<RuntimeEntityAssets>) -> WorldStream {
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

#[test]
fn inherited_rotated_rest_is_drawn_instead_of_animated_pose_and_survives_replacement() {
    let (_pack, artwork, entities) = fixture();
    let mut world = stream(entities.clone());
    world.submit(1, spawn(-1)).unwrap();
    let rest = world.actor_rig(42).unwrap().rest.to_vec();
    // The rig frame mirrors authored X, so the root's +90 Z turn swings the tail down.
    assert!((rest[1].translation_scale[0] + 1.0).abs() < 1e-5);
    assert!((rest[1].translation_scale[1] + 2.0).abs() < 1e-5);
    world.advance_actor_interpolation_ticks(5);
    let rig = world.actor_rig(42).unwrap();
    assert_eq!(rig.rest, rest);
    assert_ne!(rig.current, rig.rest);
    let drawn = entity_rig_presentation(&rig, world.actor(42).unwrap(), &artwork, 0.5).unwrap();
    assert_eq!(drawn.submission.route, ActorRigRoute::StaticFallback);
    assert_eq!(
        drawn.submission.input.previous_bones,
        drawn.submission.input.current_bones
    );
    for (actual, expected) in drawn.submission.input.current_bones.iter().zip(&rest) {
        let expected = render::RenderBoneTransform::from_model_space(
            expected.rotation,
            expected.translation_scale,
        )
        .unwrap();
        assert_eq!(actual.translation_scale, expected.translation_scale);
        assert_eq!(actual.rotation, expected.rotation);
    }
    let old_lifetime = rig.actor;
    world.submit(2, spawn(-2)).unwrap();
    let replaced = world.actor_rig(42).unwrap();
    assert_ne!(old_lifetime, replaced.actor);
    assert_eq!(replaced.rest, rest);
    let mut reconnect = stream(entities);
    reconnect.submit(1, spawn(-3)).unwrap();
    assert_ne!(
        old_lifetime.session_id,
        reconnect.actor_rig(42).unwrap().actor.session_id
    );
    assert_eq!(reconnect.actor_rig(42).unwrap().rest, rest);
}

#[test]
fn absent_nonfinite_or_wrong_length_rest_is_nodraw_without_pose_substitution() {
    let (_pack, artwork, entities) = fixture();
    let mut world = stream(entities);
    world.submit(1, spawn(-1)).unwrap();
    world.advance_actor_interpolation_ticks(1);
    let rig = world.actor_rig(42).unwrap();
    let mut nonfinite = rig.rest.to_vec();
    nonfinite[0].translation_scale[0] = f32::NAN;
    // Invalid animated palettes are not consulted by the static route.
    let ignored_animation = ActorRigSnapshot {
        previous: &nonfinite,
        current: &nonfinite,
        ..rig
    };
    assert_eq!(
        entity_rig_presentation(&ignored_animation, world.actor(42).unwrap(), &artwork, 0.5)
            .unwrap()
            .submission
            .route,
        ActorRigRoute::StaticFallback
    );
    for rest in [&[][..], &rig.rest[..1], &nonfinite[..]] {
        let rejected = ActorRigSnapshot { rest, ..rig };
        let draw =
            entity_rig_presentation(&rejected, world.actor(42).unwrap(), &artwork, 0.5).unwrap();
        assert_eq!(draw.submission.route, ActorRigRoute::NoDraw);
        assert!(draw.submission.input.previous_bones.is_empty());
        assert!(draw.submission.input.current_bones.is_empty());
        let mut builder = render::ActorRigFrameBuilder::new([]).unwrap();
        let frame = builder.build(0.5, None, [draw.submission]);
        assert_eq!(frame.rejects.no_draw, 1);
        assert!(frame.instances.is_empty());
    }
}

#[test]
fn static_clock_survives_invalid_first_eval_but_requires_real_tick_after_reset_or_spawn() {
    let (_pack, artwork, entities) =
        weighted_fixture("math.sqrt(query.modified_move_speed - 1)", 1);
    let mut world = stream(entities.clone());
    world.advance_actor_interpolation_ticks(3);
    world.submit(1, spawn(-1)).unwrap();
    let initial = world.actor_rig(42).unwrap();
    assert_eq!(initial.rest_completed_tick, 0);
    assert!(entity_rig_presentation(&initial, world.actor(42).unwrap(), &artwork, 0.5).is_none());
    let animated_generation = initial.reset_generation;
    world.advance_actor_interpolation_ticks(1);
    let observed = world.actor_rig(42).unwrap();
    assert_eq!(observed.completed_tick, 3);
    assert_eq!(observed.rest_completed_tick, 4);
    assert_eq!(observed.reset_generation, animated_generation);
    assert_eq!(world.actor_animation_stats().frozen_actors, 1);
    let first =
        entity_rig_presentation(&observed, world.actor(42).unwrap(), &artwork, 0.5).unwrap();
    assert_eq!(first.submission.route, ActorRigRoute::StaticFallback);
    assert_eq!(first.submission.input.completed_tick, 4);
    let rest = observed.rest.to_vec();
    let static_generation = observed.rest_reset_generation;
    world
        .submit(
            2,
            WorldEvent::Actor(ActorEvent::Move(protocol::ActorMoveEvent {
                dimension: 0,
                runtime_id: 42,
                position: [Some(2.0), None, None],
                position_origin: protocol::ActorPositionOrigin::Feet,
                pitch: None,
                yaw: None,
                head_yaw: None,
                on_ground: None,
                teleported: true,
                player_mode: None,
                source_tick: Some(2),
            })),
        )
        .unwrap();
    let pending = world.actor_rig(42).unwrap();
    assert_eq!(pending.rest_completed_tick, 0);
    assert!(entity_rig_presentation(&pending, world.actor(42).unwrap(), &artwork, 0.5).is_none());
    world.advance_actor_interpolation_ticks(1);
    let reset = world.actor_rig(42).unwrap();
    assert_eq!(reset.rest_completed_tick, 5);
    assert!(reset.rest_reset_generation > static_generation);
    assert_eq!(reset.reset_generation, animated_generation);
    assert_eq!(reset.rest, rest);
    assert_eq!(
        entity_rig_presentation(&reset, world.actor(42).unwrap(), &artwork, 0.5)
            .unwrap()
            .submission
            .route,
        ActorRigRoute::StaticFallback
    );
    let lifetime = reset.actor;
    world.submit(3, spawn(-2)).unwrap();
    let replacement = world.actor_rig(42).unwrap();
    assert_ne!(replacement.actor, lifetime);
    assert_eq!(replacement.rest_completed_tick, 0);
    assert!(
        entity_rig_presentation(&replacement, world.actor(42).unwrap(), &artwork, 0.5).is_none()
    );
    world.advance_actor_interpolation_ticks(1);
    assert!(
        entity_rig_presentation(
            &world.actor_rig(42).unwrap(),
            world.actor(42).unwrap(),
            &artwork,
            0.5
        )
        .is_some()
    );
    let mut reconnect = stream(entities);
    reconnect.submit(1, spawn(-3)).unwrap();
    assert_eq!(reconnect.actor_rig(42).unwrap().rest_completed_tick, 0);
    assert_ne!(
        reconnect.actor_rig(42).unwrap().actor.session_id,
        lifetime.session_id
    );
}

#[test]
fn static_publication_observes_actors_before_actor_and_world_budget_branches() {
    // 127 queries and 126 adds = 253 real operations per weight. Repeated
    // bindings exercise the production budget, not an injected test seam.
    let weight = std::iter::repeat_n("query.modified_move_speed", 127)
        .collect::<Vec<_>>()
        .join("+");
    for (repetitions, actors) in [(17, 1u64), (16, 70u64)] {
        let (_pack, artwork, entities) = weighted_fixture(&weight, repetitions);
        let mut world = stream(entities);
        for index in 0..actors {
            world
                .submit(index + 1, spawn_runtime(42 + index, -(index as i64) - 1))
                .unwrap();
        }
        world.advance_actor_interpolation_ticks(1);
        let stats = world.actor_animation_stats();
        if actors == 1 {
            assert!(stats.actor_budget_exhaustions > 0);
        } else {
            assert!(stats.world_budget_exhaustions > 0);
        }
        let runtime = 42 + actors - 1;
        let rig = world.actor_rig(runtime).unwrap();
        assert_eq!(rig.completed_tick, 0);
        assert_eq!(rig.rest_completed_tick, 1);
        let draw =
            entity_rig_presentation(&rig, world.actor(runtime).unwrap(), &artwork, 0.5).unwrap();
        assert_eq!(draw.submission.route, ActorRigRoute::StaticFallback);
        assert_eq!(draw.submission.input.completed_tick, 1);
        assert_eq!(
            draw.submission.input.previous_bones,
            draw.submission.input.current_bones
        );
    }
}

/// A texture-only reload republishes base artwork; the scene must validate against the same pages
/// presentation selects from, or it rejects the whole actor batch.
#[test]
fn replaced_base_artwork_without_a_session_pack_keeps_actors_drawn() {
    use bevy::{math::Vec3, prelude::World, time::Real};
    let (_pack, artwork, entities) =
        compiled_fixture("1.0", 1, assets::ActorPoseMode::CompiledLiteral);
    let (_replaced_pack, replaced, _) = compiled_fixture("1.0", 1, assets::ActorPoseMode::RestPose);
    assert_ne!(artwork.identity(), replaced.identity());
    let mut stream = stream(Arc::clone(&entities));
    stream.submit(1, spawn_runtime(100, -100)).unwrap();
    let mut scene =
        render::ActorRenderScene::with_runtime_entity_assets_and_equipment(&entities, &[]).unwrap();
    scene.configure_artwork(artwork.clone());
    let mut client_world = crate::runtime::world::ClientWorld::new_with_entity_assets(
        Arc::new(RuntimeAssets::diagnostic()),
        Arc::clone(&entities),
    );
    client_world.stream = Some(stream);
    let mut world = super::actor_frame_allocations::actor_frame_world(
        client_world,
        scene,
        artwork,
        crate::runtime::network::HandRigBuilder::from_runtime_assets(&entities).unwrap(),
        (Vec3::new(0.0, 66.0, -12.0), Vec3::new(0.0, 64.0, 4.0)),
    );
    let mut clock = std::time::Instant::now();
    let mut drawn = |world: &mut World| {
        for _ in 0..4 {
            clock += std::time::Duration::from_millis(50);
            world
                .resource_mut::<bevy::time::Time<Real>>()
                .update_with_instant(clock);
            world
                .run_system_cached(crate::runtime::network::prepare_actor_render_frame)
                .unwrap();
            world
                .run_system_cached(crate::runtime::network::publish_actor_render_frame)
                .unwrap();
        }
        world
            .resource::<render::ActorRenderFrame>()
            .rig
            .instances
            .len()
    };
    assert_eq!(drawn(&mut world), 1);
    world.insert_resource(replaced.clone());
    assert_eq!(
        drawn(&mut world),
        1,
        "the replaced artwork rejected the actor"
    );
    assert_eq!(
        world
            .resource::<render::ActorRenderFrame>()
            .artwork_pages()
            .identity(),
        replaced.identity()
    );
}
