#[cfg(feature = "acceptance")]
use crate::runtime::movement_evidence::{MovementEvidenceEmitter, MovementEvidenceIdentitySource};
use crate::runtime::network::resource_packs::PackApplication;
#[cfg(feature = "acceptance")]
use ::acceptance::AcceptanceRun;
use client_runtime::network::NetworkHandle;

#[cfg(feature = "acceptance")]
use bevy::ecs::system::SystemParam;
use bevy::{
    prelude::{App, AppExit, Last, Local, MessageReader, MessageWriter, Res, ResMut, Resource},
    window::WindowCloseRequested,
};
#[cfg(feature = "acceptance")]
use render_scene::TransparentSortMetrics;
use std::sync::{Arc, Mutex, PoisonError};

#[cfg(not(target_arch = "wasm32"))]
use client_runtime::shutdown::{ShutdownWatchdog, begin_bounded_shutdown};
#[cfg(feature = "acceptance")]
use diagnostics::metrics::TransparentSortMetricsSnapshot;
#[cfg(feature = "acceptance")]
use {crate::runtime::visibility::AppMetrics, client_interaction::movement::MovementTicker};

/// The error that ends the current session, kept apart from `ClientWorld` so systems that only
/// report a failure do not borrow the whole world mutably. Session teardown clears it; shutdown,
/// session recovery and gameplay gates read it.
#[derive(Resource, Debug, Default)]
pub(crate) struct FatalError(pub(crate) Option<String>);

pub(crate) fn record_fatal_error(fatal_error: &mut Option<String>, error: String) {
    if fatal_error.is_none() {
        *fatal_error = Some(error);
    }
}

/// The first fatal error behind an error exit, readable after `App::run` consumes the world.
#[derive(Resource, Clone, Default)]
pub(crate) struct TerminalCause(Arc<Mutex<Option<String>>>);

impl TerminalCause {
    /// Captures into a handle that outlives `app`.
    pub(crate) fn install(app: &mut App) -> Self {
        let cause = Self::default();
        app.insert_resource(cause.clone())
            .add_systems(Last, capture_terminal_cause);
        cause
    }

    /// Keeps the first cause; later failures during teardown are consequences of it.
    fn record(&self, cause: String) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert(cause);
    }

    /// The process failure for an error exit, naming its cause when one was recorded.
    pub(crate) fn exit_message(&self) -> String {
        let cause = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        match cause.as_deref() {
            Some(cause) => format!("Bevy app exited after a fatal runtime error: {cause}"),
            None => "Bevy app exited after a fatal runtime error".to_owned(),
        }
    }
}

fn capture_terminal_cause(
    mut exits: MessageReader<AppExit>,
    fatal_error: Option<Res<FatalError>>,
    cause: Res<TerminalCause>,
) {
    if !exits.read().any(AppExit::is_error) {
        return;
    }
    let Some(error) = fatal_error
        .and_then(|fatal_error| fatal_error.0.clone())
        .filter(|error| !error.is_empty())
    else {
        return;
    };
    cause.record(error);
}

/// Ends the app once the render device is lost, naming the driver's reason; nothing can draw
/// again, so the crash window shows it and offers a restart.
pub(crate) fn exit_on_device_loss(
    loss: Res<render::DeviceLoss>,
    cause: Res<TerminalCause>,
    mut reported: Local<bool>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(message) = loss.message() else {
        return;
    };
    if std::mem::replace(&mut *reported, true) {
        return;
    }
    cause.record(device_loss_cause(message));
    exit.write(AppExit::error());
}

fn device_loss_cause(driver: &str) -> String {
    format!(
        "the graphics device stopped working: the driver reset, the GPU was removed, or it ran \
         out of video memory. Restart to continue. Driver report: {driver}"
    )
}

pub(crate) fn fatal_runtime_exit(error: &str) -> Option<AppExit> {
    (!error.is_empty()).then(AppExit::error)
}

pub(crate) fn window_close_exit(requested: bool) -> Option<AppExit> {
    requested.then_some(AppExit::Success)
}

#[cfg(not(target_arch = "wasm32"))]
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
    fatal_error: Res<FatalError>,
    mut network: ResMut<NetworkHandle<PackApplication>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(exit_status) = fatal_error.0.as_deref().and_then(fatal_runtime_exit) else {
        return;
    };
    network.shutdown();
    exit.write(exit_status);
}

#[cfg(feature = "acceptance")]
#[derive(SystemParam)]
pub(crate) struct MovementTerminalEvidence<'w> {
    movement: Res<'w, MovementTicker>,
    identity_source: Option<Res<'w, MovementEvidenceIdentitySource>>,
    evidence: ResMut<'w, MovementEvidenceEmitter>,
}

/// Converts terminal observations into the ordinary network stop and app-exit commands.
#[cfg(feature = "acceptance")]
pub(crate) fn finish_acceptance_run(
    #[cfg(feature = "acceptance")] mut acceptance: ResMut<AcceptanceRun>,
    fatal_error: Res<FatalError>,
    mut metrics: ResMut<AppMetrics>,
    transparent_sort: Res<TransparentSortMetrics>,
    terminal_evidence: MovementTerminalEvidence,
    mut network: ResMut<NetworkHandle<PackApplication>>,
    mut exit: MessageWriter<AppExit>,
) {
    let MovementTerminalEvidence {
        movement,
        identity_source,
        mut evidence,
    } = terminal_evidence;
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
        fatal_error.0.as_deref(),
        &mut metrics.0,
        TransparentSortMetricsSnapshot::from(transparent_sort.snapshot()),
        terminal,
        &mut evidence,
    ) {
        network.shutdown();
        exit.write(status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::Update;

    fn failing_app(cause: &str, exit: AppExit) -> App {
        let mut app = App::new();
        app.insert_resource(FatalError(Some(cause.into())))
            .add_systems(Update, move |mut exits: MessageWriter<AppExit>| {
                exits.write(exit.clone());
            });
        app
    }

    /// `App::run` consumes the world holding the error, so its cause must be captured first.
    #[test]
    fn error_exit_names_its_cause_after_the_app_is_consumed() {
        let cause = "world FIFO rejected LevelChunk: truncated framing";
        let mut app = failing_app(cause, AppExit::error());
        let terminal = TerminalCause::install(&mut app);
        assert!(app.run().is_error());
        assert!(app.world().get_resource::<FatalError>().is_none());
        assert!(terminal.exit_message().contains(cause));
    }

    /// A later failure during teardown must not replace the one that ended the app.
    #[test]
    fn first_terminal_cause_survives_later_world_failures() {
        let first = "failed to encode PlayerAuthInput";
        let mut app = failing_app(first, AppExit::error());
        let terminal = TerminalCause::install(&mut app);
        app.set_runner(|mut owned| {
            owned.finish();
            owned.cleanup();
            owned.update();
            owned.world_mut().resource_mut::<FatalError>().0 =
                Some("network command channel is closed".into());
            owned.update();
            owned.should_exit().expect("terminal updates request exit")
        });
        assert!(app.run().is_error());
        assert!(terminal.exit_message().ends_with(first));
    }

    /// A lost GPU device must end the app with the driver's reason, not a bare error exit.
    #[test]
    fn device_loss_exits_with_the_driver_reason() {
        let mut app = App::new();
        app.init_resource::<render::DeviceLoss>()
            .add_systems(Update, exit_on_device_loss);
        let terminal = TerminalCause::install(&mut app);
        app.update();
        assert!(app.should_exit().is_none());

        app.world_mut()
            .resource_mut::<render::DeviceLoss>()
            .record("DXGI_ERROR_DEVICE_REMOVED");
        assert!(app.run().is_error());
        let message = terminal.exit_message();
        assert!(
            message.contains("graphics device stopped working"),
            "{message}"
        );
        assert!(message.ends_with("DXGI_ERROR_DEVICE_REMOVED"), "{message}");
    }

    #[test]
    fn successful_exit_does_not_capture_a_stale_failure() {
        let mut app = failing_app("a stale session failure", AppExit::Success);
        let terminal = TerminalCause::install(&mut app);
        assert!(app.run().is_success());
        assert!(!terminal.exit_message().contains("stale"));
    }
}
