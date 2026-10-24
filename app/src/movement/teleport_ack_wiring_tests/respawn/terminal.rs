//! A retained network terminal owns the failure ahead of a respawn completion retry.

use super::*;
use crate::runtime::shutdown::FatalError;
use client_session::{NetworkControlEvent, NetworkFailureOrigin};

#[derive(Debug)]
struct Observation {
    error: Option<String>,
    completion_pending: bool,
    original_controls_preserved: bool,
    later_controls: usize,
}

/// Leaves a real committed ready response pending, then closes its command queue between drains.
fn observe_closed_completion(later_control: bool, terminal_pending: bool) -> Observation {
    let mut app = wiring_app(authorized_ticker(false), LocalPhysicsController::default());
    submit(
        &mut app,
        1,
        WorldEvent::Respawn(RespawnEvent {
            position: [8.5, 71.620_01, -4.25],
            state: 1,
            runtime_entity_id: 0,
        }),
    );
    app.update();
    assert!(app.world().resource::<ClientWorld>().respawn.input_held());
    assert!(app.world().resource::<FatalError>().0.is_none());

    let (network, controls) = NetworkHandle::<PackApplication>::stub_with_control_sender();
    if terminal_pending {
        controls
            .try_send(NetworkControlEvent::BlobCacheTelemetry {
                enabled: false,
                stats: Default::default(),
            })
            .unwrap();
        controls
            .try_send(NetworkControlEvent::Failed {
                message: "retained remote terminal".to_owned(),
                decode_error_count: 0,
                server_disconnect: None,
                origin: NetworkFailureOrigin::Receive,
            })
            .unwrap();
    }
    app.insert_resource(network);
    if later_control {
        submit(&mut app, 2, WorldEvent::NetworkStackLatency(123));
    } else {
        app.add_systems(
            Update,
            advance_dimension_transfer.after(reconcile_world_stream_before_physics),
        );
    }
    app.update();

    let error = app.world().resource::<FatalError>().0.clone();
    let mut world = app.world_mut().resource_mut::<ClientWorld>();
    let completion_pending = world.respawn.input_held();
    let later_controls = world
        .stream
        .as_mut()
        .unwrap()
        .take_committed_controls()
        .len();
    let mut network = app
        .world_mut()
        .remove_resource::<NetworkHandle<PackApplication>>()
        .unwrap();
    let receiver = network.control_events_mut();
    let original_controls_preserved = (!terminal_pending
        || (matches!(
            receiver.try_recv(),
            Ok(NetworkControlEvent::BlobCacheTelemetry { enabled: false, .. })
        ) && matches!(
            receiver.try_recv(),
            Ok(NetworkControlEvent::Failed {
                message,
                decode_error_count: 0,
                server_disconnect: None,
                origin: NetworkFailureOrigin::Receive,
            }) if message == "retained remote terminal"
        )))
        && receiver.try_recv().is_err();
    network.shutdown();
    drop(network);
    drop(controls);
    drop(app);
    Observation {
        error,
        completion_pending,
        original_controls_preserved,
        later_controls,
    }
}

/// Both production callers must defer only while the original control FIFO can classify closure.
fn require_original_terminal(later_control: bool) {
    let pending = observe_closed_completion(later_control, true);
    let unclassified = observe_closed_completion(later_control, false);
    for observed in [&pending, &unclassified] {
        assert!(observed.completion_pending, "{observed:?}");
        assert!(observed.original_controls_preserved, "{observed:?}");
        assert_eq!(observed.later_controls, usize::from(later_control));
    }
    assert_eq!(
        unclassified.error.as_deref(),
        Some("respawn completion: network command channel is closed"),
        "an unclassified closure must still fail"
    );
    assert!(
        pending.error.is_none(),
        "the queued receive terminal must precede a synthetic respawn failure: {pending:?}"
    );
}

#[test]
fn respawn_completion_before_later_control_preserves_queued_terminal() {
    require_original_terminal(true);
}

#[test]
fn respawn_completion_after_controls_preserves_queued_terminal() {
    require_original_terminal(false);
}
