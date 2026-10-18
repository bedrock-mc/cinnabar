//! Download measurements and formatting shared by pack and mod progress.

use std::time::{Duration, Instant};

pub(crate) const MIB: u64 = 1 << 20;
pub(crate) const GIB: u64 = 1 << 30;

/// Minimum download time before the rate steadies enough to show.
const DOWNLOAD_RATE_MIN_ELAPSED: Duration = Duration::from_millis(500);
/// How far back the rate averages, so a stall fades instead of sticking.
const DOWNLOAD_WINDOW: Duration = Duration::from_secs(3);
/// Minimum spacing between samples, independent of rendering frequency.
const DOWNLOAD_SAMPLE_INTERVAL: Duration = Duration::from_millis(250);
/// Sample capacity, with room for the full averaging window.
const DOWNLOAD_SAMPLES: usize = 16;

/// Rolling average shared by pack and mod downloads; callers supply monotonic sample times.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DownloadTracker {
    samples: [Option<(u64, Instant)>; DOWNLOAD_SAMPLES],
    len: usize,
    rate_bytes_per_sec: Option<u64>,
    eta_secs: Option<u64>,
}

impl DownloadTracker {
    /// Clears the sample history and estimates for a new download.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Samples download progress at a bounded rate and updates its estimates.
    pub fn observe(&mut self, received_bytes: u64, total_bytes: u64, now: Instant) {
        if self.len > 0 {
            let (last_bytes, last_time) = self.samples[self.len - 1].expect("tracked sample");
            if received_bytes < last_bytes {
                // A retry restarts its bytes, so the old window no longer applies.
                self.reset();
            } else if now.checked_duration_since(last_time).unwrap_or_default()
                < DOWNLOAD_SAMPLE_INTERVAL
            {
                return;
            }
        }
        if self.len < DOWNLOAD_SAMPLES {
            self.samples[self.len] = Some((received_bytes, now));
            self.len += 1;
        } else {
            self.samples.copy_within(1..DOWNLOAD_SAMPLES, 0);
            self.samples[DOWNLOAD_SAMPLES - 1] = Some((received_bytes, now));
        }
        // Keep only the trailing window so a stall ages out instead of sticking.
        while self.len > 1 {
            let (_, oldest_time) = self.samples[0].expect("tracked sample");
            if now.checked_duration_since(oldest_time).unwrap_or_default() <= DOWNLOAD_WINDOW {
                break;
            }
            self.samples.copy_within(1..self.len, 0);
            self.samples[self.len - 1] = None;
            self.len -= 1;
        }
        let (oldest_bytes, oldest_time) = self.samples[0].expect("tracked sample");
        let (newest_bytes, newest_time) = self.samples[self.len - 1].expect("tracked sample");
        let elapsed = newest_time
            .checked_duration_since(oldest_time)
            .unwrap_or_default();
        if elapsed < DOWNLOAD_RATE_MIN_ELAPSED {
            self.rate_bytes_per_sec = None;
            self.eta_secs = None;
            return;
        }
        let elapsed_secs = elapsed.as_secs_f64();
        if !elapsed_secs.is_finite() || elapsed_secs <= 0.0 {
            return;
        }
        let delta = newest_bytes.saturating_sub(oldest_bytes);
        if delta == 0 {
            self.rate_bytes_per_sec = None;
            self.eta_secs = None;
            return;
        }
        let rate = (delta as f64 / elapsed_secs) as u64;
        if rate == 0 {
            self.rate_bytes_per_sec = None;
            self.eta_secs = None;
            return;
        }
        self.rate_bytes_per_sec = Some(rate);
        let remaining = total_bytes.saturating_sub(received_bytes);
        self.eta_secs = Some(remaining.div_ceil(rate));
    }
}

impl DownloadTracker {
    /// Smoothed bytes per second once the sample window is stable.
    pub fn bytes_per_sec(&self) -> Option<u64> {
        self.rate_bytes_per_sec
    }
    /// Estimated whole seconds until all advertised bytes arrive.
    pub fn eta_secs(&self) -> Option<u64> {
        self.eta_secs
    }
}

/// Vanilla file sizes: MB to two places under 1 MiB and one above, GB from 1 GiB.
pub fn file_size(bytes: u64, tr: &impl Fn(&str, &str) -> String) -> String {
    let megabytes = || tr("playscreen.fileSize.MB", "MB");
    if bytes < MIB {
        format!("{:.2}{}", bytes as f64 / MIB as f64, megabytes())
    } else if bytes < GIB {
        format!("{:.1}{}", bytes as f64 / MIB as f64, megabytes())
    } else {
        let gigabytes = tr("playscreen.fileSize.GB", "GB");
        format!("{:.1}{gigabytes}", bytes as f64 / GIB as f64)
    }
}

/// Short ETA for the download line: seconds, minutes and seconds, or hours and minutes.
pub fn format_eta(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
    }
}
