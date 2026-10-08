use std::{path::Path, sync::Arc};

use assets::{RuntimeActorCatalog, RuntimeAssets, RuntimeEntityAssets};
use bevy::math::{EulerRot, Quat, Vec3};
use client_world::WorldAuthority;
use protocol::{ActorEvent, ActorKind, ActorSpawnEvent, WorldBootstrap, WorldEvent};
use render::{ActorArtworkPages, ActorRigFrameBuilder, ActorRigRoute};
use render_model::{EntityRigId, entity_geometry};

use crate::presentation::{
    actors::{ActorPresentationBatch, PoseConversions, actor_rig_presentation_cached},
    entity_layers::{LayerPoseCache, apply_render_layers_cached},
};

fn carrier(extension: &str) -> Option<Vec<u8>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/assets/compiled");
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping installed billboard frame fixture: {} is absent",
                root.display()
            );
            return None;
        }
        Err(error) => panic!("read {}: {error}", root.display()),
    };
    let paths = entries
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|value| value == extension))
        .collect::<Vec<_>>();
    if paths.is_empty() {
        eprintln!(
            "skipping installed billboard frame fixture: no {extension} carrier in {}",
            root.display()
        );
        return None;
    }
    assert_eq!(
        paths.len(),
        1,
        "ambiguous installed {extension} carriers: {paths:?}"
    );
    Some(std::fs::read(&paths[0]).unwrap())
}

fn transform(rows: [[f32; 4]; 3], point: Vec3) -> Vec3 {
    Vec3::from_array(std::array::from_fn(|axis| {
        Vec3::from_array([rows[axis][0], rows[axis][1], rows[axis][2]]).dot(point) + rows[axis][3]
    }))
}

#[test]
fn installed_billboards_follow_the_fresh_render_camera_at_extreme_pitch_without_a_tick() {
    assert_billboards_follow_fresh_camera(false);
}

#[test]
fn installed_billboards_follow_fresh_camera_after_the_completed_tick_was_culled() {
    assert_billboards_follow_fresh_camera(true);
}

fn assert_billboards_follow_fresh_camera(cull_completed_tick: bool) {
    let (Some(entity_bytes), Some(actor_bytes)) = (carrier("mcbeent"), carrier("mcbeact")) else {
        return;
    };
    let entities = Arc::new(RuntimeEntityAssets::decode(&entity_bytes).unwrap());
    let artwork = RuntimeActorCatalog::decode(&actor_bytes, &entities).unwrap();
    let pages = ActorArtworkPages::new(&artwork);
    for identifier in [
        "minecraft:xp_orb",
        "minecraft:snowball",
        "minecraft:ender_pearl",
    ] {
        let mut world = WorldAuthority::new(
            WorldBootstrap {
                dimension: 0,
                local_player_runtime_id: 999,
                local_player_unique_id: 999,
                player_position: [0.0; 3],
                world_spawn_position: [0; 3],
                air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
                block_network_ids_are_hashes: false,
            },
            Arc::new(RuntimeAssets::diagnostic()),
            Some(Arc::clone(&entities)),
            [0.0; 3],
            None,
        );
        world
            .apply_ordered_event(
                WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                    dimension: 0,
                    unique_id: 1,
                    runtime_id: 1,
                    kind: ActorKind::Entity {
                        identifier: identifier.into(),
                    },
                    position: [0.0; 3],
                    velocity: [0.0, 1.0, 0.0],
                    pitch: -90.0,
                    yaw: 27.0,
                    head_yaw: 27.0,
                    body_yaw: 27.0,
                    held_item: Default::default(),
                    metadata: Arc::from([]),
                    attributes: Arc::from([]),
                    properties: Arc::from([]),
                    links: Arc::from([]),
                })),
                Some(1),
            )
            .unwrap();
        world.set_actor_camera_rotation(super::actor_camera_rotation(Quat::IDENTITY));
        world.advance_actor_interpolation_ticks(2);
        if cull_completed_tick {
            let view = client_world::ActorAnimationView {
                planes: [[0.0, 0.0, 0.0, -1.0]; 6],
                camera: [0.0; 3],
                player_distance: 100.0,
                entity_radius: 100.0,
            };
            world.set_actor_animation_view(Some(view));
            world.advance_actor_interpolation_ticks(1);
            world.set_actor_animation_view(Some(client_world::ActorAnimationView {
                planes: [[0.0, 0.0, 0.0, 1.0]; 6],
                ..view
            }));
        }
        let completed_tick = world.actor_rig(1).unwrap().completed_tick;
        let actor_before = world.actor(1).unwrap().clone();
        for (yaw, pitch) in [
            (0.7, 89.9_f32),
            (-1.4, -89.9_f32),
            (1.9, 90.0_f32),
            (-2.0, -90.0_f32),
        ] {
            let camera = Quat::from_euler(EulerRot::YXZ, yaw, pitch.to_radians(), 0.0);
            world.set_actor_camera_rotation(super::actor_camera_rotation(camera));
            world.advance_actor_interpolation_frame(0);
            let rig = world.actor_rig(1).unwrap();
            let actor = world.actor(1).unwrap();
            let (source, index) = rig.geometry_source().unwrap();
            let geometry = entity_geometry(source, index, EntityRigId(rig.rig.0)).unwrap();
            let mut poses = PoseConversions::default();
            poses.begin_frame();
            let mut presentation =
                actor_rig_presentation_cached(&rig, actor, None, 0.25, &mut poses).unwrap();
            presentation.submission.route = ActorRigRoute::Compiled;
            let location = pages
                .variant_location(presentation.submission.input.rig, rig.render[0].source)
                .expect("installed billboard artwork");
            let identity = presentation.submission.input.identity;
            let mut batch = ActorPresentationBatch {
                submissions: vec![presentation.submission],
                skin_layers: Vec::new(),
                artwork: std::collections::HashMap::from([(identity, location)]),
            };
            let layers = world.actor_render_frame(0.25).layers(1).unwrap();
            apply_render_layers_cached(
                &mut batch,
                |_| Some(layers.clone()),
                |_, _| None,
                &pages,
                &mut LayerPoseCache::default(),
            );
            let frame = ActorRigFrameBuilder::new([geometry]).unwrap().build_paged(
                0.25,
                None,
                batch.submissions,
                |identity| batch.artwork[identity].page(),
            );
            assert_eq!(frame.instances.len(), 1, "{identifier} must stay admitted");
            let instance = frame.instances[0];
            let vertices = frame
                .geometry_vertices
                .span(frame.geometry_spans[instance.geometry_id as usize])
                .unwrap();
            let points: [Vec3; 3] = std::array::from_fn(|index| {
                let vertex = vertices[index];
                let bone = frame.current_bones
                    [instance.current_bone_base as usize + vertex.bone_index as usize];
                transform(
                    instance.world_from_actor,
                    transform(bone, Vec3::from_array(vertex.position)),
                )
            });
            let normal = (points[1] - points[0])
                .cross(points[2] - points[0])
                .normalize();
            let toward_camera = camera * Vec3::Z;
            assert!(
                normal.is_finite() && normal.dot(toward_camera).abs() > 0.999,
                "{identifier} must follow the render camera, rather than its vertical launch or stale tick pose: pitch {pitch}, normal {normal:?}, camera {toward_camera:?}"
            );
            assert_eq!(world.actor_rig(1).unwrap().completed_tick, completed_tick);
            assert_eq!(world.actor(1).unwrap(), &actor_before);
        }
    }
}

#[test]
fn sampled_authored_rig_scales_reach_layer_placement_including_zero_axes() {
    let (Some(entity_bytes), Some(actor_bytes)) = (carrier("mcbeent"), carrier("mcbeact")) else {
        return;
    };
    let entities = Arc::new(RuntimeEntityAssets::decode(&entity_bytes).unwrap());
    let artwork = RuntimeActorCatalog::decode(&actor_bytes, &entities).unwrap();
    let pages = ActorArtworkPages::new(&artwork);
    let feet = [3.0, 64.0, 4.0];
    let mut world = WorldAuthority::new(
        WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 999,
            local_player_unique_id: 999,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        Some(entities),
        [0.0; 3],
        None,
    );
    world
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: 1,
                runtime_id: 1,
                kind: ActorKind::Entity {
                    identifier: "minecraft:creeper".into(),
                },
                position: feet,
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
            Some(1),
        )
        .unwrap();
    world.advance_actor_interpolation_ticks(2);
    let rig = world.actor_rig(1).unwrap();
    let actor = world.actor(1).unwrap();
    for scale in [[2.0, 3.0, 4.0, 5.0], [2.0, 3.0, 0.0, 5.0]] {
        let presentation = crate::presentation::actors::entity_rig_presentation_cached(
            &rig, actor, &pages, 0.5, None,
        )
        .unwrap();
        let identity = presentation.submission.input.identity;
        let location = presentation.artwork.unwrap();
        let mut batch = ActorPresentationBatch {
            submissions: vec![presentation.submission],
            skin_layers: Vec::new(),
            artwork: std::collections::HashMap::from([(identity, location)]),
        };
        let mut layers = rig.render.to_vec();
        for layer in &mut layers {
            layer.sampled_scale = Some(scale);
        }
        apply_render_layers_cached(
            &mut batch,
            |_| Some(std::borrow::Cow::Borrowed(&layers)),
            |_, sampled| {
                crate::presentation::actors::sampled_rig_placement(&rig, actor, 0.5, sampled)
                    .map(|(rows, _)| rows)
            },
            &pages,
            &mut LayerPoseCache::default(),
        );
        assert!(!batch.submissions.is_empty());
        for submission in &batch.submissions {
            for axis in 0..3 {
                let row = submission.world_from_actor[axis];
                assert_eq!(row[3], feet[axis]);
                for (column, value) in row[..3].iter().enumerate() {
                    let expected = if column != axis {
                        0.0
                    } else {
                        scale[0] * scale[axis + 1] * if axis == 1 { 1.0 } else { -1.0 }
                    };
                    assert!(
                        (*value - expected).abs() < 1e-5,
                        "sampled global and axis scale reach actual layer placement: {row:?}"
                    );
                }
            }
        }
    }
}
