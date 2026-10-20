//! The packet mod's WASI preview 2 context: clocks, randomness and captured output only.

use anyhow::Result;
use std::{
    future::Future,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};
use wasmtime_wasi::{
    WasiCtxBuilder,
    cli::{IsTerminal, StdoutStream},
    p2::{OutputStream, Pollable, StreamError, pipe::SinkOutputStream},
};

/// Guest output bytes logged per call; the rest is dropped with one notice.
const MAX_LOG_BYTES_PER_CALL: usize = 16 * 1024;
/// Longest logged line; longer output is split.
const MAX_LOG_LINE_BYTES: usize = 1024;

/// Configures a context with no preopened directories, environment, arguments or stdin, and
/// with every socket and name lookup refused. Standard output and error go to the log.
pub(super) fn builder(budget: &LogBudget) -> WasiCtxBuilder {
    let mut builder = WasiCtxBuilder::new();
    builder
        .stdout(LogSink::new("stdout", budget.clone()))
        .stderr(LogSink::new("stderr", budget.clone()))
        .allow_tcp(false)
        .allow_udp(false)
        .allow_ip_name_lookup(false)
        .allow_blocking_current_thread(false);
    builder
}

/// Cancels suspended WASI work at the same deadline used for guest execution.
pub(super) struct CallDeadline {
    runtime: tokio::runtime::Runtime,
    expires: Instant,
}

impl CallDeadline {
    /// Creates the worker's private runtime and arms its initial load budget.
    pub(super) fn new(budget: Duration) -> Result<Self> {
        Ok(Self {
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?,
            expires: Instant::now() + budget,
        })
    }

    /// Shares one absolute deadline across all lowered calls and post-return work.
    pub(super) fn arm(&mut self, budget: Duration) {
        self.expires = Instant::now() + budget;
    }

    /// Drives an interruptible guest call, dropping its suspended host future on expiry.
    pub(super) fn run<T>(&self, call: impl Future<Output = Result<T>>) -> Result<T> {
        self.runtime.block_on(async {
            tokio::time::timeout_at(self.expires.into(), call)
                .await
                .map_err(|_| wasmtime::Trap::Interrupt)?
        })
    }
}

/// The per-call log allowance shared by standard output and error.
#[derive(Clone)]
pub(super) struct LogBudget(Arc<Mutex<Allowance>>);

struct Allowance {
    remaining: usize,
    truncated: bool,
}

impl Default for LogBudget {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(Allowance {
            remaining: MAX_LOG_BYTES_PER_CALL,
            truncated: false,
        })))
    }
}

impl LogBudget {
    /// Restores the full allowance before the next guest call.
    pub(super) fn refill(&self) {
        let mut allowance = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        allowance.remaining = MAX_LOG_BYTES_PER_CALL;
        allowance.truncated = false;
    }

    /// Returns how many of `wanted` bytes may still be logged in this call.
    fn take(&self, wanted: usize) -> usize {
        let mut allowance = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let granted = wanted.min(allowance.remaining);
        allowance.remaining -= granted;
        if granted < wanted && !allowance.truncated {
            allowance.truncated = true;
            tracing::warn!("packet mod output truncated for this call");
        }
        granted
    }
}

/// A guest output stream that logs whole lines and never fails a write.
#[derive(Clone)]
struct LogSink {
    stream: &'static str,
    budget: LogBudget,
    line: Arc<Mutex<Vec<u8>>>,
}

impl LogSink {
    fn new(stream: &'static str, budget: LogBudget) -> Self {
        Self {
            stream,
            budget,
            line: Arc::default(),
        }
    }

    /// Appends logged bytes, emitting each completed or overlong line.
    fn append(&self, bytes: &[u8]) {
        let bytes = &bytes[..self.budget.take(bytes.len())];
        let mut line = self.line.lock().unwrap_or_else(PoisonError::into_inner);
        for &byte in bytes {
            if byte == b'\n' {
                self.emit(&mut line);
            } else {
                line.push(byte);
                if line.len() >= MAX_LOG_LINE_BYTES {
                    self.emit(&mut line);
                }
            }
        }
    }

    fn emit(&self, line: &mut Vec<u8>) {
        let text = String::from_utf8_lossy(line);
        tracing::info!(stream = self.stream, "packet mod: {}", text.trim_end());
        line.clear();
    }
}

impl IsTerminal for LogSink {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl StdoutStream for LogSink {
    fn p2_stream(&self) -> Box<dyn OutputStream> {
        Box::new(self.clone())
    }

    /// Only WASI preview 3 asks for this stream, and that is not linked.
    fn async_stream(&self) -> Box<dyn tokio::io::AsyncWrite + Send + Sync> {
        StdoutStream::async_stream(&SinkOutputStream)
    }
}

#[wasmtime_wasi::async_trait]
impl OutputStream for LogSink {
    fn write(&mut self, bytes: bytes::Bytes) -> Result<(), StreamError> {
        self.append(&bytes);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), StreamError> {
        Ok(())
    }

    fn check_write(&mut self) -> Result<usize, StreamError> {
        Ok(usize::MAX)
    }
}

#[wasmtime_wasi::async_trait]
impl Pollable for LogSink {
    async fn ready(&mut self) {}
}
