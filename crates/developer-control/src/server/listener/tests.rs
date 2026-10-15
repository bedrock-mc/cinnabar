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
    .expect("recoverable connection failures stopped acceptance");
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
fn no_ready_connection_returns_to_poll_without_retiring_the_listener() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let stopped = AtomicBool::new(false);
    for kind in [io::ErrorKind::WouldBlock, io::ErrorKind::Other] {
        let stream: Option<TcpStream> = next_connection(&stopped, || Err(io::Error::from(kind)));
        assert!(stream.is_none());
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let stream = next_connection(&stopped, || listener.accept()).unwrap();
        assert_eq!(stream.peer_addr().unwrap(), client.local_addr().unwrap());
    }
}

#[test]
fn shutdown_is_checked_between_recoverable_accept_failures() {
    let stopped = AtomicBool::new(false);
    let mut attempts = 0;
    let stream: Option<TcpStream> = next_connection(&stopped, || {
        attempts += 1;
        stopped.store(true, Ordering::Release);
        Err(io::Error::from(io::ErrorKind::ConnectionAborted))
    });
    assert!(stream.is_none());
    assert_eq!(attempts, 1);
    let stream: Option<TcpStream> = next_connection(&stopped, || {
        panic!("stopped listener attempted another accept");
    });
    assert!(stream.is_none());
}
