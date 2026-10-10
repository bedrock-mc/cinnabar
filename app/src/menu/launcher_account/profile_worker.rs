//! Profile polling is independent of Home, catalogs and impression reporting.

use super::*;

/// A local RPC guard, longer than the core's service deadline and response write allowance.
/// This is transport recovery, not a vanilla animation or service timeout constant.
pub(super) const RESPONSE_TIMEOUT: Duration = Duration::from_secs(60);

/// Publishes a terminal result even if a control endpoint accepts but never answers.
pub(super) fn poll(
    socket_dir: &std::path::Path,
    artwork_dir: &std::path::Path,
    shared: &Mutex<Snapshot>,
    stop: &Receiver<()>,
    requests: &Receiver<()>,
) {
    poll_with_timeout(
        socket_dir,
        artwork_dir,
        shared,
        stop,
        requests,
        RESPONSE_TIMEOUT,
    );
}

/// Uses the same socket path and publication rules with a shorter deadline in fixtures.
fn poll_with_timeout(
    socket_dir: &std::path::Path,
    artwork_dir: &std::path::Path,
    shared: &Mutex<Snapshot>,
    stop: &Receiver<()>,
    requests: &Receiver<()>,
    timeout: Duration,
) {
    let Some(runtime) = runtime() else {
        publish(shared, |snapshot| snapshot.profile = Some(Err(())));
        log_unavailable("runtime_unavailable");
        return;
    };
    let images = artwork::feed_images(artwork_dir);
    let mut last_log = None;
    loop {
        while requests.try_recv().is_ok() {}
        let generation = auth_generation(shared);
        let started = Instant::now();
        let logging =
            last_log.is_none_or(|last: Instant| started.duration_since(last) >= FEED_RETRY);
        if logging {
            last_log = Some(started);
            bevy::log::info!(
                request = "profile.v1",
                outcome = "started",
                "profile request"
            );
        }
        let result = runtime.block_on(async {
            let mut result =
                tokio::time::timeout(timeout, launcher_control::profile(socket_dir)).await;
            if let Ok(Ok(profile)) = &mut result {
                artwork::fill(&images, artwork::profile_slots(profile)).await;
            }
            result
        });
        let outcome = match &result {
            Ok(Ok(_)) => "success",
            Ok(Err(_)) => "unavailable",
            Err(_) => "timeout",
        };
        let failed = !matches!(result, Ok(Ok(_)));
        let profile = result.ok().and_then(Result::ok).ok_or(());
        let current = auth_generation(shared) == generation;
        publish_account(shared, generation, |snapshot| {
            snapshot.profile = Some(profile)
        });
        if logging {
            bevy::log::info!(
                request = "profile.v1",
                outcome = if current { outcome } else { "retired_account" },
                elapsed_ms = started.elapsed().as_millis() as u64,
                "profile request"
            );
        }
        let wake = match idle_wait(failed) {
            Some(wait) => crossbeam_channel::select! {
                recv(stop) -> _ => return,
                recv(requests) -> request => request.is_ok(),
                default(wait) => true,
            },
            None => crossbeam_channel::select! {
                recv(stop) -> _ => return,
                recv(requests) -> request => request.is_ok(),
            },
        };
        if !wake {
            return;
        }
    }
}

/// How long Profile idles before reloading on its own; a loaded Profile waits for a request.
fn idle_wait(failed: bool) -> Option<Duration> {
    failed.then_some(FEED_RETRY)
}

/// Limits missing-worker diagnostics even when Retry is clicked repeatedly.
pub(crate) fn log_unavailable(outcome: &'static str) {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|poison| poison.into_inner());
    let now = Instant::now();
    if last.is_none_or(|last| now.duration_since(last) >= FEED_RETRY) {
        *last = Some(now);
        bevy::log::warn!(request = "profile.v1", outcome, "profile request");
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{io::Read, os::unix::net::UnixListener};

    /// A loaded Profile reloads only when opened, retried or the account changes, not on a timer.
    #[test]
    fn loaded_profile_waits_for_a_request() {
        assert_eq!(idle_wait(false), None);
        assert_eq!(idle_wait(true), Some(FEED_RETRY));
    }

    #[test]
    fn profile_loading_ends_when_control_accepts_without_reply() {
        let dir = std::env::temp_dir().join(format!("profile-timeout-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let listener = UnixListener::bind(dir.join("control.sock")).unwrap();
        let fixture = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut size = [0; 4];
            stream.read_exact(&mut size).unwrap();
            let mut request = vec![0; u32::from_be_bytes(size) as usize];
            stream.read_exact(&mut request).unwrap();
            let request: serde_json::Value = serde_json::from_slice(&request).unwrap();
            assert_eq!(request["method"], "profile.v1");
            let mut byte = [0];
            assert_eq!(
                stream.read(&mut byte).unwrap(),
                0,
                "timeout must close the request"
            );
        });
        let shared = Arc::new(Mutex::new(Snapshot::default()));
        let (alive, stop) = bounded(0);
        let (_sender, requests) = bounded(1);
        let snapshot = Arc::clone(&shared);
        let endpoint = dir.clone();
        let worker = thread::spawn(move || {
            poll_with_timeout(
                &endpoint,
                &endpoint.join("artwork"),
                &snapshot,
                &stop,
                &requests,
                Duration::from_millis(100),
            )
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while shared.lock().unwrap().profile.is_none() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        drop(alive);
        worker.join().unwrap();
        fixture.join().unwrap();
        assert!(matches!(shared.lock().unwrap().profile, Some(Err(()))));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
