//! Per-phase wall time of one StartGame bootstrap, logged once so a join hitch inside
//! `network_ingestion` names the phase that spent it.

use std::time::{Duration, Instant};

/// Phases of the bootstrap in the order it runs them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BootstrapPhase {
    /// Session reset, UI session and inventory publication.
    Session,
    /// Collision registration of the server's custom blocks.
    CustomBlocks,
    /// The session's block carrier and chunk texture installation.
    BlockAssets,
    /// The world stream and its session tables.
    WorldStream,
    /// Buffered equipment routed into the new stream.
    Equipment,
    /// Server language, icons, UI, glyphs, sounds and abilities.
    Presentation,
}

const PHASES: usize = 6;

/// Wall time per phase since the bootstrap started.
pub(super) struct BootstrapTimings {
    started: Instant,
    last: Instant,
    spent: [Duration; PHASES],
}

impl BootstrapTimings {
    pub(super) fn start() -> Self {
        let now = Instant::now();
        Self {
            started: now,
            last: now,
            spent: [Duration::ZERO; PHASES],
        }
    }

    /// Charges the time since the previous mark to `phase`.
    pub(super) fn mark(&mut self, phase: BootstrapPhase) {
        let now = Instant::now();
        self.spent[phase as usize] += now.saturating_duration_since(self.last);
        self.last = now;
    }

    /// Logs the total and each phase in milliseconds.
    pub(super) fn log(&self) {
        let ms = |duration: Duration| duration.as_secs_f64() * 1e3;
        let [
            session,
            custom_blocks,
            block_assets,
            world_stream,
            equipment,
            presentation,
        ] = self.spent.map(ms);
        bevy::log::info!(
            total_ms = ms(self.last.saturating_duration_since(self.started)),
            session,
            custom_blocks,
            block_assets,
            world_stream,
            equipment,
            presentation,
            "StartGame bootstrap applied"
        );
    }

    #[cfg(test)]
    fn spent(&self, phase: BootstrapPhase) -> Duration {
        self.spent[phase as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each mark charges only its own interval, so the phases add up to the total.
    #[test]
    fn marks_partition_the_bootstrap() {
        let mut timings = BootstrapTimings::start();
        timings.mark(BootstrapPhase::Session);
        timings.mark(BootstrapPhase::CustomBlocks);
        timings.mark(BootstrapPhase::Presentation);
        let total: Duration = timings.spent.iter().sum();
        assert_eq!(
            total,
            timings.last.saturating_duration_since(timings.started)
        );
        assert_eq!(timings.spent(BootstrapPhase::Equipment), Duration::ZERO);
    }
}
