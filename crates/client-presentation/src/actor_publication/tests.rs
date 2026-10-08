use super::EquipmentRuntime;
use bevy::prelude::{PerspectiveProjection, Projection, Transform, Vec3};
use std::sync::Arc;

/// Every actor the renderer can draw is animated: the guard-banded view admits a superset.
#[test]
fn the_animation_view_admits_everything_the_render_cull_draws() {
    let camera =
        Transform::from_xyz(3.0, 70.0, -2.0).looking_at(Vec3::new(20.0, 64.0, 9.0), Vec3::Y);
    let projection = Projection::Perspective(PerspectiveProjection {
        fov: 70f32.to_radians(),
        aspect_ratio: 16.0 / 9.0,
        ..Default::default()
    });
    let view = super::animation_view(&camera, &projection).unwrap();
    let cull = render::ActorCullView {
        clip_from_world: projection.get_clip_from_view() * camera.to_matrix().inverse(),
        camera_position: camera.translation,
        max_distance: render::MAX_ACTOR_RENDER_DISTANCE_BLOCKS,
    };
    let (mut drawn, mut held) = (0, 0);
    for x in (-60..=60).step_by(3) {
        for z in (-60..=60).step_by(3) {
            for (y, scale) in [(60.0, 1.0), (64.0, 0.01), (75.0, 3.0)] {
                let feet = [x as f32, y, z as f32];
                if render::actor_bounds_are_visible(feet, scale, Default::default(), Some(cull)) {
                    drawn += 1;
                    assert!(
                        view.admits(feet, scale, false, Default::default()),
                        "{feet:?} x{scale}"
                    );
                } else if !view.admits(feet, scale, false, Default::default()) {
                    held += 1;
                }
            }
        }
    }
    assert!(drawn > 100 && held > 100, "drawn={drawn} held={held}");
}

use client_world::HandPhase;

// The swing wraps forward from its last tick to rest, and an eat use counts from its first
// using tick only while the rig reports the use.
#[test]
fn hand_progress_interpolates_the_swing_forward_and_counts_the_use() {
    let phase = |attack_time, arm_height, use_ticks| HandPhase {
        attack_time,
        arm_height,
        use_ticks,
    };
    let hand = super::hand_progress([phase(5.0 / 6.0, 0.6, 0), phase(0.0, 1.0, 0)], None, 0.5);
    assert!((hand.swing - 11.0 / 12.0).abs() < 1e-6);
    assert!((hand.equip - 0.8).abs() < 1e-6);
    assert_eq!(hand.consume, None);
    let eating = super::hand_progress([phase(0.0, 1.0, 3), phase(0.0, 1.0, 4)], Some(32), 0.25);
    assert_eq!(eating.consume, Some((3.25, 32.0)));
    let idle = super::hand_progress([phase(0.0, 1.0, 0), phase(0.0, 1.0, 0)], Some(32), 0.25);
    assert_eq!(idle.consume, None);
}

// The pack's first-person arm offset sits behind the model's left side; vanilla's facing puts
// that ahead of the view and to its right.
#[test]
fn first_person_arm_offset_lands_ahead_and_right_of_the_camera() {
    let rows = super::hand_camera_from_rig(
        0.9375,
        crate::local_player::LOCAL_AVATAR_EYE_HEIGHT_BLOCKS,
        bevy::math::Mat4::IDENTITY,
    );
    let arm = [-8.5 / 16.0, 12.0 / 16.0, 12.0 / 16.0];
    let camera: [f32; 3] = std::array::from_fn(|row| {
        (0..3).map(|axis| rows[row][axis] * arm[axis]).sum::<f32>() + rows[row][3]
    });
    assert!(
        camera[0] > 0.0 && camera[1] < 0.0 && camera[2] < 0.0,
        "{camera:?}"
    );
    assert!(
        (rows[1][3] + crate::local_player::LOCAL_AVATAR_EYE_HEIGHT_BLOCKS - 0.9375 / 128.0).abs()
            < 1e-6
    );
}

/// Supplies simulated local state through presentation's borrowed observation interface.
struct JumpPhysics(sim::PlayerState, sim::MovementMode);

impl crate::observations::PhysicsObservation for JumpPhysics {
    fn mode(&self) -> sim::MovementMode {
        self.1
    }

    fn fall_fly_ticks(&self) -> u32 {
        0
    }

    /// Returns the last completed simulation state.
    fn state(&self) -> Option<&sim::PlayerState> {
        Some(&self.0)
    }

    /// This fixture keeps both movement modifiers released.
    fn latest_sneak_sprint(&self) -> Option<(bool, bool)> {
        Some((false, false))
    }

    /// The synthetic floor does not own a streamed collision identity.
    fn last_world_identity(&self) -> Option<&sim::WorldCollisionIdentity> {
        None
    }

    /// This actor-publication fixture does not retain motion ticks.
    fn visit_motion_ticks(
        &self,
        _after: Option<u64>,
        _visit: &mut dyn FnMut(u64, crate::audio::local::MotionSample),
    ) {
    }

    /// The fixture always has a current player state.
    fn is_active(&self) -> bool {
        true
    }
}

/// The local feed carries the current view-bobbing setting into authored hand animation.
#[test]
fn local_feed_reads_view_bobbing_toggle() {
    let physics = JumpPhysics(
        sim::PlayerState::new(sim::Vec3::ZERO),
        sim::MovementMode::Walking,
    );
    for enabled in [false, true] {
        let feed = crate::actor_feed::build_local_player_feed(
            &physics,
            bevy::math::Quat::IDENTITY,
            true,
            enabled,
            [1; 16],
            || {
                protocol::PlayerSkin::Unavailable(
                    protocol::PlayerSkinUnavailable::InvalidDimensions,
                )
            },
            client_world::LocalItemUse::Unpredicted,
        )
        .unwrap();
        assert_eq!(feed.view_bobbing, enabled);
        assert!(feed.first_person);
    }
}

struct JumpFloor;

#[test]
fn local_feed_reads_active_flight_from_the_completed_movement_mode() {
    let mut physics = JumpPhysics(
        sim::PlayerState::new(sim::Vec3::ZERO),
        sim::MovementMode::Walking,
    );
    for mode in [
        sim::MovementMode::Flying,
        sim::MovementMode::Walking,
        sim::MovementMode::Riding,
    ] {
        physics.1 = mode;
        let feed = crate::actor_feed::build_local_player_feed(
            &physics,
            bevy::math::Quat::IDENTITY,
            false,
            true,
            [1; 16],
            || {
                protocol::PlayerSkin::Unavailable(
                    protocol::PlayerSkinUnavailable::InvalidDimensions,
                )
            },
            client_world::LocalItemUse::Unpredicted,
        )
        .unwrap();
        assert_eq!(feed.flying, matches!(mode, sim::MovementMode::Flying));
    }
}

impl sim::CollisionWorld for JumpFloor {
    /// Supplies a stable floor for the full jumping and landing sequence.
    fn collision_boxes(
        &self,
        query: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        let floor = sim::Aabb::new(
            sim::Vec3::new(-64.0, 0.0, -64.0),
            sim::Vec3::new(64.0, 1.0, 64.0),
        );
        Ok(sim::CollisionQuery::synthetic(
            floor
                .intersects(query)
                .then_some(floor)
                .into_iter()
                .collect(),
        ))
    }
}

/// The shared actor timeline is not the local physics interpolation timeline, especially
/// after a reset or a frame that completes several movement ticks. The body must still
/// travel with the camera through the entire jump, retaining its rig's authored axes.
#[test]
fn local_jump_body_tracks_camera_render_sample_in_both_third_person_views() {
    use std::time::Duration;

    use crate::presentation::actors::{local_diagnostic_presentation, select_actor_presentations};
    use semantic_input::PerspectiveMode;

    const LOCAL_ID: u64 = 1;
    const REMOTE_ID: u64 = LOCAL_ID + 1;
    let anchor = [0.0, 1.0 + protocol::PLAYER_NETWORK_OFFSET, 0.0];
    let look = bevy::math::Quat::from_rotation_y(0.4);
    for perspective in [
        PerspectiveMode::ThirdPersonBack,
        PerspectiveMode::ThirdPersonFront,
    ] {
        let mut physics = JumpPhysics(
            sim::PlayerState::new(sim::Vec3::new(0.0, 1.0, 0.0)),
            sim::MovementMode::Walking,
        );
        physics.0.on_ground = true;
        let simulator = sim::Simulator::default();
        let mut physics_clock = crate::actor_clock::ActorFrameClock::default();
        let mut previous_position = physics.0.position;
        let mut stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: LOCAL_ID,
            local_player_unique_id: LOCAL_ID as i64,
            player_position: anchor,
            world_spawn_position: [0, 1, 0],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        });
        let mut actor_clock = crate::actor_clock::ActorFrameClock::default();
        // Physics can reset independently of the actor clock after a correction.
        actor_clock.advance(Duration::from_millis(17));
        let mut relative_origin = None;
        let mut raw_origin_drifted = false;
        let mut catch_up = false;
        let mut rose = false;
        let mut fell = false;
        let mut last_height = 1.0;
        // Interleave tick boundaries, fractional frames and two/three-tick catch-up frames.
        for (index, millis) in [50, 10, 15, 25, 120, 35, 115, 10, 15, 25, 120, 35, 115]
            .into_iter()
            .enumerate()
        {
            let elapsed = Duration::from_millis(millis);
            let frame = physics_clock.advance(elapsed);
            for tick in 0..frame.ticks {
                previous_position = physics.0.position;
                simulator
                    .tick(
                        &mut physics.0,
                        sim::MovementInput {
                            jumping: index == 0,
                            jump_pressed: index == 0 && tick == 0,
                            ..Default::default()
                        },
                        &JumpFloor,
                    )
                    .unwrap();
            }
            catch_up |= frame.ticks > 1;
            let interpolated = previous_position
                + (physics.0.position - previous_position) * f64::from(frame.partial_tick);
            let feet = Vec3::new(
                interpolated.x as f32,
                interpolated.y as f32,
                interpolated.z as f32,
            );
            let eye = feet + Vec3::Y * crate::local_player::LOCAL_AVATAR_EYE_HEIGHT_BLOCKS;
            rose |= feet.y > last_height + 1e-4;
            fell |= feet.y < last_height - 1e-4;
            last_height = feet.y;
            let feed = crate::actor_feed::build_local_player_feed(
                &physics,
                look,
                false,
                true,
                [1; 16],
                || {
                    protocol::PlayerSkin::Unavailable(
                        protocol::PlayerSkinUnavailable::InvalidDimensions,
                    )
                },
                client_world::LocalItemUse::Unpredicted,
            )
            .unwrap();
            stream.sync_local_player_pose(&feed);
            let step = actor_clock.advance(elapsed);
            stream.advance_actor_interpolation_frame(step.ticks);
            let actor_feet = stream
                .authority()
                .actor(LOCAL_ID)
                .unwrap()
                .interpolated_position(step.partial_tick)
                .unwrap();
            let mut local =
                local_diagnostic_presentation(1, 0, LOCAL_ID, 1, actor_feet, 27.0, 13.0).unwrap();
            // Authored nonuniform scale must survive the translation correction.
            for row in &mut local.submission.world_from_actor {
                for (value, scale) in row[..3].iter_mut().zip([0.7, 1.2, 0.9]) {
                    *value *= scale;
                }
            }
            local.authored_scale = 0.7;
            local.head_over_body = 11.0;
            let original = local.clone();
            let camera = crate::camera::perspective_pose(eye, look, perspective);
            let view_from_world = camera.to_matrix().inverse();
            let expected_relative = view_from_world.transform_point3(feet);
            let reference = *relative_origin.get_or_insert(expected_relative);
            assert!(expected_relative.abs_diff_eq(reference, 1e-5));
            raw_origin_drifted |= !view_from_world
                .transform_point3(Vec3::from_array(actor_feet))
                .abs_diff_eq(reference, 1e-3);
            super::place_local_actor_at_render_feet(&mut local, feet);
            let corrected_feet =
                Vec3::from_array(local.submission.world_from_actor.map(|row| row[3]));
            assert!(
                view_from_world
                    .transform_point3(corrected_feet)
                    .abs_diff_eq(reference, 1e-5),
                "{perspective:?} frame {index}"
            );
            let mut expected = original.clone();
            for (row, coordinate) in expected
                .submission
                .world_from_actor
                .iter_mut()
                .zip(feet.to_array())
            {
                row[3] = coordinate;
            }
            assert_eq!(local.submission, expected.submission);
            assert_eq!(local.authored_scale, original.authored_scale);
            assert_eq!(local.world_yaw_degrees, original.world_yaw_degrees);
            assert_eq!(local.head_over_body, original.head_over_body);
            let remote =
                local_diagnostic_presentation(1, 0, REMOTE_ID, 1, [6.0, 3.0, -4.0], 72.0, 0.0)
                    .unwrap();
            let remote_submission = remote.submission.clone();
            let batch = select_actor_presentations(LOCAL_ID, true, Some(local), [remote]);
            let selected_remote = batch
                .submissions
                .iter()
                .find(|body| body.input.identity.runtime_id == REMOTE_ID)
                .unwrap();
            assert_eq!(
                selected_remote.world_from_actor,
                remote_submission.world_from_actor
            );
            assert_eq!(selected_remote.input, remote_submission.input);
        }
        assert!(
            catch_up && rose && fell && raw_origin_drifted,
            "{perspective:?}"
        );
    }
}

/// Original equipment pixels and geometry with a distinct startup glint.
fn equipment_pack_fixture() -> (Arc<assets::SessionEntityPack>, render::ActorArtworkPages) {
    let png = |rgba: [u8; 4], width, height| {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(width, height, image::Rgba(rgba))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    };
    let geometry = assets::ELYTRA_GEOMETRY_IDENTIFIER;
    let files: Vec<(Box<str>, Vec<u8>)> = vec![
        ("attachables/wings.json".into(), serde_json::to_vec(&serde_json::json!({
            "format_version":"1.10.0","minecraft:attachable":{"description":{
                "identifier":"minecraft:elytra","materials":{"default":"elytra","enchanted":"elytra_glint"},
                "textures":{"default":"textures/models/wings"},"geometry":{"default":geometry},
                "render_controllers":["controller.render.wings"]
            }}
        })).unwrap()),
        ("models/entity/wings.json".into(), serde_json::to_vec(&serde_json::json!({
            "format_version":"1.12.0","minecraft:geometry":[{
                "description":{"identifier":geometry,"texture_width":64,"texture_height":32},
                "bones":[{"name":"body","pivot":[0,24,0],
                    "cubes":[{"origin":[-10,0,0],"size":[10,20,2],"uv":[22,0]}]}]
            }]
        })).unwrap()),
        ("render_controllers/wings.json".into(), br#"{"format_version":"1.8.0","render_controllers":{
            "controller.render.wings":{"geometry":"Geometry.default","textures":["Texture.default"],
                "materials":[{"*":"Material.default"}]}
        }}"#.to_vec()),
        ("textures/models/wings.png".into(), png([200, 40, 40, 255], 64, 32)),
        (
            format!("{}.png", assets::ACTOR_GLINT_TEXTURE_IDENTIFIER).into(),
            png([90, 20, 200, 255], 1, 1),
        ),
    ];
    let compiled = pack_compiler::compile_actor_pack(files)
        .unwrap()
        .expect("pack compiles");
    let catalog = assets::RuntimeEquipmentCatalog::from_parts(
        compiled.identity,
        compiled.equipment_bindings,
        compiled.equipment_textures,
    )
    .unwrap();
    let pack = assets::SessionEntityPack {
        assets: Arc::new(assets::RuntimeEntityAssets::from_compiled(compiled.entities).unwrap()),
        textures: Arc::from([]),
        bindings: Arc::from([]),
        equipment: Some(Arc::new(catalog)),
    };
    let startup = render::ActorArtworkPages::default().with_actor_glint(
        render_model::equipment::EquipmentRaster {
            width: 1,
            height: 1,
            rgba8: Arc::from([1, 2, 3, 255]),
        },
    );
    (Arc::new(pack), startup)
}

#[test]
fn worker_prepares_equipment_pages_and_glint_before_publication() {
    let (pack, startup) = equipment_pack_fixture();
    let (base, source) = (startup.clone(), pack.clone());
    let prepared = std::thread::spawn(move || {
        crate::prepared_actor_artwork::PreparedActorArtwork::new(&base, &source)
    })
    .join()
    .unwrap();
    let pages = prepared.pages_for(&startup, &pack).unwrap();
    let catalog = pack.equipment.as_ref().unwrap();
    let (expected, _) = startup
        .clone()
        .with_equipment_rasters(&EquipmentRuntime::pack_rasters(catalog));
    let expected = expected.with_actor_glint(EquipmentRuntime::actor_glint(catalog).unwrap());
    assert_eq!(
        pages.pages().len(),
        expected.pages().len(),
        "worker must prepare equipment pixels too"
    );
    assert_eq!(pages.pages(), expected.pages());
    assert_eq!(
        pages.actor_glint().unwrap().rgba8,
        expected.actor_glint().unwrap().rgba8
    );
    let resources =
        crate::prepared_actor_artwork::session_resources(&startup, Some(&pack), Some(&prepared));
    assert!(matches!(resources, std::borrow::Cow::Borrowed(_)));
    let mesh = &resources.equipment[0];
    let mut scene = render::ActorRenderScene::default();
    let mut ready = super::SessionGeometryReady::default();
    super::apply_session_pack(
        &mut scene,
        &resources,
        Some(&pack),
        None,
        &mut ready,
        None,
        None,
    );
    assert!(ready.entities && ready.equipment);
    assert!(scene.contains_geometry(mesh.id));
    let published = scene
        .frame()
        .rig
        .geometry_vertices
        .segments
        .iter()
        .find(|published| published.as_ref() == mesh.vertices.as_ref())
        .expect("equipment vertices are present in the published catalog");
    assert!(
        resources
            .entities
            .iter()
            .chain(&resources.equipment)
            .any(|worker| Arc::ptr_eq(published, &worker.vertices)),
        "content interning must retain an existing worker allocation"
    );
    for (worker, published) in pages
        .pages()
        .iter()
        .zip(scene.frame().artwork_pages().pages())
    {
        assert!(Arc::ptr_eq(
            &worker.shared_pixels(),
            &published.shared_pixels()
        ));
    }
    let again = prepared.pages_for(&startup, &pack).unwrap();
    for (first, next) in pages.pages().iter().zip(again.pages()) {
        assert!(Arc::ptr_eq(&first.shared_pixels(), &next.shared_pixels()));
    }
}

/// A server pack's glint replaces the startup glint for the session; disconnect restores it.
#[test]
fn session_pack_glint_replaces_the_startup_glint_until_disconnect() {
    let (pack, startup) = equipment_pack_fixture();
    let mut scene = render::ActorRenderScene::default();
    let mut ready = super::SessionGeometryReady::default();
    let glint = |pages: &render::ActorArtworkPages| pages.actor_glint().unwrap().rgba8.to_vec();
    let resources = crate::prepared_actor_artwork::session_resources(&startup, Some(&pack), None);
    let (session, ..) = super::apply_session_pack(
        &mut scene,
        &resources,
        Some(&pack),
        None,
        &mut ready,
        None,
        None,
    );
    assert_eq!(glint(&session.unwrap()), [90, 20, 200, 255]);
    assert_eq!(glint(scene.frame().artwork_pages()), [90, 20, 200, 255]);
    let restored_resources = crate::prepared_actor_artwork::session_resources(&startup, None, None);
    let (restored, ..) = super::apply_session_pack(
        &mut scene,
        &restored_resources,
        None,
        None,
        &mut ready,
        None,
        None,
    );
    assert!(restored.is_none());
    assert_eq!(glint(scene.frame().artwork_pages()), [1, 2, 3, 255]);
}
