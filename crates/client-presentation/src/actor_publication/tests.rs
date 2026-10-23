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
    let view = super::views::animation_view(&camera, &projection).unwrap();
    let cull = render_model::ActorCullView {
        clip_from_world: projection.get_clip_from_view() * camera.to_matrix().inverse(),
        camera_position: camera.translation,
        max_distance: render_model::MAX_ACTOR_RENDER_DISTANCE_BLOCKS,
    };
    let (mut drawn, mut held) = (0, 0);
    for x in (-60..=60).step_by(3) {
        for z in (-60..=60).step_by(3) {
            for (y, scale) in [(60.0, 1.0), (64.0, 0.01), (75.0, 3.0)] {
                let feet = [x as f32, y, z as f32];
                if render_model::actor_bounds_are_visible(
                    feet,
                    scale,
                    Default::default(),
                    Some(cull),
                ) {
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

use actor_animation::HandPhase;

// The swing wraps forward from its last tick to rest, and an eat use counts from its first
// using tick only while the rig reports the use.
#[test]
fn hand_progress_interpolates_the_swing_forward_and_counts_the_use() {
    let phase = |attack_time, arm_height, use_ticks| HandPhase {
        attack_time,
        arm_height,
        use_ticks,
    };
    let hand =
        super::hand::hand_progress([phase(5.0 / 6.0, 0.6, 0), phase(0.0, 1.0, 0)], None, 0.5);
    assert!((hand.swing - 11.0 / 12.0).abs() < 1e-6);
    assert!((hand.equip - 0.8).abs() < 1e-6);
    assert_eq!(hand.consume, None);
    let eating =
        super::hand::hand_progress([phase(0.0, 1.0, 3), phase(0.0, 1.0, 4)], Some(32), 0.25);
    assert_eq!(eating.consume, Some((3.25, 32.0)));
    let idle = super::hand::hand_progress([phase(0.0, 1.0, 0), phase(0.0, 1.0, 0)], Some(32), 0.25);
    assert_eq!(idle.consume, None);
}

#[test]
fn the_first_person_eat_position_moves_forward_every_frame_across_its_ticks() {
    let at = |elapsed, frame_alpha| actor_animation::AttachableAnimationInput {
        use_elapsed_ticks: Some(elapsed),
        frame_alpha,
        ..Default::default()
    };
    let samples = [at(4, 0.25), at(4, 0.9), at(5, 0.1), at(5, 0.6)].map(|input| {
        super::hand::local_consume(Some(&input), Some(32))
            .unwrap()
            .0
    });
    assert!(
        samples.windows(2).all(|pair| pair[0] < pair[1]),
        "{samples:?}"
    );
    assert_eq!(
        super::hand::local_consume(Some(&at(40, 0.5)), Some(32)),
        Some((32.0, 32.0))
    );
}

// The pack's first-person arm offset sits behind the model's left side; vanilla's facing puts
// that ahead of the view and to its right.
#[test]
fn first_person_arm_offset_lands_ahead_and_right_of_the_camera() {
    let rows = super::hand::hand_camera_from_rig(0.9375, bevy::math::Mat4::IDENTITY);
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

#[test]
fn local_equipment_count_reads_raw_armor_container_occupancy() {
    use protocol::{ContainerIdentity, InventoryContentEvent, InventoryEvent, NetworkItemStack};
    let physics = JumpPhysics(
        sim::PlayerState::new(sim::Vec3::ZERO),
        sim::MovementMode::Walking,
    );
    let mut player = player_state::PlayerState::new(1);
    let settings = crate::camera::CameraSettingsAuthority::default();
    let view = crate::local_player::LocalViewPose::default();
    let mut blocking = false;
    for count in [0, 1, protocol::ARMOR_SLOTS, 0] {
        let slots = (0..protocol::ARMOR_SLOTS)
            .map(|slot| {
                if slot < count {
                    NetworkItemStack {
                        network_id: 12_123,
                        count: 1,
                        ..Default::default()
                    }
                } else {
                    NetworkItemStack::empty()
                }
            })
            .collect::<Vec<_>>();
        player
            .inventory
            .ledger_mut()
            .apply(&InventoryEvent::Content(InventoryContentEvent {
                container: ContainerIdentity::window(protocol::ARMOR_WINDOW_ID),
                slots: slots.into(),
                storage_item: NetworkItemStack::empty(),
            }));
        let feed = crate::actor_feed::prepare_local_player_feed(
            &physics,
            &view,
            &settings,
            &Default::default(),
            &player,
            false,
            || client_world::LocalItemUse::Unpredicted,
            &mut blocking,
            [1; 16],
            || {
                protocol::PlayerSkin::Unavailable(
                    protocol::PlayerSkinUnavailable::InvalidDimensions,
                )
            },
            false,
            (
                actor_animation::ACTOR_SWING_TICKS,
                actor_animation::ACTOR_SWING_TICKS,
            ),
        )
        .unwrap();
        assert_eq!(
            feed.equipment_count, count,
            "unresolved local armor must count without a drawable definition"
        );
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
fn local_jump_body_and_flames_share_physical_feet_in_both_third_person_views() {
    use std::time::Duration;

    use crate::presentation::actors::{local_diagnostic_presentation, select_actor_presentations};
    use semantic_input::PerspectiveMode;

    const LOCAL_ID: u64 = 1;
    const REMOTE_ID: u64 = LOCAL_ID + 1;
    let anchor = [0.0, 1.0 + protocol::PLAYER_NETWORK_OFFSET, 0.0];
    let look = bevy::math::Quat::from_rotation_y(0.4);
    for (perspective, java) in [
        PerspectiveMode::ThirdPersonBack,
        PerspectiveMode::ThirdPersonFront,
    ]
    .into_iter()
    .flat_map(|perspective| [false, true].map(|java| (perspective, java)))
    {
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
        let (mut flames, _) = super::flames::WorldActorFlames::prepare(
            render_model::ActorArtworkPages::default(),
            Some(&super::flames::tests::texture()),
        );
        let mut avatar = crate::local_player::LocalAvatarPresentation::default();
        avatar.begin_session(
            simulation_clock::identity::SessionGeneration::new(1),
            LOCAL_ID,
        );
        let mut visibility = crate::local_player::LocalAvatarVisibilityCarrier::default();
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
            if index == 5 {
                physics_clock.reset();
            }
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
            stream
                .submit(
                    index as u64 + 1,
                    protocol::WorldEvent::Actor(protocol::ActorEvent::Metadata(
                        protocol::ActorMetadataUpdateEvent {
                            dimension: 0,
                            runtime_id: LOCAL_ID,
                            metadata: Arc::from([protocol::ActorMetadata {
                                key: protocol_ids::ActorDataKey::Flags.wire_value(),
                                value: protocol::ActorMetadataValue::Flags(
                                    1 << protocol_ids::ActorFlag::OnFire.wire_value(),
                                ),
                            }]),
                            properties: Arc::from([]),
                            tick: index as u64 + 1,
                        },
                    )),
                )
                .unwrap();
            let step = actor_clock.advance(elapsed);
            stream.advance_actor_interpolation_frame(step.ticks);
            let actor_feet = stream
                .authority()
                .actor(LOCAL_ID)
                .unwrap()
                .interpolated_position(step.partial_tick)
                .unwrap();
            let mut local = local_diagnostic_presentation(
                stream.authority().actor_session_id(),
                0,
                LOCAL_ID,
                render_api::PoseGeneration::new(1),
                actor_feet,
                27.0,
                13.0,
            )
            .unwrap();
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
            super::bodies::place_local_actor_at_render_feet(&mut local, feet);
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
            let remote = local_diagnostic_presentation(
                1,
                0,
                REMOTE_ID,
                render_api::PoseGeneration::new(1),
                [6.0, 3.0, -4.0],
                72.0,
                0.0,
            )
            .unwrap();
            let remote_submission = remote.submission.clone();
            avatar.publish_view_visibility(perspective, eye, feet, look, &mut visibility);
            let local_feet = super::flames::LocalFlameFeet::capture(
                Some(&local),
                visibility.snapshot().copied(),
            );
            if java {
                super::java::lift(&mut local.submission.world_from_actor, false, true);
                assert!((local.submission.world_from_actor[1][3] - feet.y).abs() > 1e-4);
            }
            let mut batch = select_actor_presentations(LOCAL_ID, true, Some(local), [remote]);
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
            let (yaw, _, _) = camera.rotation.to_euler(bevy::math::EulerRot::YXZ);
            flames.append(
                Some(stream.authority()),
                yaw,
                step.partial_tick,
                local_feet,
                &mut batch,
            );
            assert_eq!(
                batch.submissions.len(),
                3,
                "burning local body adds one flame layer"
            );
            let flame = batch.submissions.last().unwrap();
            assert_eq!(flame.input.identity.runtime_id, LOCAL_ID);
            let (low, high) = stream
                .authority()
                .actor(LOCAL_ID)
                .unwrap()
                .bounding_box_at([0.0; 3])
                .unwrap();
            let (width, height) = (high[0] - low[0], high[1] - low[1]);
            let depth = render_model::actor::flame::flame_depth(width, height)
                * width
                * render_model::actor::flame::FLAME_SCALE;
            let (sin, cos) = (yaw + std::f32::consts::PI).sin_cos();
            let flame_feet = Vec3::from_array(flame.world_from_actor.map(|row| row[3]))
                - Vec3::new(sin * depth, 0.0, cos * depth);
            assert!(
                flame_feet.abs_diff_eq(feet, 1e-5),
                "{perspective:?}, Java {java}, frame {index}"
            );
            assert!(
                view_from_world
                    .transform_point3(flame_feet)
                    .abs_diff_eq(reference, 1e-5)
            );
        }
        assert!(
            catch_up && rose && fell && raw_origin_drifted,
            "{perspective:?}"
        );
    }
}

/// Original equipment pixels and geometry with a distinct startup glint.
fn equipment_pack_fixture() -> (
    Arc<assets::SessionEntityPack>,
    render_model::ActorArtworkPages,
) {
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
    let compiled = pack_entity_compiler::compile_actor_pack(files)
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
    let startup = render_model::ActorArtworkPages::default().with_actor_glint(
        render_model::EquipmentRaster {
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
    let mut scene = render_scene::ActorRenderScene::default();
    let mut ready = super::SessionGeometryReady::default();
    super::apply_session_pack(
        &mut scene,
        &resources,
        Some(&pack),
        super::SessionIconInput::Staged(None),
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
    let mut scene = render_scene::ActorRenderScene::default();
    let mut ready = super::SessionGeometryReady::default();
    let glint =
        |pages: &render_model::ActorArtworkPages| pages.actor_glint().unwrap().rgba8.to_vec();
    let resources = crate::prepared_actor_artwork::session_resources(&startup, Some(&pack), None);
    let (session, ..) = super::apply_session_pack(
        &mut scene,
        &resources,
        Some(&pack),
        super::SessionIconInput::Staged(None),
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
        super::SessionIconInput::Staged(None),
        &mut ready,
        None,
        None,
    );
    assert!(restored.is_none());
    assert_eq!(glint(scene.frame().artwork_pages()), [1, 2, 3, 255]);
}

/// An active HitColor module cannot overwrite the Reach overlay on an undamaged target.
#[test]
fn reach_and_hurt_colors_follow_actual_damage_independently() {
    let style = super::ActorPresentationStyle {
        hurt_color: Some([1., 0., 0., 0.4]),
        hurt_armor_color: Some([0.5, 0., 0., 0.4]),
        attackable_color: Some([0., 1., 0., 0.3]),
        attackable_actor: Some(7),
    };
    assert_eq!(
        super::layers::cosmetic_overlay(&style, false, false, true, true),
        style.attackable_color
    );
    assert_eq!(
        super::layers::cosmetic_overlay(&style, true, false, true, true),
        style.hurt_color
    );
    assert_eq!(
        super::layers::cosmetic_overlay(&style, true, true, true, true),
        style.hurt_armor_color
    );
    assert_eq!(
        super::layers::cosmetic_overlay(&style, false, false, true, false),
        None
    );
}

/// Final styling preserves submission order and pose storage without allocating.
#[test]
fn styling_reuses_layer_storage_and_distinguishes_damage_from_reach() {
    use crate::presentation::actors::{local_diagnostic_presentation, select_actor_presentations};
    use protocol::{ActorEvent, ActorStatusEvent, ActorStatusKind, WorldEvent};

    let mut stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    let physics = JumpPhysics(
        sim::PlayerState::new(sim::Vec3::ZERO),
        sim::MovementMode::Walking,
    );
    let feed = crate::actor_feed::build_local_player_feed(
        &physics,
        bevy::math::Quat::IDENTITY,
        false,
        true,
        [1; 16],
        || protocol::PlayerSkin::Unavailable(protocol::PlayerSkinUnavailable::InvalidDimensions),
        client_world::LocalItemUse::Unpredicted,
    )
    .unwrap();
    stream.sync_local_player_pose(&feed);
    let local = local_diagnostic_presentation(
        1,
        0,
        1,
        render_api::PoseGeneration::new(1),
        [0.0; 3],
        0.0,
        0.0,
    )
    .unwrap();
    let mut batch = select_actor_presentations(1, true, Some(local), []);
    let mut armor = batch.submissions[0].clone();
    armor.input.identity.layer = crate::presentation::equipment::ELYTRA_LAYER;
    let mut cape = batch.submissions[0].clone();
    cape.input.identity.layer = crate::presentation::cape::ACTOR_LAYER_CAPE;
    batch.submissions.extend([armor, cape]);
    let style = super::ActorPresentationStyle {
        hurt_color: Some([1.0, 0.0, 0.0, 0.5]),
        hurt_armor_color: Some([0.0, 0.0, 1.0, 0.5]),
        attackable_actor: Some(1),
        attackable_color: Some([0.0, 1.0, 0.0, 0.5]),
    };
    let original = batch.submissions.clone();
    let (storage, capacity) = (batch.submissions.as_ptr(), batch.submissions.capacity());
    for hurt in [false, true] {
        batch.submissions.clone_from(&original);
        if hurt {
            stream
                .submit(
                    1,
                    WorldEvent::Actor(ActorEvent::Status(ActorStatusEvent {
                        runtime_id: 1,
                        kind: ActorStatusKind::Hurt,
                        data: 0,
                    })),
                )
                .unwrap();
            assert!(stream.authority().actor(1).unwrap().hurt_overlay_active());
            batch.submissions[0].overlay_rgba8 =
                render_model::pack_overlay_rgba8([1.0, 0.0, 0.0, 0.5]);
        }
        let (_, allocations) = test_alloc::count(|| {
            super::layers::apply_style_and_visibility(&mut batch, Some(&stream), Some(&style))
        });
        assert_eq!(allocations, 0);
        assert_eq!(
            (batch.submissions.as_ptr(), batch.submissions.capacity()),
            (storage, capacity)
        );
        for (index, submission) in batch.submissions.iter().enumerate() {
            let color = if hurt {
                [style.hurt_color, style.hurt_armor_color, None][index]
            } else {
                style.attackable_color
            };
            let mut expected = original[index].clone();
            expected.overlay_rgba8 = color.map_or(0, render_model::pack_overlay_rgba8);
            assert_eq!(submission, &expected);
            assert!(Arc::ptr_eq(
                &submission.input.current_bones,
                &original[index].input.current_bones
            ));
            assert!(Arc::ptr_eq(
                &submission.input.previous_bones,
                &original[index].input.previous_bones
            ));
        }
        for (kind, disable_overlay, allows_overlay) in [
            (assets::EntityRenderMaterial::DissolveDepth, false, false),
            (assets::EntityRenderMaterial::DissolveColor, false, false),
            (assets::EntityRenderMaterial::Default, true, false),
            (assets::EntityRenderMaterial::Default, false, true),
        ] {
            batch.submissions.clone_from(&original);
            let armor = &mut batch.submissions[1];
            armor.material.kind = kind;
            armor.material.state = Some(assets::EntityRenderMaterialState {
                disable_overlay,
                ..Default::default()
            });
            armor.material.dissolve_multiplier = 0.4;
            let material = armor.material;
            let tint = armor.tint;
            super::layers::apply_style_and_visibility(&mut batch, Some(&stream), Some(&style));
            let armor = &batch.submissions[1];
            assert_eq!(
                armor.overlay_rgba8,
                if allows_overlay {
                    render_model::pack_overlay_rgba8(if hurt {
                        style.hurt_armor_color.unwrap()
                    } else {
                        style.attackable_color.unwrap()
                    })
                } else {
                    0
                }
            );
            assert_eq!(armor.material, material);
            assert_eq!(armor.tint, tint);
            assert!(Arc::ptr_eq(
                &armor.input.current_bones,
                &original[1].input.current_bones
            ));
        }
    }
}

/// A server pack that replaces only the player model still carries held items and armour.
#[test]
fn player_model_from_an_attachable_free_pack_keeps_held_items() {
    let png = {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(64, 64, image::Rgba([90, 120, 200, 255]))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    };
    let entity = |identifier: &str, geometry: &str, item_bone: bool| -> Vec<(Box<str>, Vec<u8>)> {
        let item_bone = if item_bone {
            r#",{"name":"rightItem","parent":"rightArm","pivot":[-6,15,1]}"#
        } else {
            ""
        };
        vec![
            (
                format!("entity/{geometry}.json").into(),
                format!(
                    r#"{{"format_version":"1.10.0","minecraft:client_entity":{{"description":{{"identifier":"{identifier}","materials":{{"default":"entity"}},"textures":{{"default":"textures/entity/{geometry}"}},"geometry":{{"default":"geometry.{geometry}"}},"render_controllers":["controller.render.{geometry}"]}}}}}}"#
                )
                .into_bytes(),
            ),
            (
                format!("models/entity/{geometry}.geo.json").into(),
                format!(
                    r#"{{"format_version":"1.12.0","minecraft:geometry":[{{"description":{{"identifier":"geometry.{geometry}","texture_width":64,"texture_height":64}},"bones":[{{"name":"body","pivot":[0,24,0],"cubes":[{{"origin":[-4,12,-2],"size":[8,12,4],"uv":[16,16]}}]}},{{"name":"rightArm","parent":"body","pivot":[-5,22,0],"cubes":[{{"origin":[-8,12,-2],"size":[4,12,4],"uv":[40,16]}}]}}{item_bone}]}}]}}"#
                )
                .into_bytes(),
            ),
            (
                format!("render_controllers/{geometry}.json").into(),
                format!(
                    r#"{{"format_version":"1.8.0","render_controllers":{{"controller.render.{geometry}":{{"geometry":"Geometry.default","materials":[{{"*":"Material.default"}}],"textures":["Texture.default"]}}}}}}"#
                )
                .into_bytes(),
            ),
            (format!("textures/entity/{geometry}.png").into(), png.clone()),
        ]
    };
    let compile = |files| {
        pack_entity_compiler::compile_actor_pack(files)
            .unwrap()
            .expect("pack compiles")
    };
    // Startup entities without the pack's bones, so only the pack's model can carry the item.
    let startup = Arc::new(
        assets::RuntimeEntityAssets::from_compiled(
            compile(entity("test:startup", "startup", false)).entities,
        )
        .unwrap(),
    );
    let compiled = compile(entity("minecraft:player", "pack_player", true));
    assert!(compiled.equipment_bindings.is_empty());
    let pack = Arc::new(assets::SessionEntityPack {
        assets: Arc::new(assets::RuntimeEntityAssets::from_compiled(compiled.entities).unwrap()),
        textures: compiled.textures.into(),
        bindings: compiled.bindings.into(),
        equipment: None,
    });
    let sprite = assets::IconSprite {
        width: 16,
        height: 16,
        rgba8: vec![255; 16 * 16 * 4].into(),
    };
    let icons = assets::RuntimeIconCatalog::decode(
        &assets::encode_icon_catalog(
            [1; 32],
            &[sprite],
            &[assets::IconEntry {
                identifier: "minecraft:apple".into(),
                metadata: 0,
                sprite: 0,
            }],
        )
        .unwrap(),
    )
    .unwrap();
    let (mut equipment, startup_pages, _) = EquipmentRuntime::build(
        startup,
        None,
        Arc::new(icons),
        None,
        None,
        render_model::ActorArtworkPages::default(),
    );
    let resources =
        crate::prepared_actor_artwork::session_resources(&startup_pages, Some(&pack), None);
    let mut scene = render_scene::ActorRenderScene::default();
    let mut ready = super::SessionGeometryReady::default();
    super::apply_session_pack(
        &mut scene,
        &resources,
        Some(&pack),
        super::SessionIconInput::Staged(None),
        &mut ready,
        Some(&mut equipment),
        None,
    );
    let binding = 0;
    let geometry = pack.assets.rig_geometries()[binding].geometry;
    let bones = render_model::geometry_bone_names(&pack.assets, geometry as usize).unwrap();
    let pose: Arc<[render_model::RenderBoneTransform]> = bones
        .iter()
        .map(|_| render_model::RenderBoneTransform {
            rotation: [0.0, 0.0, 0.0, 1.0],
            translation_scale: [0.0, 0.0, 0.0, 1.0],
            axis_scale: render_model::UNIT_AXIS_SCALE,
        })
        .collect();
    let body = render_model::ActorRigSubmission {
        material: Default::default(),
        culling_bounds: Default::default(),
        input: render_model::ActorRigRenderInput {
            identity: render_model::ActorRenderIdentity {
                session_id: 1,
                dimension: 0,
                runtime_id: 42,
                spawn_revision: 1,
                ingress_sequence: 1,
                source_tick: None,
                movement_revision: 0,
                pose_generation: render_api::PoseGeneration::new(0),
                layer: render_model::ACTOR_LAYER_BODY,
            },
            rig: render_model::pack_rig_id(binding as u32),
            previous_bones: Arc::clone(&pose),
            current_bones: pose,
            completed_tick: 0,
            reset_generation: 0,
        },
        world_from_actor: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
        texture_layer: 0,
        route: render_model::ActorRigRoute::Compiled,
        tint: 0,
        uv_anim: render_model::IDENTITY_UV_ANIM,
        light: 0,
        overlay_rgba8: 0,
    };
    let input = crate::presentation::equipment::ActorEquipmentInput {
        main: Some(crate::presentation::equipment::WornItem {
            variant: Default::default(),
            map_id: None,
            charged_projectile: None,
            use_facts: client_world::EffectiveItemUse::default(),
            grip: assets::item::grip::builtin_item_grip("minecraft:apple"),
            identifier: Arc::from("minecraft:apple"),
            metadata: 0,
            damage: None,
            kind: crate::presentation::equipment::HeldKind::Sprite,
            dye_rgb: None,
            enchanted: false,
            glint: Default::default(),
        }),
        ..Default::default()
    };
    assert_eq!(
        equipment.layers_for(&body, &input, None).len(),
        1,
        "the pack's player model must carry the held item"
    );
}

/// Icons the pack worker placed publish the frame's pages and locations without copying pixels.
#[test]
fn worker_placed_session_icons_publish_without_restaging_pixels() {
    use client_ui::ui_runtime::presentation::{SessionIcon, SessionIcons};
    let (pack, startup) = equipment_pack_fixture();
    let icon_bytes = 16 * 16 * 4;
    let icons = Arc::new(SessionIcons {
        icons: (0..64)
            .map(|index| SessionIcon {
                identifier: format!("fixture:item_{index}").into(),
                metadata: 0,
                width: 16,
                height: 16,
                rgba8: vec![index as u8; icon_bytes].into(),
            })
            .collect(),
        ..Default::default()
    });
    let items = crate::session_assets::SessionItems {
        components: Arc::default(),
        icons: Some(Arc::clone(&icons)),
    };
    let prepared = crate::prepared_actor_artwork::PreparedActorArtwork::new(&startup, &pack)
        .with_session_icons(Some(&icons));
    let resources =
        crate::prepared_actor_artwork::session_resources(&startup, Some(&pack), Some(&prepared));
    let mut scene = render_scene::ActorRenderScene::default();
    let mut ready = super::SessionGeometryReady::default();
    let (expected, _, expected_locations) = super::apply_session_pack(
        &mut scene,
        &resources,
        Some(&pack),
        super::SessionIconInput::Staged(crate::presentation::equipment::StagedSessionIcons::stage(
            Some(&items),
        )),
        &mut ready,
        None,
        None,
    );
    let copy = Arc::new(SessionIcons {
        icons: icons
            .icons
            .iter()
            .map(|icon| SessionIcon {
                identifier: Arc::clone(&icon.identifier),
                rgba8: icon.rgba8.clone(),
                ..*icon
            })
            .collect(),
        ..Default::default()
    });
    assert!(
        crate::prepared_actor_artwork::session_icons(
            &startup,
            Some(&pack),
            Some(&copy),
            Some(&prepared)
        )
        .is_none(),
        "placement is reused only for the exact icon snapshot"
    );
    let placed = crate::prepared_actor_artwork::session_icons(
        &startup,
        Some(&pack),
        Some(&icons),
        Some(&prepared),
    )
    .unwrap();
    let ((published, staged, locations), allocated) = test_alloc::measure(|| {
        super::apply_session_pack(
            &mut scene,
            &resources,
            Some(&pack),
            super::SessionIconInput::Placed(placed),
            &mut ready,
            None,
            None,
        )
    });
    assert!(staged.is_some());
    assert_eq!(locations, expected_locations);
    assert_eq!(published.unwrap().identity(), expected.unwrap().identity());
    assert!(
        allocated.bytes < icon_bytes * icons.icons.len(),
        "the frame copied icon pixels: {allocated:?}"
    );
}
