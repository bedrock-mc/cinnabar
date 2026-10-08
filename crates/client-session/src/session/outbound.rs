//! The outbound task: batches each frame's packets and writes them without waiting on inbound.
//!
//! Vanilla also flushes a batch early once no more than 20% of its bytes are compressible.
//! Every packet a client sends is compressible, so only the end-of-frame flush applies.

use super::*;
use tokio::sync::oneshot;

/// The write half of a play session, owned by the outbound task.
pub(super) trait OutboundSession: Send + 'static {
    type Error: std::fmt::Display + Send;

    /// Writes `packets` as one batch behind every batch already written; resolves only once
    /// the batch is flushed, so receipts never describe an unsent packet.
    fn send_batch(
        &mut self,
        packets: Vec<Packet>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Takes the one-shot loading-end and initialization packets.
    fn take_finish_loading(&mut self) -> Vec<Packet> {
        Vec::new()
    }
}

impl OutboundSession for protocol::PlayOutbound {
    type Error = protocol::ProtocolError;

    fn send_batch(
        &mut self,
        packets: Vec<Packet>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async move { protocol::PlayOutbound::send_batch(self, &packets).await }
    }

    fn take_finish_loading(&mut self) -> Vec<Packet> {
        protocol::PlayOutbound::take_finish_loading(self)
    }
}

/// Work the outbound task hands to the pump because it touches inbound session state.
pub(super) enum PumpHook {
    /// Arms the packet-ID trace; the batch is written only after the pump acknowledges.
    BeginTrace(oneshot::Sender<()>),
    FastTransferSent(ChatPacketSend),
    /// The traced batch's receipts are published; inbound decoding may resume.
    TracedWriteSettled,
    /// Returns the command receiver so it stays open until the pump queues its terminal event.
    SendFailed {
        message: String,
        chats: Vec<ChatPacketSend>,
        trace_armed: bool,
        commands: mpsc::Receiver<NetworkCommand>,
    },
    CommandsClosed,
}

pub(super) struct OutboundRuntime<O, P, F, W> {
    pub(super) outbound: O,
    pub(super) commands: mpsc::Receiver<NetworkCommand>,
    pub(super) control_event_tx: mpsc::Sender<NetworkControlEvent<P>>,
    pub(super) hooks: mpsc::UnboundedSender<PumpHook>,
    pub(super) shutdown_rx: watch::Receiver<bool>,
    pub(super) experience_gate: Arc<experience::ExperienceGate>,
    pub(super) trace_line: F,
    pub(super) write_trace: W,
    pub(super) fast_transfer_action_marker: Option<&'static str>,
}

/// Aborts the outbound task when the pump ends on any path.
pub(super) struct OutboundTask(pub(super) tokio::task::JoinHandle<()>);

impl Drop for OutboundTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

struct QueuedPacket {
    packet: Packet,
    sub_chunk: Option<SubChunkRequestSend>,
    chat: Option<ChatPacketSend>,
    physics: Option<PhysicsSendIdentity>,
    physics_reanchor: Option<watch::Receiver<u64>>,
    interaction: Option<InteractionPacketGuard>,
}

impl QueuedPacket {
    fn plain(packet: Packet) -> Self {
        Self {
            packet,
            sub_chunk: None,
            chat: None,
            physics: None,
            physics_reanchor: None,
            interaction: None,
        }
    }
}

/// What to publish for one packet once its batch is written.
struct Receipt {
    sub_chunk: Option<SubChunkRequestSend>,
    chat: Option<ChatPacketSend>,
    physics: Option<PhysicsSendIdentity>,
    trace_line: Option<String>,
}

struct FlushFailure {
    message: String,
    chats: Vec<ChatPacketSend>,
    trace_armed: bool,
}

enum Flush {
    Written,
    Stopped,
    Failed(FlushFailure),
}

pub(super) async fn run_outbound<O, P, F, W>(runtime: OutboundRuntime<O, P, F, W>)
where
    O: OutboundSession,
    F: FnMut(u64, &Packet) -> Option<String>,
    W: FnMut(&str),
{
    let OutboundRuntime {
        mut outbound,
        mut commands,
        control_event_tx,
        hooks,
        mut shutdown_rx,
        experience_gate,
        mut trace_line,
        mut write_trace,
        fast_transfer_action_marker,
    } = runtime;
    let mut frame = Vec::new();
    loop {
        let command = tokio::select! {
            biased;
            _ = wait_for_shutdown(&mut shutdown_rx) => return,
            command = commands.recv() => command,
        };
        let (flush, closed) = match command {
            None => (true, true),
            Some(NetworkCommand::FlushFrame) => (true, false),
            Some(NetworkCommand::FinishLoading) => {
                frame.extend(
                    outbound
                        .take_finish_loading()
                        .into_iter()
                        .map(QueuedPacket::plain),
                );
                (false, false)
            }
            Some(NetworkCommand::Send {
                packet,
                sub_chunk,
                chat,
                physics,
                physics_reanchor,
                interaction,
            }) => {
                frame.push(QueuedPacket {
                    packet,
                    sub_chunk,
                    chat,
                    physics,
                    physics_reanchor,
                    interaction,
                });
                (false, false)
            }
        };
        if flush {
            // A closed queue still writes what its last frame queued.
            let flushed = flush_frame(
                &mut outbound,
                std::mem::take(&mut frame),
                &control_event_tx,
                &hooks,
                &mut shutdown_rx,
                &experience_gate,
                (&mut trace_line, &mut write_trace),
                fast_transfer_action_marker,
            )
            .await;
            match flushed {
                Flush::Written => {}
                Flush::Stopped => return,
                Flush::Failed(failure) => {
                    let _ = hooks.send(PumpHook::SendFailed {
                        message: failure.message,
                        chats: failure.chats,
                        trace_armed: failure.trace_armed,
                        commands,
                    });
                    return;
                }
            }
        }
        if closed {
            let _ = hooks.send(PumpHook::CommandsClosed);
            return;
        }
    }
}

#[allow(clippy::too_many_arguments)] // The task's distinct endpoints stay explicit.
async fn flush_frame<O, P, F, W>(
    outbound: &mut O,
    frame: Vec<QueuedPacket>,
    control_event_tx: &mpsc::Sender<NetworkControlEvent<P>>,
    hooks: &mpsc::UnboundedSender<PumpHook>,
    shutdown_rx: &mut watch::Receiver<bool>,
    experience_gate: &experience::ExperienceGate,
    (trace_line, write_trace): (&mut F, &mut W),
    fast_transfer_action_marker: Option<&'static str>,
) -> Flush
where
    O: OutboundSession,
    F: FnMut(u64, &Packet) -> Option<String>,
    W: FnMut(&str),
{
    let mut packets = Vec::with_capacity(frame.len());
    let mut receipts = Vec::with_capacity(frame.len());
    for queued in frame {
        if protocol::is_experience_packet(&queued.packet) && !experience_gate.enabled() {
            continue;
        }
        // Building the batch is the last point where a physics packet is provably unsent.
        if let (Some(identity), Some(reanchor)) = (queued.physics, queued.physics_reanchor.as_ref())
            && *reanchor.borrow() != identity.reanchor_epoch
        {
            if !send_control_event_or_cancel(
                control_event_tx,
                shutdown_rx,
                NetworkControlEvent::PhysicsPacketCancelled {
                    identity,
                    definitely_unsent: true,
                },
            )
            .await
            {
                return Flush::Stopped;
            }
            continue;
        }
        let packet = finalize_interaction_packet(queued.packet, queued.interaction);
        // Traces are formatted from the final packet and published only after the write.
        let line = queued
            .physics
            .and_then(|identity| trace_line(identity.session_generation, &packet));
        packets.push(packet);
        receipts.push(Receipt {
            sub_chunk: queued.sub_chunk,
            chat: queued.chat,
            physics: queued.physics,
            trace_line: line,
        });
    }
    if packets.is_empty() {
        return Flush::Written;
    }
    let trace_armed = receipts.iter().any(|receipt| {
        receipt
            .chat
            .is_some_and(|chat| chat.fast_transfer_action.is_some())
    });
    if trace_armed {
        let (armed, acknowledged) = oneshot::channel();
        if hooks.send(PumpHook::BeginTrace(armed)).is_err()
            || !matches!(
                wait_for_send_or_cancel(acknowledged, shutdown_rx).await,
                Some(Ok(()))
            )
        {
            return Flush::Stopped;
        }
    }
    match wait_for_send_or_cancel(outbound.send_batch(packets), shutdown_rx).await {
        None => Flush::Stopped,
        Some(Err(error)) => Flush::Failed(FlushFailure {
            message: error.to_string(),
            chats: receipts.iter().filter_map(|receipt| receipt.chat).collect(),
            trace_armed,
        }),
        Some(Ok(())) => {
            let settle = trace_armed.then_some(PumpHook::TracedWriteSettled);
            for receipt in receipts {
                if !publish_receipt(
                    receipt,
                    control_event_tx,
                    hooks,
                    shutdown_rx,
                    write_trace,
                    fast_transfer_action_marker,
                )
                .await
                {
                    return Flush::Stopped;
                }
            }
            if let Some(settle) = settle
                && hooks.send(settle).is_err()
            {
                return Flush::Stopped;
            }
            Flush::Written
        }
    }
}

async fn publish_receipt<P, W: FnMut(&str)>(
    receipt: Receipt,
    control_event_tx: &mpsc::Sender<NetworkControlEvent<P>>,
    hooks: &mpsc::UnboundedSender<PumpHook>,
    shutdown_rx: &mut watch::Receiver<bool>,
    write_trace: &mut W,
    fast_transfer_action_marker: Option<&'static str>,
) -> bool {
    if let Some(line) = receipt.trace_line {
        write_trace(&line);
    }
    if let Some(identity) = receipt.physics
        && !send_control_event_or_cancel(
            control_event_tx,
            shutdown_rx,
            NetworkControlEvent::PhysicsPacketSent { identity },
        )
        .await
    {
        return false;
    }
    if let Some(marker) = receipt.chat.and_then(|chat| {
        let marker_name = fast_transfer_action_marker?;
        chat.fast_transfer_action.map(|action| {
            let sent_unix_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
                .unwrap_or(0);
            action.marker(marker_name, chat.session, chat.sequence, sent_unix_ms)
        })
    }) {
        write_stdout_marker(&mut std::io::stdout().lock(), &marker);
    }
    if let Some(sub_chunk) = receipt.sub_chunk
        && !send_control_event_or_cancel(
            control_event_tx,
            shutdown_rx,
            NetworkControlEvent::SubChunkRequestSent {
                chunk: sub_chunk.chunk,
                base_sub_chunk_y: sub_chunk.base_sub_chunk_y,
                count: sub_chunk.count,
                sent_at: Instant::now(),
            },
        )
        .await
    {
        return false;
    }
    match receipt.chat {
        // The pump owns the world FIFO the transfer barrier joins, so it publishes the receipt.
        Some(chat) if chat.fast_transfer_action.is_some() => {
            hooks.send(PumpHook::FastTransferSent(chat)).is_ok()
        }
        Some(chat) => {
            send_control_event_or_cancel(
                control_event_tx,
                shutdown_rx,
                NetworkControlEvent::ChatPacketSent {
                    session: chat.session,
                    sequence: chat.sequence,
                },
            )
            .await
        }
        None => true,
    }
}

fn finalize_interaction_packet(
    packet: Packet,
    interaction: Option<InteractionPacketGuard>,
) -> Packet {
    match interaction {
        Some(guard) => guard.sanitize(packet),
        None => packet,
    }
}
