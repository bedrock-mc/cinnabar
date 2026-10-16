use super::*;
use std::collections::VecDeque;

#[test]
fn aborted_and_reset_accepts_do_not_discard_the_next_loopback_connection() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    client.write_all(b"ready").unwrap();
    let mut errors = VecDeque::from([
        io::ErrorKind::Interrupted,
        io::ErrorKind::ConnectionAborted,
        io::ErrorKind::ConnectionReset,
        io::ErrorKind::ConnectionRefused,
    ]);
    let stopped = AtomicBool::new(false);
    let mut stream = next_connection(&stopped, || match errors.pop_front() {
        Some(kind) => Err(io::Error::from(kind)),
        None => listener.accept(),
    })
    .unwrap()
    .expect("recoverable connection failures stopped acceptance");
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(test_time::DEFAULT_TIMEOUT))
        .unwrap();
    let mut received = [0; 5];
    stream.read_exact(&mut received).unwrap();
    assert_eq!(&received, b"ready");
    assert_eq!(stream.peer_addr().unwrap(), client.local_addr().unwrap());
    assert!(errors.is_empty());
}

#[test]
fn would_block_restores_deadline_free_polling() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let stopped = AtomicBool::new(false);
    let clock = test_time::ManualClock::new();
    let mut retry = AcceptRetry::default();
    let stream: Option<TcpStream> = retry.next(clock.now(), true, &stopped, || {
        Err(io::Error::other("injected undrained accept failure"))
    });
    assert!(stream.is_none());
    clock.advance(ACCEPT_RETRY_DELAY);
    assert!(
        retry
            .next(clock.now(), false, &stopped, || listener.accept())
            .is_none()
    );
    assert_eq!(retry.poll_timeout(clock.now()), None);
    let stream: Option<TcpStream> = retry.next(clock.now(), false, &stopped, || {
        panic!("drained listener accepted without readiness");
    });
    assert!(stream.is_none());
    let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let stream = retry
        .next(clock.now(), true, &stopped, || listener.accept())
        .unwrap();
    assert_eq!(stream.peer_addr().unwrap(), client.local_addr().unwrap());
}

#[test]
fn shutdown_is_checked_between_recoverable_accept_failures() {
    let stopped = AtomicBool::new(false);
    let mut attempts = 0;
    let stream: Option<TcpStream> = next_connection(&stopped, || {
        attempts += 1;
        stopped.store(true, Ordering::Release);
        Err(io::Error::from(io::ErrorKind::ConnectionAborted))
    })
    .unwrap();
    assert!(stream.is_none());
    assert_eq!(attempts, 1);
    let stream: Option<TcpStream> = next_connection(&stopped, || {
        panic!("stopped listener attempted another accept");
    })
    .unwrap();
    assert!(stream.is_none());
}

#[test]
fn undrained_errors_retry_without_another_edge_and_never_before_the_deadline() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    client.write_all(b"queued").unwrap();
    let clock = test_time::ManualClock::new();
    let stopped = AtomicBool::new(false);
    let mut retry = AcceptRetry::default();
    let mut attempts = 0;
    assert_eq!(retry.poll_timeout(clock.now()), None);
    for ready in [true, false] {
        let stream: Option<TcpStream> = retry.next(clock.now(), ready, &stopped, || {
            attempts += 1;
            Err(io::Error::other(
                "injected EMFILE-style resource exhaustion",
            ))
        });
        assert!(stream.is_none());
        assert_eq!(retry.poll_timeout(clock.now()), Some(ACCEPT_RETRY_DELAY));
        clock.advance(ACCEPT_RETRY_DELAY / 2);
        for early_readiness in [false, true] {
            let stream: Option<TcpStream> =
                retry.next(clock.now(), early_readiness, &stopped, || {
                    panic!("undrained listener retried before its deadline");
                });
            assert!(stream.is_none());
        }
        assert_eq!(
            retry.poll_timeout(clock.now()),
            Some(ACCEPT_RETRY_DELAY / 2)
        );
        clock.advance(ACCEPT_RETRY_DELAY / 2);
    }
    assert_eq!(attempts, 2, "persistent resource failure bypassed backoff");
    assert_eq!(retry.poll_timeout(clock.now()), Some(Duration::ZERO));
    let mut stream = retry
        .next(clock.now(), false, &stopped, || listener.accept())
        .expect("queued connection depended on another readiness edge");
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(test_time::DEFAULT_TIMEOUT))
        .unwrap();
    let mut received = [0; 6];
    stream.read_exact(&mut received).unwrap();
    assert_eq!(&received, b"queued");
    // The worker continues draining after success, even when the retry began without an edge.
    assert!(
        retry
            .next(clock.now(), true, &stopped, || listener.accept())
            .is_none()
    );
    assert_eq!(retry.poll_timeout(clock.now()), None);
}

#[test]
fn stop_waker_cancels_a_pending_retry_without_waiting_for_its_deadline() {
    let mut poll = Poll::new().unwrap();
    let wake = Waker::new(poll.registry(), STOP).unwrap();
    let stopped = AtomicBool::new(false);
    let clock = test_time::ManualClock::new();
    let mut retry = AcceptRetry::default();
    let stream: Option<TcpStream> = retry.next(clock.now(), true, &stopped, || {
        Err(io::Error::other("injected resource exhaustion"))
    });
    assert!(stream.is_none());
    assert_eq!(retry.poll_timeout(clock.now()), Some(ACCEPT_RETRY_DELAY));
    stopped.store(true, Ordering::Release);
    wake.wake().unwrap();
    let mut events = Events::with_capacity(2);
    poll.poll(&mut events, Some(Duration::ZERO)).unwrap();
    assert!(events.iter().any(|event| event.token() == STOP));
    let stream: Option<TcpStream> = retry.next(clock.now(), false, &stopped, || {
        panic!("shutdown retried an undrained accept");
    });
    assert!(stream.is_none());
    assert_eq!(retry.poll_timeout(clock.now()), None);
}
