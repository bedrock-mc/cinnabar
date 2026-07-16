use crate::*;

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
    network: Option<ResMut<NetworkHandle>>,
    watchdog: Res<ShutdownWatchdog>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(exit_status) = window_close_exit(close_requests.read().next().is_some()) else {
        return;
    };
    begin_bounded_shutdown(&watchdog, &exit_status);
    if let Some(mut network) = network {
        network.shutdown();
    }
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

pub(crate) fn finish_acceptance_run(
    mut acceptance: ResMut<AcceptanceRun>,
    client_world: Res<ClientWorld>,
    mut metrics: ResMut<AppMetrics>,
    transparent_sort: Res<TransparentSortMetrics>,
    mut network: ResMut<NetworkHandle>,
    mut exit: MessageWriter<AppExit>,
) {
    if acceptance.finished {
        return;
    }
    let now = Instant::now();
    let fatal = client_world.fatal_error.is_some();
    if let Some(deadline) = acceptance.deadline.filter(|deadline| now >= *deadline) {
        metrics.0.finish_timed_session(deadline);
    }
    let transparent_snapshot = TransparentSortMetricsSnapshot::from(transparent_sort.snapshot());
    let decision = acceptance.exit_decision(now, fatal, transparent_snapshot);
    if matches!(
        decision,
        AcceptanceExitDecision::Continue | AcceptanceExitDecision::WaitForTransparentPresentation
    ) {
        return;
    }

    acceptance.finished = true;
    metrics
        .0
        .record_transparent_sort_snapshot(transparent_snapshot);
    let mut output_failed = false;
    if let Some(path) = &acceptance.metrics_out
        && let Err(error) = metrics.0.report().write_json(path)
    {
        error!(
            "failed to write acceptance metrics to {}: {error}",
            path.display()
        );
        output_failed = true;
    }
    if let Some(error) = &client_world.fatal_error {
        error!("{error}");
    }
    if decision == AcceptanceExitDecision::TransparentPresentationTimedOut {
        error!(
            "transparent presentation did not settle within {:.3}s after the timed session: committed={} encoded={} presented={} ref_count={}",
            TRANSPARENT_PRESENTATION_EXIT_GRACE.as_secs_f64(),
            transparent_snapshot.committed_generation,
            transparent_snapshot.encoded_generation,
            transparent_snapshot.presented_generation,
            transparent_snapshot.ref_count,
        );
    }
    network.shutdown();
    exit.write(if decision.is_error() || output_failed {
        AppExit::error()
    } else {
        AppExit::Success
    });
}
