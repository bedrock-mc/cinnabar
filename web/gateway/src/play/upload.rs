//! Byte pacing for the actual post-Hello upload; the enclosing play owner retains cancellation.

use bridge::contract::{RELAY_UPLOAD_BURST_BYTES, RELAY_UPLOAD_BYTES_PER_SECOND};
use bytes::{Buf, Bytes};
use std::{
    future::Future,
    io,
    pin::Pin,
    task::{Context, Poll, ready},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, ReadBuf},
    time::{Instant, Sleep},
};

const BYTE_NANOS: i128 = 1_000_000_000;
const _: () = {
    assert!(RELAY_UPLOAD_BYTES_PER_SECOND > 0);
    assert!(RELAY_UPLOAD_BURST_BYTES > 0);
};

/// Keeps one consumed read and its original pacing wait across canceled borrowed reads.
pub(super) struct Upload<R> {
    inner: R,
    credit: i128,
    updated: Instant,
    pending: Bytes,
    waiting: Option<Pin<Box<Sleep>>>,
}

impl<R> Upload<R> {
    /// Starts the full generated burst after Hello and the original play lease are acquired.
    pub(super) fn new(inner: R) -> Self {
        Self {
            inner,
            credit: capacity(),
            updated: Instant::now(),
            pending: Bytes::new(),
            waiting: None,
        }
    }

    /// Reserves only bytes actually read, preserving fractional credit and the original wait.
    fn reserve(&mut self, size: usize, now: Instant) -> io::Result<Option<Instant>> {
        let now = now.max(self.updated);
        let capacity = capacity();
        // One pending read can owe at most a burst. Twice that refill also fills an indebted bucket.
        let maximum_refill = u128::try_from(capacity * 2).expect("bounded upload refill");
        let earned = now
            .saturating_duration_since(self.updated)
            .as_nanos()
            .saturating_mul(u128::from(RELAY_UPLOAD_BYTES_PER_SECOND))
            .min(maximum_refill);
        self.credit =
            (self.credit + i128::try_from(earned).expect("bounded upload refill")).min(capacity);
        self.updated = now;
        self.credit -=
            i128::try_from(size).expect("one upload read fits signed credit") * BYTE_NANOS;
        if self.credit >= 0 {
            return Ok(None);
        }
        let nanos = self
            .credit
            .unsigned_abs()
            .div_ceil(u128::from(RELAY_UPLOAD_BYTES_PER_SECOND));
        let delay = Duration::from_nanos(u64::try_from(nanos).map_err(|_| deadline_overflow())?);
        now.checked_add(delay)
            .map(Some)
            .ok_or_else(deadline_overflow)
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for Upload<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if output.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        if this.pending.is_empty() {
            let size = output.remaining().min(RELAY_UPLOAD_BURST_BYTES);
            let mut received = ReadBuf::new(output.initialize_unfilled_to(size));
            ready!(Pin::new(&mut this.inner).poll_read(cx, &mut received))?;
            let size = received.filled().len();
            if size == 0 {
                return Poll::Ready(Ok(()));
            }
            let Some(deadline) = this.reserve(size, Instant::now())? else {
                output.advance(size);
                return Poll::Ready(Ok(()));
            };
            // AsyncRead cannot advance the caller's filled bytes while its pacing wait is Pending.
            this.pending = Bytes::copy_from_slice(received.filled());
            this.waiting = Some(Box::pin(tokio::time::sleep_until(deadline)));
        }
        if let Some(waiting) = this.waiting.as_mut() {
            ready!(waiting.as_mut().poll(cx));
            this.waiting = None;
        }
        let size = output.remaining().min(this.pending.len());
        output.put_slice(&this.pending[..size]);
        this.pending.advance(size);
        if this.pending.is_empty() {
            this.pending = Bytes::new();
        }
        Poll::Ready(Ok(()))
    }
}

/// Uses generated policy once; signed units retain the fraction owed by a pending read.
fn capacity() -> i128 {
    i128::try_from(RELAY_UPLOAD_BURST_BYTES).expect("generated upload burst fits signed credit")
        * BYTE_NANOS
}

/// Terminates the same stream if its actual clock cannot represent the bounded pacing delay.
fn deadline_overflow() -> io::Error {
    io::Error::other("gateway upload pacing deadline overflow")
}

#[cfg(test)]
mod tests;
