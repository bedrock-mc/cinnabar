use std::{path::Path, sync::Arc};

use assets::{RuntimeAssets, RuntimeEntityAssets};
use bevy::math::{EulerRot, Quat, Vec3};
use client_world::WorldAuthority;
use protocol::{ActorEvent, ActorKind, ActorSpawnEvent, WorldBootstrap, WorldEvent};
use render::{ActorRigFrameBuilder, ActorRigRoute};
use render_model::{EntityRigId, entity_geometry};

use crate::presentation::actors::{PoseConversions, actor_rig_presentation_cached};

fn installed_entities() -> Option<Arc<RuntimeEntityAssets>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/assets/compiled");
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping installed XP orb fixture: {} is absent",
                root.display()
            );
            return None;
        }
        Err(error) => panic!("read {}: {error}", root.display()),
    };
    let paths = entries
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "mcbeent")
        })
        .collect::<Vec<_>>();
    if paths.is_empty() {
        eprintln!(
            "skipping installed XP orb fixture: no entity carrier in {}",
            root.display()
        );
        return None;
    }
    assert_eq!(
        paths.len(),
        1,
        "ambiguous installed entity carriers: {paths:?}"
    );
    Some(Arc::new(
        RuntimeEntityAssets::decode(&std::fs::read(&paths[0]).unwrap()).unwrap(),
    ))
}

fn transform(rows: [[f32; 4]; 3], point: Vec3) -> Vec3 {
    Vec3::from_array(std::array::from_fn(|axis| {
        Vec3::from_array([rows[axis][0], rows[axis][1], rows[axis][2]]).dot(point) + rows[axis][3]
    }))
}

fn installed_orb_world() -> Option<WorldAuthority> {
    let entities = installed_entities()?;
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
                    identifier: "minecraft:xp_orb".into(),
                },
                position: [0.0; 3],
                velocity: [0.0; 3],
                pitch: 41.0,
                yaw: 123.0,
                head_yaw: 123.0,
                body_yaw: 123.0,
                held_item: Default::default(),
                metadata: Arc::from([]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
            Some(1),
        )
        .unwrap();
    Some(world)
}

#[test]
fn installed_xp_orb_textured_face_points_toward_the_live_camera_at_every_heading() {
    let Some(mut world) = installed_orb_world() else {
        return;
    };
    for (yaw, pitch) in [
        (0.0, 0.0),
        (0.7, 0.3),
        (-1.4, -0.5),
        (std::f32::consts::PI, 0.0),
    ] {
        let camera = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
        world.set_actor_camera_rotation(super::actor_camera_rotation(camera));
        world.advance_actor_interpolation_ticks(2);
        let rig = world.actor_rig(1).expect("installed XP billboard rig");
        let actor = world.actor(1).unwrap();
        let (source, index) = rig.geometry_source().unwrap();
        let geometry = entity_geometry(source, index, EntityRigId(rig.rig.0)).unwrap();
        let mut poses = PoseConversions::default();
        poses.begin_frame();
        let mut presentation =
            actor_rig_presentation_cached(&rig, actor, None, 1.0, &mut poses).unwrap();
        presentation.submission.route = ActorRigRoute::Compiled;
        presentation.submission.texture_layer = 0;
        let frame = ActorRigFrameBuilder::new([geometry]).unwrap().build_paged(
            1.0,
            None,
            [presentation.submission],
            |_| 1,
        );
        assert_eq!(frame.instances.len(), 1);
        let instance = frame.instances[0];
        let vertices = frame
            .geometry_vertices
            .span(frame.geometry_spans[instance.geometry_id as usize])
            .unwrap();
        assert_eq!(vertices.len(), 6, "the authored orb has one physical plane");
        assert!(
            vertices.iter().all(|vertex| vertex.back_uv == vertex.uv),
            "a nocull authored quad uses its own texture from either view"
        );
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
            normal.dot(toward_camera) > 0.999,
            "textured XP face points away from the camera: yaw {yaw}, pitch {pitch}, normal {normal:?}, camera {toward_camera:?}"
        );
    }
}

#[test]
fn installed_xp_orb_publishes_each_server_timed_tick_and_partial_frame() {
    let Some(mut world) = installed_orb_world() else {
        return;
    };
    world
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Move(protocol::ActorMoveEvent {
                dimension: 0,
                runtime_id: 1,
                position: [Some(10.0), None, None],
                position_origin: protocol::ActorPositionOrigin::NetworkOffset,
                pitch: None,
                yaw: None,
                head_yaw: None,
                on_ground: Some(false),
                teleported: false,
                player_mode: None,
                source_tick: None,
                interpolation: protocol::ActorInterpolation {
                    ticks: 10,
                    force_completion: false,
                },
            })),
            Some(2),
        )
        .unwrap();
    for tick in 1..=10 {
        let camera = Quat::from_euler(EulerRot::YXZ, tick as f32 * 0.1, -0.5, 0.0);
        world.set_actor_camera_rotation(super::actor_camera_rotation(camera));
        world.advance_actor_interpolation_ticks(1);
        let actor = world.actor(1).unwrap();
        assert!((actor.position[0] - tick as f32).abs() < 0.000_01);
        let rig = world
            .actor_rig(1)
            .expect("installed moving XP billboard rig");
        let mut poses = PoseConversions::default();
        poses.begin_frame();
        for alpha in [0.0, 0.5, 1.0] {
            let presentation =
                actor_rig_presentation_cached(&rig, actor, None, alpha, &mut poses).unwrap();
            let x = presentation.submission.world_from_actor[0][3];
            assert!(
                (x - (tick as f32 - 1.0 + alpha)).abs() < 0.000_01,
                "completed tick {tick}, partial frame {alpha}: rendered X {x}"
            );
        }
    }
}
