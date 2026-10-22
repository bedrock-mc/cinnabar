//! Crash-window feedback and lifecycle behavior.

use super::*;

#[test]
fn manual_sending_restarts_after_a_finished_worker_and_keeps_active_workers() {
    let (release, waiting) = std::sync::mpsc::channel();
    let mut worker = Some(std::thread::spawn(move || waiting.recv().unwrap()));
    let started = std::cell::Cell::new(0);
    app::queue_upload(&mut worker, || {
        started.set(started.get() + 1);
        Some(std::thread::spawn(|| {}))
    });
    assert_eq!(
        started.get(),
        0,
        "an active send must not get another worker"
    );
    release.send(()).unwrap();
    assert!(test_time::wait_until(test_time::DEFAULT_TIMEOUT, || worker
        .as_ref()
        .unwrap()
        .is_finished()));
    app::queue_upload(&mut worker, || {
        started.set(started.get() + 1);
        Some(std::thread::spawn(|| {}))
    });
    worker.take().unwrap().join().unwrap();
    assert_eq!(
        started.get(),
        1,
        "a new manual send did not start after the first finished"
    );
}

#[test]
fn a_restart_is_selected_before_a_pending_upload_finishes() {
    let (release, waiting) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || waiting.recv().unwrap());
    let (selected, selection) = std::sync::mpsc::channel();
    let closing = std::thread::spawn(move || {
        finish_choice((), Choice::Restart, Some(worker), |choice| {
            selected.send(choice).unwrap()
        })
    });
    let observed = test_time::wait_until(test_time::DEFAULT_TIMEOUT, || {
        selection
            .try_recv()
            .is_ok_and(|choice| choice == Choice::Restart)
    });
    release.send(()).unwrap();
    assert_eq!(closing.join().unwrap(), Choice::Restart);
    assert!(
        observed,
        "restart selection waited for the pending network request"
    );
}

#[test]
fn a_closed_window_is_destroyed_before_the_selected_action_runs() {
    use std::{cell::Cell, rc::Rc};
    struct WindowOwner(Rc<Cell<bool>>);
    impl Drop for WindowOwner {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }
    let destroyed = Rc::new(Cell::new(false));
    let observed = Cell::new(false);
    finish_choice(
        WindowOwner(Rc::clone(&destroyed)),
        Choice::Close,
        None,
        |_| observed.set(destroyed.get()),
    );
    assert!(observed.get(), "the window survived until the upload wait");
}

#[test]
fn copy_feedback_expires_back_to_its_label() {
    let at = Instant::now();
    let mut screen = CrashScreen {
        expanded: false,
        queued: false,
        copied: Some((Copied::Done, at)),
    };
    assert_eq!(screen.label(CrashAction::CopyReport), "Copied");
    assert!(!screen.expire(at));
    assert!(screen.expire(at + COPY_FEEDBACK));
    assert_eq!(screen.label(CrashAction::CopyReport), "Copy report");
}
