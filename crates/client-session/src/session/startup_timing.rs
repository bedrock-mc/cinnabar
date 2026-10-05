use std::time::Instant;

#[derive(Clone, Copy)]
pub(super) struct StartupTiming {
    session_generation: u64,
    started: Instant,
}

impl StartupTiming {
    pub(super) fn new(session_generation: u64) -> Self {
        Self {
            session_generation,
            started: Instant::now(),
        }
    }

    pub(super) fn record(self, stage: &'static str, started: Instant, succeeded: bool) {
        let now = Instant::now();
        tracing::info!(
            session_generation = self.session_generation,
            stage,
            succeeded,
            elapsed_ms = now.duration_since(started).as_secs_f64() * 1e3,
            total_elapsed_ms = now.duration_since(self.started).as_secs_f64() * 1e3,
            "session startup stage complete"
        );
    }
}
