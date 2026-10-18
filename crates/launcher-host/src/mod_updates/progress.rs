//! Latest-value transfer observations; the UI never queues one message per download chunk.

use launcher::{
    download::DownloadTracker,
    mods::{UpdatePhase, UpdateProgress},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};

#[derive(Default)]
struct Transfer {
    revision: u64,
    shown: Option<UpdateProgress>,
    rate: DownloadTracker,
}

/// Shared edge-owned byte counters with caller-timed, pack-compatible rate estimates.
#[derive(Clone, Default)]
pub(super) struct Progress(Arc<Mutex<HashMap<u64, Transfer>>>);

impl Progress {
    /// Replaces one transfer's current counters without allocating an event for every chunk.
    pub fn set(&self, id: u64, revision: u64, phase: UpdatePhase, received: u64, total: u64) {
        let mut transfers = self.0.lock().expect("mod transfer progress");
        let transfer = transfers.entry(id).or_default();
        if transfer.revision != revision {
            *transfer = Transfer {
                revision,
                ..Default::default()
            };
        }
        let mut shown = UpdateProgress::stage(id, phase);
        shown.received_bytes = received.min(total);
        shown.total_bytes = total;
        transfer.shown = Some(shown);
    }

    /// Measures the current revision at the caller's monotonic time, including stalls.
    pub fn snapshot(&self, revision: u64, now: Instant) -> Vec<UpdateProgress> {
        let mut transfers = self.0.lock().expect("mod transfer progress");
        let mut shown = Vec::new();
        for transfer in transfers
            .values_mut()
            .filter(|transfer| transfer.revision == revision)
        {
            let Some(mut progress) = transfer.shown.clone() else {
                continue;
            };
            if progress.phase == UpdatePhase::Downloading {
                transfer
                    .rate
                    .observe(progress.received_bytes, progress.total_bytes, now);
                progress.bytes_per_sec = transfer.rate.bytes_per_sec();
                progress.eta_secs = transfer.rate.eta_secs();
            }
            shown.push(progress);
        }
        shown.sort_by_key(|progress| progress.id);
        shown
    }

    /// Removes a completed or failed transfer, without deleting a newer attempt's progress.
    pub fn finish(&self, id: u64, revision: u64) {
        let mut transfers = self.0.lock().expect("mod transfer progress");
        if transfers
            .get(&id)
            .is_some_and(|transfer| transfer.revision == revision)
        {
            transfers.remove(&id);
        }
    }

    /// Retires counters and rate windows when the check set changes.
    pub fn clear(&self) {
        self.0.lock().expect("mod transfer progress").clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn mod_rates_use_pack_windows_and_ignore_superseded_transfers() {
        let progress = Progress::default();
        let start = Instant::now();
        progress.set(1, 2, UpdatePhase::Downloading, 0, 1000);
        assert_eq!(progress.snapshot(2, start)[0].bytes_per_sec, None);
        progress.set(1, 2, UpdatePhase::Downloading, 500, 1000);
        let shown = progress.snapshot(2, start + Duration::from_secs(1));
        assert_eq!(shown[0].bytes_per_sec, Some(500));
        assert_eq!(shown[0].eta_secs, Some(1));
        assert!(progress.snapshot(1, start).is_empty());
        progress.finish(1, 1);
        assert_eq!(
            progress.snapshot(2, start + Duration::from_secs(2)).len(),
            1
        );
        progress.set(1, 2, UpdatePhase::Verifying, 1000, 1000);
        assert_eq!(
            progress.snapshot(2, start + Duration::from_secs(3))[0].bytes_per_sec,
            None
        );
        progress.finish(1, 2);
        assert!(progress.snapshot(2, start).is_empty());
    }
}
