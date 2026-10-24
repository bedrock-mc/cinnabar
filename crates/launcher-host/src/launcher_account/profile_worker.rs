//! Profile polling is independent of Home, catalogs and impression reporting.

use super::*;

mod active;
mod source;
pub(super) use source::Source;

/// The original finite account-read budget, separate from optional artwork.
pub(super) const RESPONSE_TIMEOUT: Duration = Duration::from_secs(60);

/// The single coalesced profile wake is also its one-way retirement signal.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Wake {
    Refresh,
    Stop,
}

/// Creates the existing worker's bounded latest wake, initially requesting a profile.
pub(super) fn channel() -> (
    tokio::sync::watch::Sender<Wake>,
    tokio::sync::watch::Receiver<Wake>,
) {
    tokio::sync::watch::channel(Wake::Refresh)
}

/// Coalesces a refresh without allowing a late account callback to restart a retired worker.
pub(super) fn refresh(sender: &tokio::sync::watch::Sender<Wake>) -> bool {
    !sender.is_closed() && sender.send_if_modified(|wake| *wake != Wake::Stop)
}

/// Retires active reads and future refreshes without waiting on the frame thread.
pub(super) fn stop(sender: &tokio::sync::watch::Sender<Wake>) {
    sender.send_replace(Wake::Stop);
}

struct Selection {
    generation: u64,
    source: Source,
}

impl Selection {
    /// Captures the presentation generation and its selected request source together.
    fn capture(shared: &Mutex<Snapshot>) -> Self {
        let snapshot = shared.lock().unwrap_or_else(|poison| poison.into_inner());
        Self {
            generation: snapshot.auth_generation,
            source: snapshot.profile_source.clone(),
        }
    }

    /// Rejects old presentation without file access or a credential acquisition.
    fn current(&self, snapshot: &Snapshot) -> bool {
        snapshot.auth_generation == self.generation
            && self.source.same_account(&snapshot.profile_source)
    }
}

/// Publishes only the selected account's bounded result or unavailable state.
pub(super) fn poll(
    artwork_dir: &std::path::Path,
    shared: &Mutex<Snapshot>,
    requests: tokio::sync::watch::Receiver<Wake>,
    runtime: &Handle,
) {
    poll_with_timeout(artwork_dir, shared, requests, RESPONSE_TIMEOUT, runtime);
}

/// Uses the same request source and publication rules with a shorter deadline in fixtures.
fn poll_with_timeout(
    artwork_dir: &std::path::Path,
    shared: &Mutex<Snapshot>,
    mut requests: tokio::sync::watch::Receiver<Wake>,
    timeout: Duration,
    runtime: &Handle,
) {
    let images = artwork::feed_images(artwork_dir);
    let mut last_log = None;
    loop {
        if *requests.borrow_and_update() == Wake::Stop {
            return;
        }
        let selection = Selection::capture(shared);
        let started = Instant::now();
        let logging =
            last_log.is_none_or(|last: Instant| started.duration_since(last) >= FEED_RETRY);
        if logging {
            last_log = Some(started);
            tracing::info!(
                request = "profile.v1",
                outcome = "started",
                "profile request"
            );
        }
        let completed = runtime.block_on(active::run(
            &images,
            shared,
            &selection,
            &mut requests,
            timeout,
        ));
        let (profile, mut outcome, queued_refresh) = match completed {
            active::Outcome::Stopped => return,
            active::Outcome::Retired => continue,
            active::Outcome::Complete(result, mut queued) => {
                let outcome = match &result {
                    Ok(profile) if profile.partial() => "partial",
                    Ok(_) => "success",
                    Err(active::Failure::Unavailable) => "unavailable",
                    Err(active::Failure::TimedOut) => "timeout",
                };
                let profile = match result {
                    Ok(profile) => match runtime.block_on(active::wait(
                        profile.into_current(),
                        shared,
                        &selection,
                        &mut requests,
                        None,
                        &mut queued,
                    )) {
                        Ok(profile) => profile,
                        Err(active::Exit::Stopped) => return,
                        Err(active::Exit::Retired) => continue,
                        Err(active::Exit::TimedOut | active::Exit::Unavailable) => Err(()),
                    },
                    Err(_) => Err(()),
                };
                (profile, outcome, queued)
            }
        };
        if *requests.borrow() == Wake::Stop {
            return;
        }
        let failed = profile.is_err();
        if failed && matches!(outcome, "success" | "partial") {
            outcome = "retired_profile";
        }
        let current = {
            let mut snapshot = shared.lock().unwrap_or_else(|poison| poison.into_inner());
            let current = selection.current(&snapshot);
            if current {
                snapshot.profile = Some(profile);
            }
            current
        };
        if logging {
            tracing::info!(
                request = "profile.v1",
                outcome = if current { outcome } else { "retired_account" },
                elapsed_ms = started.elapsed().as_millis() as u64,
                "profile request"
            );
        }
        if !current || queued_refresh {
            continue;
        }
        runtime.block_on(async {
            match idle_wait(failed) {
                Some(wait) => tokio::select! {
                    _ = requests.changed() => {},
                    _ = tokio::time::sleep(wait) => {},
                },
                None => {
                    let _ = requests.changed().await;
                }
            }
        });
    }
}

/// How long Profile idles before reloading on its own; a loaded Profile waits for a request.
fn idle_wait(failed: bool) -> Option<Duration> {
    failed.then_some(FEED_RETRY)
}

/// Limits missing-worker diagnostics even when Retry is clicked repeatedly.
pub fn log_unavailable(outcome: &'static str) {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|poison| poison.into_inner());
    let now = Instant::now();
    if last.is_none_or(|last| now.duration_since(last) >= FEED_RETRY) {
        *last = Some(now);
        tracing::warn!(request = "profile.v1", outcome, "profile request");
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// A loaded Profile reloads only when opened, retried or the account changes, not on a timer.
    #[test]
    fn loaded_profile_waits_for_a_request() {
        assert_eq!(idle_wait(false), None);
        assert_eq!(idle_wait(true), Some(FEED_RETRY));
    }
}
