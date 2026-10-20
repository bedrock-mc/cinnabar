//! The exact control attachment and sequential play streams on one WebTransport session.

use super::{Bidi, Transport, current, failed, next_bi};
use crate::{
    control, play,
    prefix::Prefix,
    registry::{Attachment, Registry, refusal},
    session::closed,
};
use bridge::{contract::*, relay_control::Event};
use bytes::Bytes;
use futures::SinkExt;
use std::{future::Future, io, net::IpAddr, pin::Pin, sync::Arc, time::Instant};
use tokio_util::{codec::Framed, sync::CancellationToken};

type Playing<'a> = Pin<Box<dyn Future<Output = io::Result<()>> + Send + 'a>>;

/// Ends the initial cutoff only after the original control Hello claims the actual account.
pub(super) async fn run(
    registry: &Arc<Registry>,
    client: IpAddr,
    transport: Arc<Transport>,
    deadline: Instant,
    stop: CancellationToken,
) -> io::Result<()> {
    let opening = attach(registry, client, Arc::clone(&transport), deadline);
    tokio::pin!(opening);
    let (stream, controller) = tokio::select! { biased;
        () = stop.cancelled() => return Err(closed()),
        () = registry.cancelled.cancelled() => return Err(closed()),
        result = tokio::time::timeout_at(deadline.into(), &mut opening) => result.map_err(|_| super::timed_out())??,
    };
    current(deadline)?;
    let serving = control::run_attached(registry, stream, &controller);
    let playing = plays(registry, Arc::clone(&transport), &controller);
    tokio::pin!(serving, playing);
    tokio::select! { biased;
        () = stop.cancelled() => {
            controller.cancel.cancel();
            playing.await
        }
        result = &mut serving => {
            controller.cancel.cancel();
            let cleanup = playing.await;
            result.and(cleanup)
        }
        result = &mut playing => {
            controller.cancel.cancel();
            result
        }
    }
}

/// Preserves Hello read-ahead and the same accepted-time cutoff through its actual attachment.
async fn attach(
    registry: &Arc<Registry>,
    client: IpAddr,
    transport: Arc<Transport>,
    deadline: Instant,
) -> io::Result<(Prefix<Bidi>, Attachment)> {
    let stream = next_bi(transport).await?;
    current(deadline)?;
    let mut framed = Framed::new(stream, bridge::BridgeCodec::with_max(RELAY_CONTROL_BYTES));
    let hello = control::read_hello(&mut framed, RELAY_CHANNEL_CONTROL).await?;
    current(deadline)?;
    let attachment = match registry.open_control(client, &hello.resume, Instant::now()) {
        Ok(attachment) => attachment,
        Err(error) => {
            let event = serde_json::to_vec(&Event::Error {
                error: refusal(&error),
            })
            .map_err(failed)?;
            framed.send(Bytes::from(event)).await.map_err(failed)?;
            return Err(error);
        }
    };
    current(deadline)?;
    let parts = framed.into_parts();
    Ok((Prefix::new(parts.io, parts.read_buf.freeze()), attachment))
}

/// Retains one accept/resolution future and one replacement stream while the original play closes.
async fn plays(
    registry: &Arc<Registry>,
    transport: Arc<Transport>,
    controller: &Attachment,
) -> io::Result<()> {
    let mut accepting = Box::pin(next_bi(Arc::clone(&transport)));
    let mut queued = None;
    let mut playing: Option<Playing<'_>> = None;
    loop {
        if playing.is_none()
            && let Some(stream) = queued.take()
        {
            // Keep the actual future, including its upstream cleanup, until completion.
            playing = Some(Box::pin(play::run_bound(
                registry,
                stream,
                controller,
                Arc::clone(&transport),
            )));
        }
        tokio::select! { biased;
            () = controller.cancel.cancelled() => {
                if let Some(playing) = playing.as_mut() { let _ = playing.await; }
                return Err(closed());
            }
            result = async { playing.as_mut().expect("original active play").as_mut().await }, if playing.is_some() => {
                // A refused/finished play is local to that stream; its actual cleanup has finished.
                let _ = result;
                playing = None;
            }
            stream = &mut accepting, if queued.is_none() => {
                queued = Some(stream?);
                accepting = Box::pin(next_bi(Arc::clone(&transport)));
            }
        }
    }
}
