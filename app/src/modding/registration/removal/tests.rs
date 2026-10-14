use super::super::selection::forget;
use super::*;
use std::{fs, path::PathBuf, thread};

#[derive(Debug)]
struct PendingSave {
    path: PathBuf,
    started: Sender<()>,
    release: Receiver<()>,
}

impl Drop for PendingSave {
    fn drop(&mut self) {
        let _ = self.started.send(());
        if self.release.recv().is_ok() {
            fs::write(&self.path, b"pending preferences").unwrap();
        }
    }
}

/// Queues a final save whose completion the test controls with a channel gate.
fn pending_save(path: PathBuf) -> (Receiver<PendingSave>, Receiver<()>, Sender<()>) {
    let (retired, retiring) = bounded(1);
    let (started, started_save) = bounded(1);
    let (release, released) = bounded(1);
    retired
        .send(PendingSave {
            path,
            started,
            release: released,
        })
        .unwrap();
    (retiring, started_save, release)
}

/// Writes the exact disabled selection expected by a retirement fence.
fn receipt(directory: &Path) -> Receipt {
    forget(directory, &directory.join("zoom.wasm"), "remove".into()).unwrap()
}

/// Removal cannot delete a companion until a previously queued save has completed.
#[test]
fn completion_waits_for_the_retired_hosts_final_settings_write() {
    let directory = tempfile::tempdir().unwrap();
    let companion = directory.path().join("zoom.settings.json");
    let (retiring, started_save, release) = pending_save(companion.clone());
    let (complete, completion) = bounded(1);
    let fence = Fence {
        generation: 1,
        selection: receipt(directory.path()),
        complete,
    };
    let root = directory.path().to_owned();
    let worker = thread::spawn(move || finish(&retiring, fence, &root, &AtomicU64::new(1)));
    test_time::eventually("retired host's pending save starts", || {
        started_save.try_recv().is_ok()
    });
    assert!(completion.try_recv().is_err());
    release.send(()).unwrap();
    completion.recv().unwrap().unwrap();
    fs::remove_file(&companion).unwrap();
    worker.join().unwrap();
    assert!(!companion.exists());
}

/// Re-enabling a component during a final save invalidates the fence even before polling catches up.
#[test]
fn a_dispatched_fence_rejects_registration_changes_while_settings_drain() {
    use super::super::selection::select;

    for request_id in ["replacement", "remove"] {
        let directory = tempfile::tempdir().unwrap();
        let component = directory.path().join("zoom.wasm");
        let companion = directory.path().join("zoom.settings.json");
        fs::write(&component, b"component").unwrap();
        let (retiring, started, release) = pending_save(companion.clone());
        let (complete, completion) = bounded(1);
        let (fences, dispatched) = bounded(1);
        let mut pending = Pending::default();
        pending.wait(1, receipt(directory.path()), complete, &fences);
        pending.applied(1, "remove".into(), &fences);
        let fence = dispatched.try_recv().unwrap();
        let root = directory.path().to_owned();
        let worker = thread::spawn(move || finish(&retiring, fence, &root, &AtomicU64::new(1)));
        test_time::eventually("dispatched retirement starts saving", || {
            started.try_recv().is_ok()
        });
        select(directory.path(), &component, true, request_id.into()).unwrap();
        pending.observe(Some(request_id));
        release.send(()).unwrap();
        assert!(completion.recv().unwrap().is_err());
        worker.join().unwrap();
        assert!(component.exists());
        assert_eq!(fs::read(companion).unwrap(), b"pending preferences");
    }
}

/// An old applied generation cannot approve deletion even when the registration bytes match.
#[test]
fn a_dispatched_fence_rejects_a_newer_generation() {
    let directory = tempfile::tempdir().unwrap();
    let (complete, completion) = bounded(1);
    let fence = Fence {
        generation: 1,
        selection: receipt(directory.path()),
        complete,
    };
    let (_, retiring) = bounded::<()>(1);
    finish(&retiring, fence, directory.path(), &AtomicU64::new(2));
    assert!(completion.recv().unwrap().is_err());
}

/// A retirement reply does not authorize deletion after the watcher has moved on.
#[test]
fn a_retirement_permit_rejects_a_generation_change_after_completion() {
    let latest = Arc::new(AtomicU64::new(1));
    let permit = Permit::new(1, Arc::clone(&latest), Arc::new(Mutex::new(Vec::new())));
    permit.confirm().unwrap();
    latest.store(2, Ordering::Release);
    assert!(permit.confirm().is_err());
}

/// A queued removal waits for full frame detachment, not just an early disabled notice.
#[test]
fn a_removal_waits_for_its_matching_applied_selection() {
    let directory = tempfile::tempdir().unwrap();
    let (fences, waiting_fence) = bounded(1);
    let (complete, completion) = bounded(1);
    let mut pending = Pending::default();
    pending.wait(1, receipt(directory.path()), complete, &fences);
    pending.observe(Some("remove"));
    assert!(waiting_fence.try_recv().is_err());
    assert!(completion.try_recv().is_err());
    pending.applied(1, "remove".into(), &fences);
    let (_, retiring) = bounded::<()>(1);
    finish(
        &retiring,
        waiting_fence.try_recv().unwrap(),
        directory.path(),
        &AtomicU64::new(1),
    );
    completion.recv().unwrap().unwrap();
}

/// An applied selection remains usable when its acknowledgment arrives before the request.
#[test]
fn a_late_removal_wait_uses_only_the_current_generation() {
    let directory = tempfile::tempdir().unwrap();
    let (fences, waiting_fence) = bounded(1);
    let mut pending = Pending::default();
    pending.applied(1, "remove".into(), &fences);
    let (complete, completion) = bounded(1);
    pending.wait(2, receipt(directory.path()), complete, &fences);
    assert!(waiting_fence.try_recv().is_err());
    pending.applied(2, "remove".into(), &fences);
    let (_, retiring) = bounded::<()>(1);
    finish(
        &retiring,
        waiting_fence.try_recv().unwrap(),
        directory.path(),
        &AtomicU64::new(2),
    );
    completion.recv().unwrap().unwrap();
    let (complete, completion) = bounded(1);
    pending.wait(2, receipt(directory.path()), complete, &fences);
    finish(
        &retiring,
        waiting_fence.try_recv().unwrap(),
        directory.path(),
        &AtomicU64::new(2),
    );
    completion.recv().unwrap().unwrap();
}

/// Superseded selections fail removal instead of leaving a worker waiting indefinitely.
#[test]
fn a_superseded_removal_reports_failure_without_a_retirement_fence() {
    let directory = tempfile::tempdir().unwrap();
    let (fences, waiting_fence) = bounded(1);
    let (complete, completion) = bounded(1);
    let mut pending = Pending::default();
    pending.wait(1, receipt(directory.path()), complete, &fences);
    pending.observe(Some("replacement"));
    assert!(completion.recv().unwrap().is_err());
    assert!(waiting_fence.try_recv().is_err());
}
