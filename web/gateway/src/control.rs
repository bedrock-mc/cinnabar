//! The actual bounded control stream; service acquisition stays with the independent caller driver.

use super::{
    budget::Budget,
    registry::{Attachment, Registry, refusal},
    session::closed,
};
use bridge::{
    FrameQueue, FrameRead,
    contract::*,
    relay_control::{Event, Hello, Request},
};
use bytes::Bytes;
use futures::StreamExt;
use std::{
    collections::VecDeque,
    future::Future,
    io,
    net::IpAddr,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::io::{AsyncRead, AsyncWrite};

type Writing = Pin<Box<dyn Future<Output = io::Result<()>> + Send>>;

struct Output {
    write: Writing,
    welcome: bool,
}

/// Consumes one already selected control stream and keeps its writer caller-owned through completion.
pub(super) async fn run<S>(registry: &Arc<Registry>, client: IpAddr, stream: S) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let (read, write) = tokio::io::split(stream);
    let (reader, frames, writer) = bridge::tasks::prepare_bounded_connection(
        read,
        write,
        RELAY_CONTROL_BYTES,
        RELAY_CONTROL_BYTES,
    );
    let serving = serve(registry, client, reader, frames);
    tokio::pin!(serving, writer);
    tokio::select! {
        biased;
        () = registry.cancelled.cancelled() => Err(closed()),
        result = &mut serving => result,
        () = &mut writer => Err(closed()),
    }
}

/// Preserves one pending output future while unrelated requests, watches and timers make progress.
async fn serve(
    registry: &Arc<Registry>,
    client: IpAddr,
    mut reader: impl FrameRead,
    frames: FrameQueue,
) -> io::Result<()> {
    let hello = read_hello(&mut reader, RELAY_CHANNEL_CONTROL).await?;
    let attachment = match registry.open_control(client, &hello.resume, Instant::now()) {
        Ok(attachment) => attachment,
        Err(error) => {
            output(
                frames.clone(),
                Event::Error {
                    error: refusal(&error),
                },
                false,
            )?
            .write
            .await?;
            return Err(error);
        }
    };
    serve_attached(registry, reader, frames, &attachment).await
}

/// Drives an already claimed controller without another Hello or capability lookup.
#[cfg(feature = "webtransport")]
pub(super) async fn run_attached<S>(
    registry: &Arc<Registry>,
    stream: S,
    attachment: &Attachment,
) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let (read, write) = tokio::io::split(stream);
    let (reader, frames, writer) = bridge::tasks::prepare_bounded_connection(
        read,
        write,
        RELAY_CONTROL_BYTES,
        RELAY_CONTROL_BYTES,
    );
    let serving = serve_attached(registry, reader, frames, attachment);
    tokio::pin!(serving, writer);
    tokio::select! { biased;
        () = registry.cancelled.cancelled() => Err(closed()),
        result = &mut serving => result,
        () = &mut writer => Err(closed()),
    }
}

/// Keeps the original controller's writer, requests and observations under one lifetime.
async fn serve_attached(
    registry: &Arc<Registry>,
    mut reader: impl FrameRead,
    frames: FrameQueue,
    attachment: &Attachment,
) -> io::Result<()> {
    let mut observation = attachment.session.observation.subscribe();
    let mut progress = attachment.session.progress.subscribe();
    let capability = attachment
        .session
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .capability(attachment.generation, false, Instant::now())?;
    let mut writing = Some(output(frames.clone(), Event::Welcome { capability }, true)?);
    let mut errors = VecDeque::new();
    let mut account_changed = false;
    let mut progress_changed = false;
    let mut keepalive_due = false;
    let period = Duration::from_secs(u64::from(RELAY_KEEPALIVE_SECONDS));
    let mut keepalive = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
    keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let silence = tokio::time::sleep(period * 2);
    tokio::pin!(silence);
    let mut requests = Budget::new(
        RELAY_CONTROL_REQUEST_BURST,
        Duration::from_millis(u64::from(RELAY_CONTROL_REFILL_MILLIS)),
        Instant::now(),
    );
    loop {
        if writing.is_none() {
            let event = if let Some(error) = errors.pop_front() {
                Some(Event::Error { error })
            } else if keepalive_due {
                keepalive_due = false;
                let capability = attachment
                    .session
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .capability(attachment.generation, true, Instant::now())?;
                Some(Event::Ping { capability })
            } else if account_changed {
                account_changed = false;
                Some(Event::Account {
                    account: observation.borrow_and_update().clone(),
                })
            } else if progress_changed {
                progress_changed = false;
                Some(Event::Connect {
                    connect: *progress.borrow_and_update(),
                })
            } else {
                None
            };
            if let Some(event) = event {
                writing = Some(output(frames.clone(), event, false)?);
            }
        }
        tokio::select! {
            biased;
            () = registry.cancelled.cancelled() => return Err(closed()),
            () = attachment.cancel.cancelled() => return Err(closed()),
            () = &mut silence => return Err(io::Error::new(io::ErrorKind::TimedOut, "gateway control peer silent")),
            result = async { writing.as_mut().expect("pending output").write.as_mut().await }, if writing.is_some() => {
                result?;
                if writing.take().expect("completed output").welcome { account_changed = true; }
            }
            frame = reader.next(), if errors.len() < usize::try_from(RELAY_CONTROL_REQUEST_BURST).expect("bounded generated burst") => {
                let frame = frame.ok_or_else(closed)?.map_err(failed)?;
                let now = Instant::now();
                if !requests.spend(now) { return Err(closed()); }
                let request: Request = serde_json::from_slice(&frame).map_err(failed)?;
                let result = match request {
                    Request::SignIn => registry.sign_in(attachment, now),
                    Request::SignOut => attachment.session.sign_out(attachment.generation),
                    Request::Pong { resume } => {
                        let acknowledged = attachment.session.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).acknowledge(attachment.generation, &resume, now);
                        if acknowledged { silence.as_mut().reset(tokio::time::Instant::now() + period * 2); }
                        continue;
                    }
                    Request::Unknown => Err(io::Error::new(io::ErrorKind::InvalidInput, "unsupported gateway operation")),
                };
                if let Err(error) = result { errors.push_back(refusal(&error)); }
            }
            changed = observation.changed() => {
                changed.map_err(failed)?;
                account_changed = true;
            }
            changed = progress.changed() => {
                changed.map_err(failed)?;
                progress_changed = true;
            }
            _ = keepalive.tick() => { keepalive_due = true; }
        }
    }
}

/// Reads exactly one nonempty bounded Hello under its original application-handshake budget.
pub(super) async fn read_hello(
    reader: &mut (impl futures::Stream<Item = Result<Bytes, bridge::BridgeError>> + Unpin),
    channel: &str,
) -> io::Result<Hello> {
    let frame = tokio::time::timeout(
        Duration::from_secs(u64::from(RELAY_HELLO_SECONDS)),
        reader.next(),
    )
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "gateway Hello timed out"))?
    .ok_or_else(closed)?
    .map_err(failed)?;
    let hello: Hello = serde_json::from_slice(&frame).map_err(failed)?;
    if hello.version != RELAY_VERSION
        || hello.channel != channel
        || channel == RELAY_CHANNEL_CONTROL
            && ![RELAY_CHANNEL_CONTROL, RELAY_CHANNEL_PLAY]
                .iter()
                .all(|expected| hello.channels.iter().any(|channel| channel == expected))
    {
        return Err(failed(()));
    }
    Ok(hello)
}

/// Serializes one bounded control result once and retains its original write deadline across wakes.
fn output(frames: FrameQueue, event: Event, welcome: bool) -> io::Result<Output> {
    let frame = serde_json::to_vec(&event).map_err(failed)?;
    if frame.is_empty() || frame.len() > RELAY_CONTROL_BYTES {
        return Err(failed(()));
    }
    let deadline =
        tokio::time::Instant::now() + Duration::from_secs(u64::from(RELAY_HELLO_SECONDS));
    let write = async move {
        tokio::time::timeout_at(deadline, frames.send(Bytes::from(frame)))
            .await
            .map_err(|_| {
                io::Error::new(io::ErrorKind::TimedOut, "gateway control output timed out")
            })?
            .map_err(failed)
    };
    Ok(Output {
        write: Box::pin(write),
        welcome,
    })
}

/// Converts framing and provider causes without echoing input, paths, capabilities or credentials.
fn failed<T>(_cause: T) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid gateway control stream")
}
