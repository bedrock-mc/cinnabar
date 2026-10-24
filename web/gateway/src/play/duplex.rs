//! Caller-polled browser I/O around the original weighted native connection driver.

use super::{
    Bytes, CoreMessage, FrameQueue, FrameRead, PlayLease, closed, current, elapsed, expires,
    invalid, progress, session_wire,
};
use futures::StreamExt;
use protocol::{NativeRelayDriver, NativeRelayHandoff, NativeRelayInbound, NativeRelaySender};
use std::{io, time::Instant};
use tokio::sync::oneshot;

/// Pins each actual direction once; driver completion never replaces accepted input.
#[expect(
    clippy::too_many_arguments,
    reason = "the original pump carries its I/O, handoff, deadline and archive delivery owner together"
)]
pub(super) async fn run(
    lease: &PlayLease,
    driver: &mut NativeRelayDriver,
    (sender, incoming): (NativeRelaySender, NativeRelayInbound),
    reader: impl FrameRead,
    frames: FrameQueue,
    handoff: NativeRelayHandoff,
    deadline: Instant,
    delivery: super::packs::Delivery,
) -> io::Result<()> {
    lease.current()?;
    let (handoff_flushed, handoff_written) = oneshot::channel();
    let (terminal_reached, terminal_observed) = oneshot::channel();
    let upload = upload(lease, reader, sender, handoff_written, terminal_observed);
    let output = output(
        lease,
        &frames,
        incoming,
        handoff,
        deadline,
        handoff_flushed,
        terminal_reached,
        delivery,
    );
    let driving = driver.drive();
    tokio::pin!(upload, output, driving);
    let mut uploading = true;
    let mut driver_done = false;
    let mut send_failure = None;
    let mut drain_deadline = None;
    loop {
        tokio::select! { biased;
            () = lease.cancel.cancelled() => return Err(closed()),
            result = &mut output => return result,
            () = expires(drain_deadline), if drain_deadline.is_some() => {
                return Err(send_failure.take().expect("failed send armed its original drain"));
            }
            result = &mut upload, if uploading => {
                uploading = false;
                lease.current()?;
                match result {
                    Err(UploadError::Input(error)) => return Err(error),
                    Err(UploadError::Send(error)) => {
                        send_failure = Some(error);
                        drain_deadline = Some(Instant::now()
                            .checked_add(protocol::WRITE_FAILURE_DRAIN)
                            .ok_or_else(closed)?);
                    }
                    Ok(()) => return Err(closed()),
                }
            }
            _ = &mut driving, if !driver_done => {
                // The driver retains its close result. The original incoming FIFO and
                // one completion tail must still deliver their own prefix and cause.
                driver_done = true;
            }
        }
    }
}

enum UploadError {
    Input(io::Error),
    Send(io::Error),
}

/// Retains one browser frame through original queue admission and the actual write receipt.
async fn upload(
    lease: &PlayLease,
    mut reader: impl FrameRead,
    sender: NativeRelaySender,
    handoff_written: oneshot::Receiver<()>,
    terminal_observed: oneshot::Receiver<()>,
) -> Result<(), UploadError> {
    let result = {
        let sending = upload_frames(lease, &mut reader, sender, handoff_written);
        tokio::pin!(sending);
        tokio::select! { biased;
            _ = terminal_observed => None,
            result = &mut sending => Some(result),
        }
    };
    if let Some(result) = result {
        return result;
    }
    // Stop new application admissions after the actual terminal. Dropping a
    // receipt does not cancel its driver's accepted write. Keep the same reader
    // so empty EOF or a post-terminal payload can still end blocked forwarding.
    let _ = reader.next().await;
    Err(UploadError::Input(closed()))
}

/// Keeps partial reads, queue admission and receipt progress until this one direction ends.
async fn upload_frames(
    lease: &PlayLease,
    reader: &mut impl FrameRead,
    sender: NativeRelaySender,
    handoff_written: oneshot::Receiver<()>,
) -> Result<(), UploadError> {
    let mut handoff_written = Some(handoff_written);
    loop {
        let frame = reader
            .next()
            .await
            .ok_or_else(|| UploadError::Input(closed()))?
            .map_err(|error| UploadError::Input(invalid(error)))?;
        lease.current().map_err(UploadError::Input)?;
        let body = session_wire::batch_frame_body(&frame)
            .ok_or_else(|| UploadError::Input(invalid(())))?;
        if body.len() > bedrock_net::BatchLimits::default().max_decoded_bytes {
            return Err(UploadError::Input(invalid(())));
        }
        if let Some(written) = handoff_written.take() {
            // Reading remains active during handoff, so empty EOF is observable.
            // An actual first payload waits for the original handoff/archive flush.
            written.await.map_err(|_| UploadError::Input(closed()))?;
            lease.current().map_err(UploadError::Input)?;
        }
        let accepted = sender.accept_body(body).await;
        lease.current().map_err(UploadError::Input)?;
        let receipt = accepted.map_err(UploadError::Send)?;
        let written = receipt.await;
        lease.current().map_err(UploadError::Input)?;
        written.map_err(UploadError::Send)?;
    }
}

/// Flushes original handoff/archive order, then one original incoming packet at a time.
#[expect(
    clippy::too_many_arguments,
    reason = "one original output owns handoff, archive routing and both actual flush barriers"
)]
async fn output(
    lease: &PlayLease,
    frames: &FrameQueue,
    incoming: NativeRelayInbound,
    mut handoff: NativeRelayHandoff,
    deadline: Instant,
    handoff_flushed: oneshot::Sender<()>,
    terminal_reached: oneshot::Sender<()>,
    delivery: super::packs::Delivery,
) -> io::Result<()> {
    if delivery.independent() {
        handoff.handoff.pack_stream = Some(lease.join);
    }
    write(
        lease,
        frames,
        session_wire::encode_core_message(&CoreMessage::Handoff(handoff.handoff))
            .map_err(invalid)?,
        Some(deadline),
    )
    .await?;
    if !delivery.independent() {
        super::packs::send(delivery, lease, frames, handoff.archives, deadline).await?;
        current(lease, Some(deadline))?;
        lease.progress(progress(bridge::ConnectStage::Idle))?;
        let _ = handoff_flushed.send(());
        return forward(lease, frames, incoming, terminal_reached).await;
    }
    // The separate archive stream cannot withhold game reads, control or empty browser EOF.
    let _ = handoff_flushed.send(());
    let sending = super::packs::send(delivery, lease, frames, handoff.archives, deadline);
    let forwarding = forward(lease, frames, incoming, terminal_reached);
    tokio::pin!(sending, forwarding);
    tokio::select! { biased;
        result = &mut sending => {
            result?;
            current(lease, Some(deadline))?;
            lease.progress(progress(bridge::ConnectStage::Idle))?;
            forwarding.await
        }
        result = &mut forwarding => {
            result?;
            sending.await
        }
    }
}

/// Delivers the same accepted reliable prefix and first terminal after the actual handoff flush.
async fn forward(
    lease: &PlayLease,
    frames: &FrameQueue,
    mut incoming: NativeRelayInbound,
    terminal_reached: oneshot::Sender<()>,
) -> io::Result<()> {
    let mut terminal_reached = Some(terminal_reached);
    loop {
        lease.current()?;
        let message = incoming.recv().await?;
        lease.current()?;
        let terminal = matches!(
            message,
            CoreMessage::Disconnect(_) | CoreMessage::Transfer(_)
        );
        if terminal {
            let _ = terminal_reached
                .take()
                .expect("first actual terminal")
                .send(());
        }
        write(
            lease,
            frames,
            session_wire::encode_core_message(&message).map_err(invalid)?,
            None,
        )
        .await?;
        if terminal {
            return Ok(());
        }
    }
}

/// Keeps the same bounded frame admission/flush future and original absolute join budget.
pub(super) async fn write(
    lease: &PlayLease,
    frames: &FrameQueue,
    frame: Bytes,
    deadline: Option<Instant>,
) -> io::Result<()> {
    current(lease, deadline)?;
    tokio::select! { biased;
        () = lease.cancel.cancelled() => Err(closed()),
        () = expires(deadline), if deadline.is_some() => Err(elapsed()),
        result = frames.send(frame) => {
            result.map_err(invalid)?;
            current(lease, deadline)
        }
    }
}
