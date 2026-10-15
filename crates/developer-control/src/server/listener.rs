//! Wakeable listener ownership; accepted connections retain their existing request protocol.

use super::*;
use mio::{Events, Interest, Poll, Token, Waker};
use std::sync::atomic::{AtomicBool, Ordering};

const ACCEPT: Token = Token(0);
const STOP: Token = Token(1);

pub(super) struct ListenerWorker {
    stopped: Arc<AtomicBool>,
    wake: Arc<Waker>,
    thread: Option<thread::JoinHandle<()>>,
}

impl ListenerWorker {
    /// Arms an allocation-free wake path before the worker starts or an endpoint is published.
    pub(super) fn start(
        listener: TcpListener,
        token: Arc<str>,
        sender: Sender<Pending>,
        spawn: impl FnOnce(Box<dyn FnOnce() + Send>) -> io::Result<thread::JoinHandle<()>>,
    ) -> io::Result<Self> {
        listener.set_nonblocking(true)?;
        let mut listener = mio::net::TcpListener::from_std(listener);
        let poll = Poll::new()?;
        poll.registry()
            .register(&mut listener, ACCEPT, Interest::READABLE)?;
        let wake = Arc::new(Waker::new(poll.registry(), STOP)?);
        let stopped = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stopped);
        let thread = spawn(Box::new(move || {
            accept(poll, listener, token, sender, stopping)
        }))?;
        Ok(Self {
            stopped,
            wake,
            thread: Some(thread),
        })
    }
}

impl Drop for ListenerWorker {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        let _ = self.wake.wake();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Drains ready accepts and exits on shutdown without polling deadlines or a new TCP connection.
fn accept(
    mut poll: Poll,
    listener: mio::net::TcpListener,
    token: Arc<str>,
    sender: Sender<Pending>,
    stopped: Arc<AtomicBool>,
) {
    let mut events = Events::with_capacity(2);
    while !stopped.load(Ordering::Acquire) {
        if let Err(error) = poll.poll(&mut events, None) {
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return;
        }
        for event in &events {
            if event.token() == STOP {
                return;
            }
            while let Some(stream) = next_connection(&stopped, || listener.accept()) {
                let stream = TcpStream::from(stream);
                if stream.set_nonblocking(false).is_err() {
                    continue;
                }
                let (token, sender) = (Arc::clone(&token), sender.clone());
                let _ = thread::Builder::new()
                    .name("developer-control-connection".into())
                    .spawn(move || serve(stream, &token, &sender));
            }
        }
    }
}

/// Retries individual connection failures; no ready connection yields control back to Poll.
/// Shutdown is checked before every retry, including a run of interrupted or aborted accepts.
fn next_connection<S>(
    stopped: &AtomicBool,
    mut accept: impl FnMut() -> io::Result<(S, SocketAddr)>,
) -> Option<S> {
    while !stopped.load(Ordering::Acquire) {
        match accept() {
            Ok((stream, peer)) if is_loopback(&peer) => return Some(stream),
            Ok(_) => continue,
            Err(error) => match error.kind() {
                io::ErrorKind::Interrupted
                | io::ErrorKind::ConnectionAborted
                | io::ErrorKind::ConnectionReset
                | io::ErrorKind::ConnectionRefused => continue,
                _ => return None,
            },
        }
    }
    None
}

#[cfg(test)]
mod tests;
