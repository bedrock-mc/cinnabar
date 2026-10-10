#[cfg(feature = "acceptance")]
use crate::runtime::phase3_evidence::{Phase3EvidenceEmitter, Phase3EvidenceIdentitySource};
#[cfg(feature = "acceptance")]
use ::acceptance::AcceptanceRun;

#[cfg(feature = "acceptance")]
use bevy::ecs::system::SystemParam;
use bevy::{
    prelude::{AppExit, MessageReader, MessageWriter, Res, ResMut},
    window::WindowCloseRequested,
};
#[cfg(feature = "acceptance")]
use render::TransparentSortMetrics;

use crate::runtime::{
    network::NetworkHandle,
    world::{ClientWorld, ShutdownWatchdog, begin_bounded_shutdown},
};
#[cfg(feature = "acceptance")]
use crate::{movement::MovementTicker, runtime::visibility::AppMetrics};
#[cfg(feature = "acceptance")]
use diagnostics::metrics::TransparentSortMetricsSnapshot;

pub(crate) fn record_fatal_error(fatal_error: &mut Option<String>, error: String) {
    if fatal_error.is_none() {
        *fatal_error = Some(error);
    }
}

pub(crate) fn fatal_runtime_exit(error: &str) -> Option<AppExit> {
    (!error.is_empty()).then(AppExit::error)
}

pub(crate) fn window_close_exit(requested: bool) -> Option<AppExit> {
    requested.then_some(AppExit::Success)
}

pub(crate) fn exit_on_window_close_requested(
    mut close_requests: MessageReader<WindowCloseRequested>,
    #[cfg(feature = "acceptance")] mut acceptance: ResMut<AcceptanceRun>,
    watchdog: Res<ShutdownWatchdog>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(exit_status) = window_close_exit(close_requests.read().next().is_some()) else {
        return;
    };
    begin_bounded_shutdown(&watchdog, &exit_status);
    #[cfg(feature = "acceptance")]
    acceptance.request_shutdown();
    exit.write(exit_status);
}

pub(crate) fn exit_on_fatal_runtime_error(
    client_world: Res<ClientWorld>,
    mut network: ResMut<NetworkHandle>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(exit_status) = client_world
        .fatal_error
        .as_deref()
        .and_then(fatal_runtime_exit)
    else {
        return;
    };
    network.shutdown();
    exit.write(exit_status);
}

#[cfg(feature = "acceptance")]
#[derive(SystemParam)]
pub(crate) struct Phase3TerminalEvidence<'w> {
    movement: Res<'w, MovementTicker>,
    identity_source: Option<Res<'w, Phase3EvidenceIdentitySource>>,
    evidence: ResMut<'w, Phase3EvidenceEmitter>,
}

/// Converts terminal observations into the ordinary network stop and app-exit commands.
#[cfg(feature = "acceptance")]
pub(crate) fn finish_acceptance_run(
    #[cfg(feature = "acceptance")] mut acceptance: ResMut<AcceptanceRun>,
    client_world: Res<ClientWorld>,
    mut metrics: ResMut<AppMetrics>,
    transparent_sort: Res<TransparentSortMetrics>,
    phase3: Phase3TerminalEvidence,
    mut network: ResMut<NetworkHandle>,
    mut exit: MessageWriter<AppExit>,
) {
    let Phase3TerminalEvidence {
        movement,
        identity_source,
        mut evidence,
    } = phase3;
    let terminal = || {
        let identity = identity_source
            .as_deref()
            .and_then(|source| source.for_session(movement.session_generation()).ok());
        acceptance::finish::TerminalMovementObservation {
            identity,
            source: match movement.source() {
                gameplay::movement::MovementSource::Physics => "Physics",
                gameplay::movement::MovementSource::FreeCamera => "FreeCamera",
            },
            physics_packet_count: movement.sent_physics_packet_count(),
            free_camera_packet_count: movement.sent_free_camera_packet_count(),
            pending_count: movement.pending_count(),
            outbox_reconciliation: movement.outbox_reconciliation().as_str(),
        }
    };
    if let Some(status) = acceptance::finish::finish_acceptance_run(
        &mut acceptance,
        client_world.fatal_error.as_deref(),
        &mut metrics.0,
        TransparentSortMetricsSnapshot::from(transparent_sort.snapshot()),
        terminal,
        &mut evidence,
    ) {
        network.shutdown();
        exit.write(status);
    }
}
