//! Terminal evidence decides when to request an ordinary runtime stop.
use crate::{
    AcceptanceExitDecision, AcceptanceRun, Phase3TerminalDrainDecision,
    TRANSPARENT_PRESENTATION_EXIT_GRACE,
    mutation::write_stdout_marker,
    phase3_evidence::{Phase3EvidenceEmitter, Phase3EvidenceIdentity},
};
use bevy::{log::error, prelude::AppExit};
use diagnostics::metrics::{MetricsCollector, TransparentSortMetricsSnapshot};
use std::time::Instant;

/// Immutable movement facts observed at the terminal evidence boundary.
pub struct TerminalMovementObservation {
    pub identity: Option<Phase3EvidenceIdentity>,
    pub source: &'static str,
    pub physics_packet_count: u64,
    pub free_camera_packet_count: u64,
    pub pending_count: usize,
    pub outbox_reconciliation: &'static str,
}

/// Writes final evidence and requests a stop; the caller owns the actual session teardown.
pub fn finish_acceptance_run(
    acceptance: &mut AcceptanceRun,
    fatal_error: Option<&str>,
    metrics: &mut MetricsCollector,
    transparent_snapshot: TransparentSortMetricsSnapshot,
    observe_terminal: impl FnOnce() -> TerminalMovementObservation,
    evidence: &mut Phase3EvidenceEmitter,
) -> Option<AppExit> {
    if acceptance.finished {
        return None;
    }
    let now = Instant::now();
    let fatal = fatal_error.is_some();
    if let Some(deadline) = acceptance.deadline.filter(|deadline| now >= *deadline) {
        metrics.finish_timed_session(deadline);
    }
    let decision = acceptance.exit_decision(now, fatal, transparent_snapshot);
    if matches!(
        decision,
        AcceptanceExitDecision::Continue | AcceptanceExitDecision::WaitForTransparentPresentation
    ) {
        return None;
    }

    let phase3 = observe_terminal();
    let drain_decision = if fatal {
        Phase3TerminalDrainDecision::Drained
    } else {
        acceptance.phase3_terminal_drain_decision(
            now,
            phase3
                .identity
                .as_ref()
                .is_some_and(|identity| identity.candidate_physics()),
            phase3.pending_count,
        )
    };
    if drain_decision == Phase3TerminalDrainDecision::Wait {
        return None;
    }
    let phase3_drain_timed_out = drain_decision == Phase3TerminalDrainDecision::TimedOut;

    acceptance.finished = true;
    if let Some(identity) = phase3.identity {
        let markers = evidence.observe_terminal(
            identity,
            phase3.source,
            phase3.physics_packet_count,
            phase3.free_camera_packet_count,
            phase3.pending_count,
            phase3.outbox_reconciliation,
        );
        let mut stdout = diagnostics::console::stdout();
        for marker in markers {
            write_stdout_marker(&mut stdout, &marker);
        }
    }
    metrics.record_transparent_sort_snapshot(transparent_snapshot);
    let mut output_failed = false;
    if let Some(path) = &acceptance.metrics_out
        && let Err(error) = metrics.report().write_json(path)
    {
        error!(
            "failed to write acceptance metrics to {}: {error}",
            path.display()
        );
        output_failed = true;
    }
    if let Some(error) = fatal_error {
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
    if phase3_drain_timed_out {
        error!(
            "Phase 3 terminal movement acknowledgement drain timed out after {:.3}s: pending={} reconciliation={}",
            TRANSPARENT_PRESENTATION_EXIT_GRACE.as_secs_f64(),
            phase3.pending_count,
            phase3.outbox_reconciliation,
        );
    }
    Some(
        if decision.is_error() || output_failed || phase3_drain_timed_out {
            AppExit::error()
        } else {
            AppExit::Success
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_observation_is_lazy_and_stop_is_requested_once() {
        let mut run = AcceptanceRun::new(None, None, false, false);
        let mut metrics = MetricsCollector::new();
        let mut evidence = Phase3EvidenceEmitter::default();
        assert!(
            finish_acceptance_run(
                &mut run,
                None,
                &mut metrics,
                TransparentSortMetricsSnapshot::default(),
                || panic!("ordinary play must not capture terminal evidence"),
                &mut evidence,
            )
            .is_none()
        );
        run.request_shutdown();
        assert_eq!(
            finish_acceptance_run(
                &mut run,
                None,
                &mut metrics,
                TransparentSortMetricsSnapshot::default(),
                || TerminalMovementObservation {
                    identity: None,
                    source: "FreeCamera",
                    physics_packet_count: 0,
                    free_camera_packet_count: 0,
                    pending_count: 0,
                    outbox_reconciliation: "NotAuthoritative",
                },
                &mut evidence,
            ),
            Some(AppExit::Success)
        );
        assert!(
            finish_acceptance_run(
                &mut run,
                None,
                &mut metrics,
                TransparentSortMetricsSnapshot::default(),
                || panic!("completed evidence must not request another stop"),
                &mut evidence,
            )
            .is_none()
        );
    }
}
