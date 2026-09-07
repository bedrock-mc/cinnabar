use bevy::{
    app::{App, AppExit, Update},
    prelude::{IntoScheduleConfigs, Quat, ResMut, Resource, Transform, Vec3},
};
use protocol::{BlobCacheStats, PlayerInputMode, ServerDisconnectEvent};
use sim::{CollisionIdSpace, CollisionRegistryIdentity, WorldCollisionIdentity};

use super::{
    super::{CoreProcessGuard, MenuAction, MenuRuntime, MenuScreen},
    drive_menu_connection, recover_menu_session_failure,
};
use crate::{
    acceptance::AcceptanceRun,
    app::{ClientBlobCacheOwner, ClientFrameSet, configure_client_frame_schedule},
    local_player::{InteractionOriginSnapshot, LocalPlayerFrameCarrier, LocalPlayerFrameSample},
    metrics::MetricsCollector,
    movement::{
        LocalPhysicsController, MovementSource, MovementTicker, PhysicsMovementSample,
        ProcessedMovementState,
    },
    runtime::{
        network::{NetworkControlEvent, NetworkFailureOrigin, drain_network_controls},
        network::{NetworkHandle, ResourcePackAdmissionState},
        shutdown::record_fatal_error,
        telemetry::send_player_auth_inputs,
        visibility::AppMetrics,
        world::ClientWorld,
    },
    semantic_controls::SemanticInputSnapshot,
    ui_runtime::UiRuntime,
};

#[derive(Resource)]
struct DelayedTerminal(Option<tokio::sync::mpsc::Sender<NetworkControlEvent>>);

fn receive_terminal_for_test(
    mut network: ResMut<NetworkHandle>,
    mut client_world: ResMut<ClientWorld>,
) {
    for event in drain_network_controls(network.control_events_mut(), 16) {
        let NetworkControlEvent::Failed {
            message,
            server_disconnect,
            ..
        } = event
        else {
            continue;
        };
        record_fatal_error(
            &mut client_world.fatal_error,
            crate::runtime::network::session_failure_display(&message, server_disconnect.as_ref()),
        );
    }
}

fn close_after_receive(mut delayed: ResMut<DelayedTerminal>) {
    let Some(sender) = delayed.0.take() else {
        return;
    };
    for _ in 0..16 {
        sender
            .try_send(NetworkControlEvent::BlobCacheTelemetry {
                enabled: true,
                stats: BlobCacheStats::default(),
            })
            .unwrap();
    }
    sender
        .try_send(NetworkControlEvent::Failed {
            message: "network read failed: closed".to_owned(),
            decode_error_count: 0,
            server_disconnect: Some(ServerDisconnectEvent {
                reason: "Kicked".to_owned(),
                message: Some("Cinnabar launcher return check".to_owned()),
                filtered_message: None,
            }),
            origin: NetworkFailureOrigin::Receive,
        })
        .unwrap();
}

fn fixture_world_identity() -> WorldCollisionIdentity {
    WorldCollisionIdentity::new(
        CollisionRegistryIdentity {
            protocol: 1001,
            id_space: CollisionIdSpace::Sequential,
            preg_sha256: [7; 32],
        },
        [],
    )
    .unwrap()
}

fn pending_sample(world_identity: WorldCollisionIdentity) -> PhysicsMovementSample {
    PhysicsMovementSample {
        tick: 11,
        position: [1.0, 64.0, 2.0],
        velocity: [0.0; 3],
        move_vector: [0.0; 2],
        raw_move_vector: [0.0; 2],
        analogue_move_vector: [0.0; 2],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        camera_orientation: [0.0, 0.0, 1.0],
        jumping: false,
        sneaking: false,
        sprinting: false,
        input_mode: PlayerInputMode::Mouse,
        grounded_before_tick: true,
        grounded_after_tick: true,
        horizontal_collision: false,
        vertical_collision: false,
        jump_repeated: false,
        processed: ProcessedMovementState::default(),
        world_identity,
    }
}

#[test]
fn pause_disconnect_retires_pending_movement_before_network_send() {
    let mut menu = MenuRuntime::new(true, 2, "Player".to_owned());
    menu.mark_connected();
    menu.open_pause();
    menu.activate(MenuAction::PauseDisconnect);

    let mut movement = MovementTicker::default();
    movement.reset(1, 10, [0.0; 3]);
    movement.set_source(MovementSource::Physics);
    movement.testing_lift_spawn_settle_gate();
    let world_identity = fixture_world_identity();
    movement
        .enqueue_completed_physics(pending_sample(world_identity.clone()))
        .unwrap();
    assert_eq!(movement.pending_count(), 1);

    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([1.0, 64.0, 2.0], 10, true);
    assert!(physics.is_active());

    let mut local_frame = LocalPlayerFrameCarrier::default();
    local_frame
        .publish(LocalPlayerFrameSample {
            session_generation: 1,
            fifo_sequence: 4,
            physics_tick: 10,
            perspective: semantic_input::PerspectiveMode::FirstPerson,
            world_collision_identity: world_identity,
            pose: Transform::default(),
            eye: Vec3::new(1.0, 64.0, 2.0),
            rotation: Quat::IDENTITY,
        })
        .unwrap();
    let mut interaction = InteractionOriginSnapshot::default();
    interaction.publish_from_local_player_frame(&local_frame);

    let mut app = App::new();
    configure_client_frame_schedule(&mut app);
    app.add_message::<AppExit>()
        .insert_resource(menu)
        .insert_resource(CoreProcessGuard::default())
        .insert_resource(NetworkHandle::disconnected())
        .insert_resource(ClientBlobCacheOwner::default())
        .insert_resource(ResourcePackAdmissionState::default())
        .insert_resource(UiRuntime::new(1))
        .insert_resource(ClientWorld::default())
        .insert_resource(movement)
        .insert_resource(physics)
        .insert_resource(AcceptanceRun::new(None, None, false, false))
        .insert_resource(SemanticInputSnapshot::default())
        .insert_resource(local_frame)
        .insert_resource(interaction)
        .insert_resource(AppMetrics(MetricsCollector::new()))
        .add_systems(
            Update,
            drive_menu_connection.in_set(ClientFrameSet::UiAuthority),
        )
        .add_systems(
            Update,
            send_player_auth_inputs.in_set(ClientFrameSet::NetworkSend),
        )
        .add_systems(
            Update,
            recover_menu_session_failure.after(ClientFrameSet::NetworkSend),
        );

    app.update();

    let menu = app.world().resource::<MenuRuntime>();
    assert_eq!(menu.view().screen, MenuScreen::Home);
    assert!(menu.view().message.is_none());
    assert!(app.world().resource::<ClientWorld>().fatal_error.is_none());
    let movement = app.world().resource::<MovementTicker>();
    assert!(!movement.physics_is_authorized());
    assert_eq!(movement.pending_count(), 0);
    assert_eq!(movement.sent_physics_packet_count(), 0);
    assert!(!app.world().resource::<LocalPhysicsController>().is_active());
    assert!(
        app.world()
            .resource::<LocalPlayerFrameCarrier>()
            .snapshot()
            .is_none()
    );
    assert!(
        app.world()
            .resource::<InteractionOriginSnapshot>()
            .outbound_ray()
            .is_none()
    );
}

#[test]
fn terminal_queued_after_receive_wins_over_closed_physics_send_and_recovers_launcher() {
    let mut menu = MenuRuntime::new(true, 2, "Player".to_owned());
    menu.mark_connected();

    let mut movement = MovementTicker::default();
    movement.reset(1, 10, [0.0; 3]);
    movement.set_source(MovementSource::Physics);
    movement.testing_lift_spawn_settle_gate();
    let world_identity = fixture_world_identity();
    movement
        .enqueue_completed_physics(pending_sample(world_identity.clone()))
        .unwrap();

    let mut local_frame = LocalPlayerFrameCarrier::default();
    local_frame
        .publish(LocalPlayerFrameSample {
            session_generation: 1,
            fifo_sequence: 4,
            physics_tick: 10,
            perspective: semantic_input::PerspectiveMode::FirstPerson,
            world_collision_identity: world_identity,
            pose: Transform::default(),
            eye: Vec3::new(1.0, 64.0, 2.0),
            rotation: Quat::IDENTITY,
        })
        .unwrap();

    let (network, terminal_sender) = NetworkHandle::stub_with_control_sender();
    let mut app = App::new();
    configure_client_frame_schedule(&mut app);
    app.add_message::<AppExit>()
        .insert_resource(menu)
        .insert_resource(CoreProcessGuard::default())
        .insert_resource(network)
        .insert_resource(DelayedTerminal(Some(terminal_sender)))
        .insert_resource(ClientBlobCacheOwner::default())
        .insert_resource(ResourcePackAdmissionState::default())
        .insert_resource(UiRuntime::new(1))
        .insert_resource(ClientWorld::default())
        .insert_resource(movement)
        .insert_resource(LocalPhysicsController::default())
        .insert_resource(AcceptanceRun::new(None, None, false, false))
        .insert_resource(SemanticInputSnapshot::default())
        .insert_resource(local_frame)
        .insert_resource(InteractionOriginSnapshot::default())
        .insert_resource(AppMetrics(MetricsCollector::new()))
        .add_systems(
            Update,
            receive_terminal_for_test.before(ClientFrameSet::NetworkSend),
        )
        .add_systems(
            Update,
            close_after_receive
                .after(receive_terminal_for_test)
                .before(ClientFrameSet::NetworkSend),
        )
        .add_systems(
            Update,
            send_player_auth_inputs.in_set(ClientFrameSet::NetworkSend),
        )
        .add_systems(
            Update,
            recover_menu_session_failure.after(ClientFrameSet::NetworkSend),
        );

    app.update();
    assert!(
        app.world().resource::<ClientWorld>().fatal_error.is_none(),
        "a queued terminal control must outrank the synthetic closed-send error"
    );
    assert!(
        !app.world().resource::<MenuRuntime>().is_visible(),
        "recovery waits for the exact queued terminal control"
    );
    assert_eq!(
        app.world().resource::<MovementTicker>().pending_count(),
        1,
        "the unsent physics snapshot remains available while control classification is pending"
    );

    app.update();
    assert!(app.world().resource::<ClientWorld>().fatal_error.is_none());
    assert!(!app.world().resource::<MenuRuntime>().is_visible());

    app.update();
    let menu = app.world().resource::<MenuRuntime>();
    assert!(menu.is_visible());
    assert_eq!(menu.view().screen, MenuScreen::Play);
    assert_eq!(
        menu.view().message.as_deref(),
        Some(
            "Disconnected: server disconnected: Cinnabar launcher return check (network read failed: closed)"
        )
    );
    assert!(app.world().resource::<ClientWorld>().fatal_error.is_none());
}
