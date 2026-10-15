//! Wakeable listener ownership; accepted connections retain their existing request protocol.

use super::*;
use mio::{Events, Interest, Poll, Token, Waker};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const ACCEPT: Token = Token(0);
const STOP: Token = Token(1);
const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(10);

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

/// Drains ready accepts, retrying undrained errors without another readiness edge.
/// Normal waiting has no deadline; shutdown always wakes Poll immediately.
fn accept(
    mut poll: Poll,
    listener: mio::net::TcpListener,
    token: Arc<str>,
    sender: Sender<Pending>,
    stopped: Arc<AtomicBool>,
) {
    let mut events = Events::with_capacity(2);
    let mut retry = AcceptRetry::default();
    while !stopped.load(Ordering::Acquire) {
        if let Err(error) = poll.poll(&mut events, retry.poll_timeout(Instant::now())) {
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return;
        }
        if events.iter().any(|event| event.token() == STOP) {
            return;
        }
        let mut ready = events.iter().any(|event| event.token() == ACCEPT);
        while let Some(stream) = retry.next(Instant::now(), ready, &stopped, || listener.accept()) {
            // An accepted stream does not prove the edge-triggered queue is drained.
            ready = true;
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

#[derive(Default)]
struct AcceptRetry {
    deadline: Option<Instant>,
}

impl AcceptRetry {
    /// Keeps normal polling deadline-free, or waits only until an undrained queue must retry.
    fn poll_timeout(&self, now: Instant) -> Option<Duration> {
        self.deadline
            .map(|deadline| deadline.saturating_duration_since(now))
    }

    /// Accepts on readiness or a due retry, retaining backoff even if readiness arrives early.
    fn next<S>(
        &mut self,
        now: Instant,
        ready: bool,
        stopped: &AtomicBool,
        accept: impl FnMut() -> io::Result<(S, SocketAddr)>,
    ) -> Option<S> {
        if stopped.load(Ordering::Acquire) {
            self.deadline = None;
            return None;
        }
        let due = self.deadline.map_or(ready, |deadline| now >= deadline);
        if !due {
            return None;
        }
        match next_connection(stopped, accept) {
            Ok(stream) => {
                self.deadline = None;
                stream
            }
            Err(_) => {
                self.deadline =
                    (!stopped.load(Ordering::Acquire)).then_some(now + ACCEPT_RETRY_DELAY);
                None
            }
        }
    }
}

/// Retries individual connection failures; only WouldBlock proves readiness was drained.
/// Shutdown is checked before every retry, including a run of interrupted or aborted accepts.
fn next_connection<S>(
    stopped: &AtomicBool,
    mut accept: impl FnMut() -> io::Result<(S, SocketAddr)>,
) -> io::Result<Option<S>> {
    while !stopped.load(Ordering::Acquire) {
        match accept() {
            Ok((stream, peer)) if is_loopback(&peer) => return Ok(Some(stream)),
            Ok(_) => continue,
            Err(error) => match error.kind() {
                io::ErrorKind::Interrupted
                | io::ErrorKind::ConnectionAborted
                | io::ErrorKind::ConnectionReset
                | io::ErrorKind::ConnectionRefused => continue,
                io::ErrorKind::WouldBlock => return Ok(None),
                _ => return Err(error),
            },
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests;
