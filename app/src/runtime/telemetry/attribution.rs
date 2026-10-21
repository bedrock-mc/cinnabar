//! Keeps attribution metrics current while bounding the diagnostic log volume.

use std::time::Duration;
use web_time::Instant;

use diagnostics::{
    metrics::{DiagnosticQuadTracker, MetricsCollector},
    rate::LogBudget,
};

const LOG_INTERVAL: Duration = Duration::from_secs(5);

pub(super) struct DiagnosticAttributionLogState {
    budget: LogBudget,
    pending: Option<String>,
}

impl Default for DiagnosticAttributionLogState {
    fn default() -> Self {
        Self {
            budget: LogBudget::new(LOG_INTERVAL),
            pending: None,
        }
    }
}

impl DiagnosticAttributionLogState {
    /// Retains the latest changed snapshot and emits it at most once every five seconds.
    pub(super) fn take(&mut self, now: Instant, fresh: Option<String>) -> Option<String> {
        if let Some(fresh) = fresh {
            self.pending = Some(fresh);
        }
        self.pending.take_if(|_| self.budget.admit(now).is_some())
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
