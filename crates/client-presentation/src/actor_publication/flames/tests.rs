use super::*;
use protocol::{
    ActorEvent, ActorMetadata, ActorMetadataValue, ActorSpawnEvent, WorldBootstrap, WorldEvent,
};
use protocol_ids::{ActorDataKey, ActorFlag};

/// Builds an isolated authoritative zombie with no inferred daylight or damage state.
fn world(burning: bool) -> client_world::WorldAuthority {
    let mut world = client_world::WorldAuthority::new(
        WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 999,
            local_player_unique_id: 999,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    );
    world
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: 7,
                runtime_id: 7,
                kind: protocol::ActorKind::Entity {
                    identifier: "minecraft:zombie".into(),
                },
                position: [3.0, 70.0, 8.0],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 65.0,
                head_yaw: 65.0,
                body_yaw: 65.0,
                held_item: Default::default(),
                metadata: Arc::from([
                    ActorMetadata {
                        key: ActorDataKey::Flags.wire_value(),
                        value: ActorMetadataValue::Flags(
                            u64::from(burning) << ActorFlag::OnFire.wire_value(),
                        ),
                    },
                    ActorMetadata {
                        key: ActorDataKey::Width.wire_value(),
                        value: ActorMetadataValue::Float(0.6),
                    },
                    ActorMetadata {
                        key: ActorDataKey::Height.wire_value(),
                        value: ActorMetadataValue::Float(1.9),
                    },
                ]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
            Some(1),
        )
        .unwrap();
    world
}

/// Supplies one selected body, preserving its exact spawn and publication identity.
fn batch(world: &client_world::WorldAuthority) -> ActorPresentationBatch {
    let actor = world.actor(7).unwrap();
    ActorPresentationBatch {
        submissions: vec![ActorRigSubmission {
            input: ActorRigRenderInput {
                identity: render_model::ActorRenderIdentity {
                    session_id: world.actor_session_id(),
                    dimension: 0,
                    runtime_id: 7,
                    spawn_revision: actor.render_spawn_revision,
                    ingress_sequence: actor.spawn_revision,
                    source_tick: None,
                    movement_revision: actor.movement_revision,
                    pose_generation: render_api::PoseGeneration::new(actor.spawn_revision),
                    layer: render_model::ACTOR_LAYER_BODY,
                },
                rig: render_model::DIAGNOSTIC_RIG_ID,
                previous_bones: Arc::from([RenderBoneTransform::default()]),
                current_bones: Arc::from([RenderBoneTransform::default()]),
                completed_tick: 1,
                reset_generation: 1,
            },
            world_from_actor: [
                [1.0, 0.0, 0.0, 3.0],
                [0.0, 1.0, 0.0, 70.0],
                [0.0, 0.0, 1.0, 8.0],
            ],
            material: Default::default(),
            culling_bounds: Default::default(),
            texture_layer: 0,
            route: ActorRigRoute::Compiled,
            tint: 0,
            overlay_rgba8: 0,
            uv_anim: render_model::IDENTITY_UV_ANIM,
            light: 0,
        }],
        skin_layers: Vec::new(),
        artwork: HashMap::new(),
    }
}

/// Original three-frame marker art distinguishes retained animation frames.
pub(in crate::actor_publication) fn texture() -> assets::ParticleTexture {
    assets::ParticleTexture {
        path: assets::ACTOR_FLAME_TEXTURE.into(),
        width: 8,
        height: 24,
        rgba8: (0..3)
            .flat_map(|index| vec![index * 60; 8 * 8 * 4])
            .collect::<Vec<_>>()
            .into(),
    }
}

/// Applies the same authoritative flag update regardless of the server's fire source.
fn set_flags(world: &mut client_world::WorldAuthority, flags: u64, tick: u64) {
    world
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 7,
                metadata: Arc::from([ActorMetadata {
                    key: ActorDataKey::Flags.wire_value(),
                    value: ActorMetadataValue::Flags(flags),
                }]),
                properties: Arc::from([]),
                tick,
            })),
            Some(tick),
        )
        .unwrap();
}

#[test]
fn metadata_ignition_extinguishing_and_invisibility_control_flames_without_respawning() {
    let mut world = world(false);
    let (mut flames, _) = WorldActorFlames::prepare(ActorArtworkPages::default(), Some(&texture()));
    let fire = 1 << ActorFlag::OnFire.wire_value();
    let invisible = 1 << ActorFlag::Invisible.wire_value();
    for (tick, flags, expected) in [
        (2, 0, 1),
        (3, fire, 2),
        (4, 0, 1),
        (5, fire, 2),
        (6, fire | invisible, 1),
        (7, fire, 2),
    ] {
        set_flags(&mut world, flags, tick);
        let mut output = batch(&world);
        flames.append(Some(&world), 0.0, 0.5, None, &mut output);
        assert_eq!(output.submissions.len(), expected, "flags at update {tick}");
    }
}

#[test]
fn authoritative_burning_zombie_reaches_the_world_draw_builder_with_original_frames() {
    let world = world(true);
    let (mut flames, pages) =
        WorldActorFlames::prepare(ActorArtworkPages::default(), Some(&texture()));
    let mut output = batch(&world);
    flames.append(Some(&world), 0.0, 0.5, None, &mut output);
    assert_eq!(output.submissions.len(), 2);
    let fire = output.submissions.pop().unwrap();
    assert_eq!(fire.input.identity.runtime_id, 7);
    assert_eq!(
        fire.input.identity.spawn_revision,
        world.actor(7).unwrap().render_spawn_revision
    );
    let location = output.artwork[&fire.input.identity];
    assert!(pages.valid(fire.input.rig, location));
    let page = &pages.pages[location.page as usize - 1];
    assert_eq!(page.rgba8[location.layer as usize * 8 * 8 * 4], 60);
    let mut scene = render_scene::ActorRenderScene::default();
    scene.configure_artwork(pages);
    scene.insert_geometry(flames.geometry().unwrap()).unwrap();
    let frame = scene.update_rigs_with_artwork(0.5, None, [fire], &[], &output.artwork);
    assert_eq!(frame.rig.rejects, render_model::ActorRigRejects::default());
    assert_eq!(
        frame.rig.instances.len(),
        1,
        "fire must survive actual artwork and geometry validation"
    );
}

#[test]
fn extinguished_hidden_unselected_and_missing_art_actors_have_no_flames() {
    let (mut flames, _) = WorldActorFlames::prepare(ActorArtworkPages::default(), Some(&texture()));
    let unlit = world(false);
    let mut output = batch(&unlit);
    flames.append(Some(&unlit), 0.0, 0.5, None, &mut output);
    assert_eq!(output.submissions.len(), 1);
    let burning = world(true);
    let mut output = batch(&burning);
    output.submissions[0].route = ActorRigRoute::NoDraw;
    flames.append(Some(&burning), 0.0, 0.5, None, &mut output);
    assert_eq!(output.submissions.len(), 1);
    output.submissions.clear();
    flames.append(Some(&burning), 0.0, 0.5, None, &mut output);
    assert!(output.submissions.is_empty());
    let (mut missing, _) = WorldActorFlames::prepare(ActorArtworkPages::default(), None);
    let mut output = batch(&burning);
    missing.append(Some(&burning), 0.0, 0.5, None, &mut output);
    assert_eq!(output.submissions.len(), 1);
}

#[test]
fn flames_follow_the_fresh_camera_and_collision_box_without_following_body_yaw() {
    let world = world(true);
    let (mut flames, _) = WorldActorFlames::prepare(ActorArtworkPages::default(), Some(&texture()));
    let mut front = batch(&world);
    flames.append(Some(&world), 0.0, 0.5, None, &mut front);
    let mut side = batch(&world);
    flames.append(
        Some(&world),
        std::f32::consts::FRAC_PI_2,
        0.5,
        None,
        &mut side,
    );
    let a = front.submissions[1].world_from_actor;
    let b = side.submissions[1].world_from_actor;
    assert!((a[0][0] + 0.84).abs() < 1e-6);
    assert!((a[1][1] - 0.84).abs() < 1e-6);
    assert!(a[2][0].abs() < 1e-6);
    assert!(b[0][0].abs() < 1e-6);
    assert!((b[2][0] - 0.84).abs() < 1e-6);
    assert!(
        a[2][3] > 8.0,
        "front-view flames must sit toward the camera"
    );
    assert!(b[0][3] > 3.0, "side-view flames must sit toward the camera");
    assert_eq!(a[1][3], 70.0);
    assert_eq!(b[1][3], 70.0);
    flames.append(None, 0.0, 0.5, None, &mut side);
    assert!(flames.playback.is_empty());
}

#[test]
fn local_physical_feet_override_only_the_matching_body_publication() {
    let world = world(true);
    let (mut flames, _) = WorldActorFlames::prepare(ActorArtworkPages::default(), Some(&texture()));
    let body = batch(&world).submissions.remove(0);
    let placement = LocalFlameFeet {
        identity: body.input.identity,
        feet: [30.0, 80.0, 90.0],
    };
    let mut output = batch(&world);
    flames.append(Some(&world), 0.0, 0.5, Some(placement), &mut output);
    assert_eq!(output.submissions[1].world_from_actor[1][3], 80.0);
    for identity in [
        render_model::ActorRenderIdentity {
            session_id: placement.identity.session_id + 1,
            ..placement.identity
        },
        render_model::ActorRenderIdentity {
            dimension: placement.identity.dimension + 1,
            ..placement.identity
        },
        render_model::ActorRenderIdentity {
            runtime_id: placement.identity.runtime_id + 1,
            ..placement.identity
        },
        render_model::ActorRenderIdentity {
            spawn_revision: placement.identity.spawn_revision.checked_add(1).unwrap(),
            ..placement.identity
        },
        render_model::ActorRenderIdentity {
            pose_generation: render_api::PoseGeneration::new(999),
            ..placement.identity
        },
    ] {
        let mut output = batch(&world);
        flames.append(
            Some(&world),
            0.0,
            0.5,
            Some(LocalFlameFeet {
                identity,
                ..placement
            }),
            &mut output,
        );
        assert_eq!(
            output.submissions[1].world_from_actor[1][3], 70.0,
            "a stale or other actor's sample must use remote interpolation"
        );
    }
}
