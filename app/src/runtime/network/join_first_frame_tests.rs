//! Original join inputs through bootstrap, pack publication, rendering and movement transport.

use super::*;
use bevy::prelude::*;
use client_interaction::movement::{
    LocalPhysicsController, MovementTicker, PhysicsCollisionRegistries,
};
use client_presentation::{
    actor_publication::{ActorFramePartialTick, HandRigBuilder, publish_actor_render_frame},
    camera::{AutoFly, FlyCamera},
    local_player::{CameraPose, LocalAvatarVisibilityCarrier},
};
use client_state::{ClientFrameSet, SemanticInputSnapshot};
use render_scene::{ActorRenderFrame, ChunkRenderInstance, ChunkTextureReload};
use std::{sync::Arc, time::Duration};

mod fixture;
use fixture::*;

/// Runs the production pack consumer without completing the held render request.
fn reload(app: &mut App) {
    app.world_mut()
        .run_system_cached(reload_resource_packs)
        .unwrap();
    assert!(app.world().resource::<PackReload>().error().is_none());
}

/// Polls authoritative terrain to a committed frontier without advancing the simulation clock.
fn commit(app: &mut App, sequence: u64) {
    test_time::eventually("join fixture ingress", || {
        let mut world = app
            .world_mut()
            .resource_mut::<crate::runtime::world::ClientWorld>();
        let stream = world.stream.as_mut().unwrap();
        stream.begin_frame_work();
        stream.poll(EYE, 0);
        stream.committed_sequence() >= sequence
    });
}

/// Checks actual prepared draw spans against the artwork routed by the published actor frame.
fn assert_current_frame(app: &mut App) -> render::PublicationActorPreparation {
    let mut query = app.world_mut().query::<&ChunkRenderInstance>();
    let world = app.world().resource::<crate::runtime::world::ClientWorld>();
    let stream = world.stream.as_ref().unwrap();
    let tint = stream.biome_tint_identity();
    assert!(stream.authority().actor(ACTOR).is_some());
    let assets = stream.runtime_assets();
    let chunks: Vec<_> = query.iter(app.world()).collect();
    assert_eq!(
        chunks.len(),
        1,
        "the whole original terrain fixture must be prepared"
    );
    assert_eq!(chunks[0].key(), KEY);
    assert_eq!(chunks[0].tint_identity(), tint);
    assert!(
        !chunks[0].quads().is_empty(),
        "terrain is a positive draw control"
    );
    for quad in chunks[0].quads() {
        let texture = assets.material(quad.material_id()).texture;
        let page = &assets.texture_pages()[texture.page() as usize];
        let mip = &page.texture.mips[0];
        let at = texture.layer() as usize * (mip.size * mip.size) as usize * 4;
        assert_eq!(&mip.rgba8[at..at + 4], &TERRAIN_COLOR);
    }
    let terminal = render::publication_render_terminal_snapshot(app);
    assert_eq!(terminal.allocation_manifest.len(), 1);
    assert_eq!(terminal.allocation_manifest[0].0, KEY);
    let frame = app.world().resource::<ActorRenderFrame>();
    assert_eq!(
        frame.rig.manifest.len(),
        1,
        "the original actor must reach publication"
    );
    assert_eq!(frame.rig.manifest[0].identity.runtime_id, ACTOR);
    let prepared = render::publication_actor_preparation(app);
    assert_eq!(prepared.instances, 1);
    assert!(prepared.geometry_ready && prepared.artwork_current);
    assert_eq!(prepared.artwork_identity, frame.artwork_pages().identity());
    assert_eq!(prepared.spans.len(), 1);
    let (page, span) = &prepared.spans[0];
    assert_eq!(span, &(0..1));
    assert_eq!(*page, frame.instance_pages[0]);
    let page_index = usize::from(
        page.checked_sub(1)
            .expect("the actor must use generic artwork"),
    );
    let layer = frame.rig.instances[0].texture_layer as usize;
    let texture = &frame.artwork_pages().pages()[page_index];
    let pixels = texture.pixels();
    let at = layer * texture.width as usize * texture.height as usize * 4;
    assert_eq!(&pixels[at..at + 4], &ACTOR_COLOR);
    prepared
}

#[test]
fn join_first_frame_keeps_current_artwork_after_reversed_biome_completion_and_sends_movement() {
    let _sounds = client_audio::audio::SERVER_SOUNDS_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut app, _) = crate::runtime::world::committed_ui::tests::fixture_app();
    let (_original, artwork, entities) = crate::tests::actor_rest_presentation::compiled_fixture(
        "1.0",
        1,
        assets::ActorPoseMode::CompiledLiteral,
    );
    let blocks = blocks();
    let inputs = Arc::new(pack_runtime::PackInputs {
        blocks: blocks.clone(),
        hashed: true,
        ..Default::default()
    });
    let mut packs = resource_packs::prepare_validated_application(
        resource_pack::validate_handoff(bedrock_packs::ResourcePackHandoff::default()),
        inputs,
    );
    let base = app
        .world()
        .resource::<crate::runtime::world::ClientWorld>()
        .runtime_assets
        .clone();
    packs.registries = session_registries::RegistryBase::new(
        &base,
        app.world().resource::<PhysicsCollisionRegistries>(),
    )
    .prepare(
        simulation_clock::identity::SessionGeneration::new(1),
        true,
        &blocks,
        &packs,
        &|| false,
    );
    app.init_resource::<PublicationController>()
        .init_resource::<ChunkUploadAcknowledgements>()
        .init_resource::<LocalAvatarPresentation>()
        .init_resource::<LocalAvatarVisibilityCarrier>()
        .insert_resource(PhysicsAuthorityGate::ProductionEnabled)
        .insert_resource(AutoFly::new(false))
        .insert_resource(crate::runtime::visibility::AppMetrics(
            diagnostics::metrics::MetricsCollector::new(),
        ))
        .init_resource::<PackReload>();
    app.world_mut()
        .resource_mut::<PackReload>()
        .request_globals(pack());
    {
        let mut world = app
            .world_mut()
            .resource_mut::<crate::runtime::world::ClientWorld>();
        world.stream = None;
        world.entity_assets = Some(entities.clone());
    }
    *app.world_mut()
        .resource_mut::<client_presentation::environment::WorldClock>() = Default::default();
    let (network, sender) =
        NetworkHandle::<resource_packs::PackApplication>::stub_with_control_sender();
    sender.try_send(bootstrap(packs, blocks.clone())).unwrap();
    app.insert_resource(network);
    app.world_mut()
        .run_system_cached(receive_network_events)
        .unwrap();
    assert!(
        app.world()
            .resource::<crate::runtime::shutdown::FatalError>()
            .0
            .is_none()
    );
    assert!(app.world().resource::<LocalPhysicsController>().is_active());
    let (network, mut sent) =
        NetworkHandle::<resource_packs::PackApplication>::stub_capturing_packets();
    app.insert_resource(network);
    {
        let mut world = app
            .world_mut()
            .resource_mut::<crate::runtime::world::ClientWorld>();
        let stream = world.stream.as_mut().unwrap();
        stream
            .submit(1, terrain(blocks.blocks[0].hashed_states()[0].hash))
            .unwrap();
        stream.submit(2, actor()).unwrap();
    }
    commit(&mut app, 2);
    let joined_assets = app
        .world()
        .resource::<crate::runtime::world::ClientWorld>()
        .stream
        .as_ref()
        .unwrap()
        .runtime_assets()
        .clone();
    install_renderer(&mut app, joined_assets, artwork, entities);
    seed_resident_terrain(&mut app);
    render::settle_publication_noop_frame(&mut app);
    // The resident fixture really reached GPU allocation before the replacement is held.
    assert_eq!(
        render::publication_render_terminal_snapshot(&mut app)
            .allocation_manifest
            .len(),
        1
    );

    let gpu = app.world().resource::<ChunkTextureReload>().clone();
    test_time::eventually("initial pack geometry request", || {
        reload(&mut app);
        gpu.preparation().is_some()
    });
    let obsolete = gpu.preparation().unwrap();
    {
        let mut world = app
            .world_mut()
            .resource_mut::<crate::runtime::world::ClientWorld>();
        world.stream.as_mut().unwrap().submit(3, biomes()).unwrap();
    }
    commit(&mut app, 3);
    test_time::eventually("replacement biome geometry request", || {
        reload(&mut app);
        gpu.preparation()
            .is_some_and(|request| !gpu.is_current(&obsolete) && request.geometry().is_some())
    });
    // The newer preparation completes first through the real NOOP renderer.
    let replacement = gpu.preparation().unwrap();
    test_time::eventually("current pack GPU preparation", || {
        render::settle_publication_noop_frame(&mut app);
        gpu.status(replacement.assets().binding_identity()) == Some(Ok(()))
    });
    assert!(
        !gpu.finish(&obsolete, true),
        "the old biome completion must be rejected"
    );
    test_time::eventually("current pack publication", || {
        reload(&mut app);
        app.world().resource::<PackReload>().progress().is_none()
    });

    // Press before the first frame selecting the prepared pack, then observe that exact frame.
    drop(sent.drain());
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyW);
    *app.world_mut()
        .resource_mut::<bevy::time::TimeUpdateStrategy>() =
        bevy::time::TimeUpdateStrategy::ManualDuration(Duration::from_millis(50));
    render::settle_publication_noop_frame(&mut app);
    assert_current_frame(&mut app);
    assert_eq!(
        app.world().resource::<SemanticInputSnapshot>().movement(),
        [0.0, 1.0]
    );
    let packets = sent
        .take_flushed_frame()
        .expect("the movement frame must reach transport flush");
    let movement: Vec<_> = packets
        .iter()
        .filter_map(protocol::player_auth_input_trace_sample)
        .collect();
    assert_eq!(
        movement.len(),
        1,
        "the first movement input must be sent immediately"
    );
    assert_eq!(movement[0].move_vector, [0.0, 1.0]);
    assert_eq!(movement[0].raw_move_vector, [0.0, 1.0]);
    assert_ne!(movement[0].position[2], EYE[2]);
    assert!(
        app.world()
            .resource::<MovementTicker>()
            .physics_is_authorized()
    );
    assert!(
        app.world()
            .resource::<crate::runtime::shutdown::FatalError>()
            .0
            .is_none()
    );
}

/// A queued failure belongs to its session even after a replacement request is accepted.
#[cfg(feature = "acceptance")]
#[test]
fn old_session_failure_preserves_replacement_join_profile() {
    let (mut app, _) = crate::runtime::world::committed_ui::tests::fixture_app();
    app.init_resource::<PublicationController>()
        .init_resource::<ChunkUploadAcknowledgements>()
        .init_resource::<LocalAvatarPresentation>()
        .insert_resource(PhysicsAuthorityGate::ProductionEnabled)
        .insert_resource(AutoFly::new(false))
        .insert_resource(crate::runtime::visibility::AppMetrics(
            diagnostics::metrics::MetricsCollector::new(),
        ));
    let old = bridge::join_profile::JoinProfile::begin();
    let (network, sender) = old.run(NetworkHandle::<PackApplication>::stub_with_control_sender);
    sender
        .try_send(NetworkControlEvent::Failed {
            message: "old session failed".into(),
            decode_error_count: 0,
            server_disconnect: None,
            origin: client_session::NetworkFailureOrigin::Receive,
        })
        .unwrap();
    app.insert_resource(network);
    let replacement = bridge::join_profile::JoinProfile::begin();
    let expected = replacement.id().expect("replacement profile is active");
    replacement.run(|| {
        app.world_mut()
            .run_system_cached(receive_network_events)
            .unwrap()
    });
    assert!(
        app.world()
            .resource::<crate::runtime::shutdown::FatalError>()
            .0
            .is_some()
    );
    assert_eq!(
        replacement.id(),
        Some(expected),
        "old session must not finish the replacement request"
    );
    replacement.finish("test_complete");
}
