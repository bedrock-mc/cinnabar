use super::*;
use futures::future::{pending, ready};

/// Retains a live control generation until its cancellation sender or guard is dropped.
fn lease_guard() -> (oneshot::Sender<()>, Guard, oneshot::Receiver<()>) {
    let (cancel, cancelled) = oneshot::channel();
    let (ended, finished) = oneshot::channel();
    (
        cancel,
        Guard {
            _ended: ended,
            cancelled,
        },
        finished,
    )
}

#[test]
fn writer_failure_keeps_session_alive_for_terminal_recovery() {
    for terminal in ["transfer", "server disconnect reason"] {
        let (_cancel, mut guard, mut finished) = lease_guard();
        let (stream, peer) = tokio::io::duplex(64);
        drop(peer);
        let (read, write) = tokio::io::split(stream);
        let (_, frames, writer) = bridge::tasks::prepare_connection(read, write, 64);
        let (boundary, reached) = oneshot::channel();
        let (publish, event) = oneshot::channel();
        let session = async move {
            assert!(frames.send(vec![1].into()).await.is_err());
            publish.send(reached.await.unwrap()).unwrap();
        };
        let mut run = Box::pin(guard.run(session, writer));
        assert!(run.as_mut().now_or_never().is_none());
        assert!(finished.try_recv().unwrap().is_none());
        boundary.send(terminal).unwrap();
        assert_eq!(run.as_mut().now_or_never(), Some(true));
        assert_eq!(event.now_or_never(), Some(Ok(terminal)));
        drop(run);
        drop(guard);
        assert!(finished.now_or_never().unwrap().is_err());
    }
}

#[test]
fn cancellation_still_stops_recovery_after_the_writer_exits() {
    let (cancel, mut guard, _finished) = lease_guard();
    let (owned, released) = oneshot::channel::<()>();
    let session = async move {
        let _owned = owned;
        pending::<()>().await;
    };
    let mut run = Box::pin(guard.run(session, ready(())));
    assert!(run.as_mut().now_or_never().is_none());
    cancel.send(()).unwrap();
    assert_eq!(run.as_mut().now_or_never(), Some(false));
    assert!(released.now_or_never().unwrap().is_err());
}

#[test]
fn session_completion_drops_a_stalled_writer() {
    let (_cancel, mut guard, _finished) = lease_guard();
    let (owned, released) = oneshot::channel::<()>();
    let writer = async move {
        let _owned = owned;
        pending::<()>().await;
    };
    assert_eq!(guard.run(ready(()), writer).now_or_never(), Some(true));
    assert!(released.now_or_never().unwrap().is_err());
}

#[test]
fn cancellation_wins_over_a_ready_session() {
    let (cancel, mut guard, _finished) = lease_guard();
    cancel.send(()).unwrap();
    assert_eq!(guard.run(ready(()), pending()).now_or_never(), Some(false));
}
