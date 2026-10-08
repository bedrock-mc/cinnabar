//! Native local body and equipment share the physics-sampled attack parent.
use super::*;
use bevy::ecs::system::SystemState;
use bevy::math::Quat;
use bevy::prelude::{PerspectiveProjection, Projection, Time, Transform, World};
use bevy::time::Real;

#[derive(Clone, Copy, PartialEq)]
enum BodyCase {
    Native,
    JavaFallback,
    Emote,
    InvalidEmote,
    Persona,
}

/// Runs the actual early and final publication owners with independent interpolation clocks.
fn assert_native_body_sample(equipment_parent: bool, case: BodyCase) {
    for duration in [6.0f32, 4.0, 8.0] {
        for (actor_alpha, physics_alpha) in [(0.1, 0.75), (0.9, 0.25)] {
            let ((mut stream, equipment, artwork, input), entities) =
                super::native_tests::fixture_with_appearance(
                    Some("minecraft:shield"),
                    false,
                    case == BodyCase::Persona,
                );
            if case == BodyCase::JavaFallback {
                stream
                    .submit(
                        1,
                        protocol::WorldEvent::Actor(protocol::ActorEvent::Metadata(
                            protocol::ActorMetadataUpdateEvent {
                                dimension: 0,
                                runtime_id: 1,
                                metadata: Arc::from([protocol::ActorMetadata {
                                    key: 0,
                                    value: protocol::ActorMetadataValue::Flags(1 << 57),
                                }]),
                                properties: Arc::from([]),
                                tick: 0,
                            },
                        )),
                    )
                    .unwrap();
                stream.advance_actor_interpolation_frame(1);
                assert!(
                    stream
                        .authority()
                        .actor_rig(1)
                        .unwrap()
                        .java
                        .vanilla_posture
                );
            }
            let mut world = World::new();
            let mut time = Time::<Real>::default();
            time.advance_by(std::time::Duration::from_millis(50));
            world.insert_resource(time);
            world.insert_resource(equipment);
            world.insert_resource(artwork);
            world.insert_resource(
                render::ActorRenderScene::with_runtime_entity_assets(&entities).unwrap(),
            );
            world.insert_resource(
                super::super::HandRigBuilder::from_runtime_assets(&entities).unwrap(),
            );
            world.init_resource::<super::super::ActorFrameState>();
            world.init_resource::<super::super::PreparedActorPublication>();
            world.init_resource::<super::super::ActorFramePartialTick>();
            world.init_resource::<render::HandRigScene>();
            let mut avatar = crate::local_player::LocalAvatarPresentation::default();
            avatar.begin_session(stream.authority().actor_session_id(), 1);
            world.insert_resource(avatar);
            world.init_resource::<crate::local_player::LocalAvatarVisibilityCarrier>();
            let mut settings = crate::camera::CameraSettingsAuthority::default();
            let mut user = ui::UserSettings::default();
            user.gameplay.default_perspective = semantic_input::PerspectiveMode::ThirdPersonBack;
            user.video.java_animations = case == BodyCase::JavaFallback;
            settings.replace(1, &user).unwrap();
            world.insert_resource(settings);
            world.insert_resource(crate::local_player::LocalViewPose::default());
            world.spawn((
                Transform::default(),
                Projection::Perspective(PerspectiveProjection::default()),
                crate::camera::FlyCamera::default(),
            ));
            let mut params = SystemState::<super::super::ActorFramePublication>::new(&mut world);
            let mut prepared_artwork = None;
            super::super::advance_actor_frame(
                super::super::ActorWorld {
                    stream: Some(&mut stream),
                    collisions: None,
                    entity_assets: Some(&entities),
                    pack_entities: None,
                    session_items: None,
                    prepared_actor_artwork: &mut prepared_artwork,
                },
                super::super::ActorFrameInput {
                    local_feed: None,
                    predicted_eye: Some([0.0, 65.62, 0.0]),
                    predicted_feet: Some([0.0, 64.0, 0.0]),
                    local_equipment: input.clone(),
                    swing_progress: None,
                    renders_game: true,
                    hide_hand: false,
                    custom_emote: match case {
                        BodyCase::Emote => Some((client_world::CustomEmote::Twerk, 0.1)),
                        BodyCase::InvalidEmote => Some((client_world::CustomEmote::Twerk, -1.0)),
                        _ => None,
                    },
                },
                |_| {},
                |_, _| (None, None),
                params.get_mut(&mut world),
            );
            world
                .resource_mut::<super::super::ActorFrameState>()
                .step
                .as_mut()
                .unwrap()
                .partial_tick = actor_alpha;
            let swing = client_world::LocalSwingProgress {
                bedrock: [1.0 / duration, 2.0 / duration],
                java: [1.0 / duration, 2.0 / duration],
                frame_alpha: Some(physics_alpha),
            };
            super::super::prepare_actor_render_frame(
                super::super::ActorWorld {
                    stream: Some(&mut stream),
                    collisions: None,
                    entity_assets: Some(&entities),
                    pack_entities: None,
                    session_items: None,
                    prepared_actor_artwork: &mut prepared_artwork,
                },
                Some(swing),
                |_, _, _, _| false,
                params.get_mut(&mut world),
            );
            let rig = stream.authority().actor_rig(1).unwrap();
            let actor = stream.authority().actor(1).unwrap();
            let committed = (
                rig.completed_tick,
                rig.previous.to_vec(),
                rig.current.to_vec(),
            );
            let stats = stream.authority().actor_animation_stats();
            let body = world
                .resource::<super::super::PreparedActorPublication>()
                .submissions()
                .unwrap()
                .iter()
                .find(|draw| draw.input.identity.layer == render::ACTOR_LAYER_BODY)
                .unwrap()
                .clone();
            if case == BodyCase::Emote {
                let pose = client_world::sample_custom_emote(
                    &rig,
                    client_world::CustomEmote::Twerk,
                    0.1,
                    0.1,
                )
                .unwrap();
                assert_eq!(
                    body.input.current_bones,
                    crate::presentation::actors::convert_bones(&pose.current).unwrap()
                );
                assert_eq!(
                    world
                        .resource::<super::super::ActorFrameState>()
                        .java_hand
                        .native_pose
                        .sample_work,
                    (0, 0),
                    "successful emotes discard native body sampling"
                );
                continue;
            }
            let presentation = crate::presentation::actors::actor_rig_presentation(
                &rig,
                actor,
                stream.authority().actor_player_profile(1),
                actor_alpha,
            )
            .unwrap();
            let camera = world
                .resource::<super::super::ActorFrameState>()
                .captured_sampling_camera;
            let mut reference = NativePoseCache::default();
            let [previous, current] = reference
                .sample(&stream, &presentation, None, None, actor_alpha, camera)
                .unwrap();
            let arm = rig
                .bone_names
                .iter()
                .position(|name| name.as_ref() == "rightarm")
                .unwrap();
            let expected =
                Quat::from_rotation_x(-((1.0 + physics_alpha) / duration) * 80.0f32.to_radians());
            assert!(Quat::from_array(current[arm].rotation).abs_diff_eq(expected, 1e-5));
            if equipment_parent {
                let mut expected_body = body.clone();
                expected_body.input.previous_bones = previous;
                expected_body.input.current_bones = current;
                let delta_seconds = world.resource::<Time<Real>>().delta_secs();
                let expected = world.resource_mut::<EquipmentRuntime>().layers_for(
                    &expected_body,
                    &input,
                    Some(crate::presentation::equipment::EquipmentAnimation {
                        owner: actor,
                        rig: &rig,
                        frame_alpha: actor_alpha,
                        delta_seconds,
                    }),
                );
                let expected = expected.first().expect("native held equipment");
                let actual = world
                    .resource::<super::super::PreparedActorPublication>()
                    .submissions()
                    .unwrap()
                    .iter()
                    .find(|draw| draw.input.identity == expected.submission.input.identity)
                    .unwrap();
                assert_eq!(
                    actual.input.previous_bones, expected.submission.input.previous_bones,
                    "native equipment previous parent duration {duration}"
                );
                assert_eq!(
                    actual.input.current_bones, expected.submission.input.current_bones,
                    "native equipment current parent duration {duration}"
                );
            } else {
                assert!(
                    Quat::from_array(body.input.current_bones[arm].rotation)
                        .abs_diff_eq(expected, 1e-5),
                    "native third-person arm must use physics {physics_alpha}, not actor {actor_alpha}, duration {duration}: {:?}",
                    body.input.current_bones[arm].rotation
                );
                assert_eq!(body.input.previous_bones, previous);
                assert_eq!(body.input.current_bones, current);
            }
            if case == BodyCase::Persona {
                assert_eq!(
                    rig.skin_layers.len(),
                    1,
                    "synthetic animated skin is admitted"
                );
                let layer = world
                    .resource::<super::super::PreparedActorPublication>()
                    .submissions()
                    .unwrap()
                    .iter()
                    .find(|draw| {
                        crate::presentation::skin_layers::is_skin_layer(draw.input.identity.layer)
                    })
                    .unwrap();
                assert!(
                    Quat::from_array(layer.input.current_bones[0].rotation)
                        .abs_diff_eq(expected, 1e-5),
                    "native persona layer must use physics {physics_alpha}, not actor {actor_alpha}: {:?}",
                    layer.input.current_bones[0].rotation
                );
                assert!(
                    Quat::from_array(layer.input.previous_bones[0].rotation)
                        .abs_diff_eq(expected, 1e-5)
                );
                assert_ne!(
                    layer.input.current_bones[0].translation_scale,
                    body.input.current_bones[arm].translation_scale,
                    "layer retains its own geometry pivot"
                );
            }
            let mut repeated = presentation.clone();
            {
                let mut state = world.resource_mut::<super::super::ActorFrameState>();
                let work = state.java_hand.native_pose.sample_work;
                assert_eq!(work.0, 1, "only the native body consumer samples the frame");
                let allocations = crate::test_allocations::count();
                state
                    .java_hand
                    .native_pose
                    .apply_body(&stream, &mut repeated, actor_alpha, camera);
                assert_eq!(state.java_hand.native_pose.sample_work, work);
                assert_eq!(crate::test_allocations::count(), allocations);
            }
            let unchanged = stream.authority().actor_rig(1).unwrap();
            assert_eq!(
                (
                    unchanged.completed_tick,
                    unchanged.previous.to_vec(),
                    unchanged.current.to_vec()
                ),
                committed
            );
            assert_eq!(stream.authority().actor_animation_stats(), stats);
        }
    }
}

#[test]
fn native_third_person_body_samples_the_physics_swing_without_committing_state() {
    assert_native_body_sample(false, BodyCase::Native);
}

#[test]
fn native_third_person_equipment_samples_the_complete_physics_parent() {
    assert_native_body_sample(true, BodyCase::Native);
}

#[test]
fn native_third_person_java_native_fallback_samples_physics_phase() {
    assert_native_body_sample(false, BodyCase::JavaFallback);
}

#[test]
fn native_third_person_successful_emote_discards_no_native_sampling_work() {
    assert_native_body_sample(false, BodyCase::Emote);
}

#[test]
fn native_third_person_invalid_emote_keeps_native_sampling() {
    assert_native_body_sample(false, BodyCase::InvalidEmote);
}

#[test]
fn native_third_person_persona_layer_samples_its_own_physics_pose() {
    assert_native_body_sample(false, BodyCase::Persona);
}
