//! Keeps attribution metrics current while bounding the diagnostic log volume.

use std::time::{Duration, Instant};

use diagnostics::metrics::{DiagnosticQuadTracker, MetricsCollector};

const LOG_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Default)]
pub(super) struct DiagnosticAttributionLogState {
    last_emitted: Option<Instant>,
    pending: Option<String>,
}

impl DiagnosticAttributionLogState {
    /// Retains the latest changed snapshot and emits it at most once every five seconds.
    pub(super) fn take(&mut self, now: Instant, fresh: Option<String>) -> Option<String> {
        if let Some(fresh) = fresh {
            self.pending = Some(fresh);
        }
        if self
            .last_emitted
            .is_some_and(|last| now.duration_since(last) < LOG_INTERVAL)
        {
            return None;
        }
        let marker = self.pending.take()?;
        self.last_emitted = Some(now);
        Some(marker)
    }
}

/// Updates metrics for every changed resident snapshot, independently of log throttling.
pub(crate) fn refresh_diagnostic_attribution(
    last_revision: &mut u64,
    tracker: &DiagnosticQuadTracker,
    metrics: &mut MetricsCollector,
) -> Option<String> {
    let revision = tracker.revision();
    if *last_revision == revision {
        return None;
    }
    let snapshot = tracker.snapshot();
    let marker = format!("DIAGNOSTIC_GEOMETRY {}", snapshot.marker_fields());
    metrics.record_diagnostic_attribution(snapshot);
    *last_revision = revision;
    Some(marker)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_attribution_log_coalesces_changes_and_flushes_when_idle() {
        let mut log = DiagnosticAttributionLogState::default();
        let now = Instant::now();
        assert!(log.take(now, None).is_none());
        assert_eq!(log.take(now, Some("first".into())), Some("first".into()));
        assert!(
            log.take(now + Duration::from_secs(1), Some("old".into()))
                .is_none()
        );
        assert!(
            log.take(now + Duration::from_secs(2), Some("latest".into()))
                .is_none()
        );
        assert_eq!(log.take(now + LOG_INTERVAL, None), Some("latest".into()));
        assert!(log.take(now + LOG_INTERVAL * 2, None).is_none());
    }
}
