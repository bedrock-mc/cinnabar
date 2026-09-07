use super::*;
use crate::movement::{pending_trace_line, write_trace_line};

fn finalize_mining_packet(packet: Packet, mining: Option<MiningPacketGuard>) -> Packet {
    match mining {
        Some(guard) => guard.sanitize(packet),
        None => packet,
    }
}

struct NetworkPumpRuntime<F, W> {
    readiness_ingress: Arc<ReadinessIngressCounter>,
    trace_line: F,
    write_trace: W,
}

#[cfg(test)]
pub(super) async fn run_network_pump<S: NetworkSession>(
    session: S,
    sequencer: NetworkSequencer,
    command_rx: mpsc::Receiver<NetworkCommand>,
    control_event_tx: mpsc::Sender<NetworkControlEvent>,
    world_event_tx: mpsc::Sender<WorldIngress>,
    shutdown_rx: watch::Receiver<bool>,
) {
    run_network_pump_with_readiness_ingress(
        session,
        sequencer,
        command_rx,
        control_event_tx,
        world_event_tx,
        shutdown_rx,
        Arc::new(ReadinessIngressCounter::default()),
    )
    .await;
}

#[cfg(test)]
pub(super) async fn run_network_pump_with_trace<S, F, W>(
    session: S,
    sequencer: NetworkSequencer,
    command_rx: mpsc::Receiver<NetworkCommand>,
    control_event_tx: mpsc::Sender<NetworkControlEvent>,
    world_event_tx: mpsc::Sender<WorldIngress>,
    shutdown_rx: watch::Receiver<bool>,
    trace: (F, W),
) where
    S: NetworkSession,
    F: FnMut(u64, &Packet) -> Option<String>,
    W: FnMut(&str),
{
    let (trace_line, write_trace) = trace;
    run_network_pump_with_readiness_ingress_and_trace(
        session,
        sequencer,
        command_rx,
        control_event_tx,
        world_event_tx,
        shutdown_rx,
        NetworkPumpRuntime {
            readiness_ingress: Arc::new(ReadinessIngressCounter::default()),
            trace_line,
            write_trace,
        },
    )
    .await;
}

pub(super) async fn run_network_pump_with_readiness_ingress<S: NetworkSession>(
    session: S,
    sequencer: NetworkSequencer,
    command_rx: mpsc::Receiver<NetworkCommand>,
    control_event_tx: mpsc::Sender<NetworkControlEvent>,
    world_event_tx: mpsc::Sender<WorldIngress>,
    shutdown_rx: watch::Receiver<bool>,
    readiness_ingress: Arc<ReadinessIngressCounter>,
) {
    run_network_pump_with_readiness_ingress_and_trace(
        session,
        sequencer,
        command_rx,
        control_event_tx,
        world_event_tx,
        shutdown_rx,
        NetworkPumpRuntime {
            readiness_ingress,
            trace_line: pending_trace_line,
            write_trace: write_trace_line,
        },
    )
    .await;
}

async fn run_network_pump_with_readiness_ingress_and_trace<S, F, W>(
    mut session: S,
    mut sequencer: NetworkSequencer,
    mut command_rx: mpsc::Receiver<NetworkCommand>,
    control_event_tx: mpsc::Sender<NetworkControlEvent>,
    world_event_tx: mpsc::Sender<WorldIngress>,
    mut shutdown_rx: watch::Receiver<bool>,
    runtime: NetworkPumpRuntime<F, W>,
) where
    S: NetworkSession,
    F: FnMut(u64, &Packet) -> Option<String>,
    W: FnMut(&str),
{
    let NetworkPumpRuntime {
        readiness_ingress,
        mut trace_line,
        mut write_trace,
    } = runtime;
    let mut pump_preference = NetworkPumpPreference::Inbound;
    let mut pending_world_event = None;
    let mut last_blob_cache_stats = None;
    if session.blob_cache_enabled() {
        let stats = session.blob_cache_stats();
        emit_blob_cache_telemetry(stats);
        if !send_control_event_or_cancel(
            &control_event_tx,
            &mut shutdown_rx,
            NetworkControlEvent::BlobCacheTelemetry {
                enabled: true,
                stats,
            },
        )
        .await
        {
            return;
        }
        last_blob_cache_stats = Some(stats);
    }

    async fn end_pump_with_transfer<S: NetworkSession>(
        session: &S,
        pending: Option<WorldIngress>,
        transfer: protocol::ServerTransferEvent,
        world_event_tx: &mpsc::Sender<WorldIngress>,
        control_event_tx: &mpsc::Sender<NetworkControlEvent>,
        shutdown_rx: &mut watch::Receiver<bool>,
    ) {
        if let Some(pending) = pending
            && !send_event_or_cancel(world_event_tx, shutdown_rx, pending).await
        {
            return;
        }
        send_final_blob_cache_telemetry(session, control_event_tx).await;
        let target = SessionTransferTarget {
            host: transfer.host,
            port: transfer.port,
        };
        emit_network_pump_transfer_marker(
            &target,
            transfer.reload_world,
            session.decode_error_count(),
        );
        let _ = send_control_event_or_cancel(
            control_event_tx,
            shutdown_rx,
            NetworkControlEvent::Transferred {
                target,
                decode_error_count: session.decode_error_count(),
            },
        )
        .await;
    }

    loop {
        match wait_for_network_work_or_cancel(
            wait_for_world_side_work(
                &mut session,
                sequencer.current_dimension(),
                &world_event_tx,
                pending_world_event.is_some(),
            ),
            command_rx.recv(),
            &mut shutdown_rx,
            &mut pump_preference,
        )
        .await
        {
            NetworkPumpWork::Shutdown => break,
            NetworkPumpWork::Command(command) => match command {
                Some(NetworkCommand::Send {
                    packet,
                    sub_chunk,
                    chat,
                    physics,
                    physics_reanchor,
                    mining,
                }) => {
                    if let (Some(identity), Some(reanchor)) = (physics, physics_reanchor.as_ref())
                        && *reanchor.borrow() != identity.reanchor_epoch
                    {
                        if !send_control_event_or_cancel(
                            &control_event_tx,
                            &mut shutdown_rx,
                            NetworkControlEvent::PhysicsPacketCancelled {
                                identity,
                                definitely_unsent: true,
                            },
                        )
                        .await
                        {
                            return;
                        }
                        continue;
                    }
                    let packet = finalize_mining_packet(packet, mining);
                    // Every physics trace is formatted from the final packet
                    // after mining sanitization and published in socket-write
                    // order only after that write succeeds.
                    let movement_trace_line = physics
                        .and_then(|identity| trace_line(identity.session_generation, &packet));
                    let trace_armed = chat.is_some_and(|chat| chat.fast_transfer_action.is_some());
                    if trace_armed {
                        session.begin_packet_id_trace();
                    }
                    // Command dequeue is the last point where a physics packet is
                    // provably unsent. Once the socket write starts, let it finish:
                    // racing a reanchor against an in-flight write cannot establish
                    // whether the server observed the old packet, and cancelling it
                    // here used to disable production movement on routine corrections.
                    let send_outcome =
                        wait_for_send_or_cancel(session.send_packet(packet), &mut shutdown_rx)
                            .await;
                    match send_outcome {
                        None => {
                            if trace_armed {
                                session.cancel_packet_id_trace();
                            }
                            if *shutdown_rx.borrow() {
                                break;
                            }
                        }
                        Some(Ok(())) => {
                            if let Some(line) = movement_trace_line {
                                write_trace(&line);
                            }
                            if trace_armed {
                                session.arm_blob_cache_reset_for_fast_transfer();
                            }
                            if let Some(identity) = physics
                                && !send_control_event_or_cancel(
                                    &control_event_tx,
                                    &mut shutdown_rx,
                                    NetworkControlEvent::PhysicsPacketSent { identity },
                                )
                                .await
                            {
                                return;
                            }
                            if let Some(marker) = chat.and_then(|chat| {
                                chat.fast_transfer_action.map(|action| {
                                    let sent_unix_ms = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .map(|duration| {
                                            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
                                        })
                                        .unwrap_or(0);
                                    action.marker(chat.session, chat.sequence, sent_unix_ms)
                                })
                            }) {
                                write_stdout_marker(&mut std::io::stdout().lock(), &marker);
                            }
                            if let Some(sub_chunk) = sub_chunk {
                                let sent_at = Instant::now();
                                if !send_control_event_or_cancel(
                                    &control_event_tx,
                                    &mut shutdown_rx,
                                    NetworkControlEvent::SubChunkRequestSent {
                                        chunk: sub_chunk.chunk,
                                        base_sub_chunk_y: sub_chunk.base_sub_chunk_y,
                                        count: sub_chunk.count,
                                        sent_at,
                                    },
                                )
                                .await
                                {
                                    return;
                                }
                            }
                            if let Some(chat) =
                                chat.filter(|chat| chat.fast_transfer_action.is_some())
                            {
                                if let Some(pending) = pending_world_event.take()
                                    && !send_event_or_cancel(
                                        &world_event_tx,
                                        &mut shutdown_rx,
                                        pending,
                                    )
                                    .await
                                {
                                    return;
                                }
                                let barrier = sequencer.wrap_fast_transfer_barrier(chat.sequence);
                                if !send_event_or_cancel(&world_event_tx, &mut shutdown_rx, barrier)
                                    .await
                                {
                                    return;
                                }
                            }
                            if let Some(chat) = chat
                                && !send_control_event_or_cancel(
                                    &control_event_tx,
                                    &mut shutdown_rx,
                                    NetworkControlEvent::ChatPacketSent {
                                        session: chat.session,
                                        sequence: chat.sequence,
                                    },
                                )
                                .await
                            {
                                return;
                            }
                        }
                        Some(Err(error)) => {
                            if trace_armed {
                                session.cancel_packet_id_trace();
                            }
                            if let Some(transfer) = session.take_server_transfer() {
                                end_pump_with_transfer(
                                    &session,
                                    pending_world_event.take(),
                                    transfer,
                                    &world_event_tx,
                                    &control_event_tx,
                                    &mut shutdown_rx,
                                )
                                .await;
                                return;
                            }
                            let server_disconnect = session.take_server_disconnect();
                            if let Some(chat) = chat {
                                let _ = send_control_event_or_cancel(
                                    &control_event_tx,
                                    &mut shutdown_rx,
                                    NetworkControlEvent::ChatPacketSendFailed {
                                        session: chat.session,
                                        sequence: chat.sequence,
                                        message: error.to_string(),
                                    },
                                )
                                .await;
                            }
                            emit_network_pump_terminal_marker(
                                "send",
                                &error.to_string(),
                                session.decode_error_count(),
                                server_disconnect.as_ref(),
                            );
                            send_final_blob_cache_telemetry(&session, &control_event_tx).await;
                            let _ = send_control_event_or_cancel(
                                &control_event_tx,
                                &mut shutdown_rx,
                                NetworkControlEvent::Failed {
                                    message: error.to_string(),
                                    decode_error_count: session.decode_error_count(),
                                    server_disconnect,
                                    origin: NetworkFailureOrigin::Send,
                                },
                            )
                            .await;
                            return;
                        }
                    }
                }
                None => break,
            },
            NetworkPumpWork::Inbound(WorldSideWork::Capacity(Ok(permit))) => {
                let pending = pending_world_event
                    .take()
                    .expect("world capacity is reserved only for a pending event");
                if let Some(transfer) = session.take_server_transfer() {
                    end_pump_with_transfer(
                        &session,
                        Some(pending),
                        transfer,
                        &world_event_tx,
                        &control_event_tx,
                        &mut shutdown_rx,
                    )
                    .await;
                    return;
                }
                permit.send(pending);
            }
            NetworkPumpWork::Inbound(WorldSideWork::Capacity(Err(_))) => return,
            NetworkPumpWork::Inbound(WorldSideWork::Event(Ok(event))) => {
                emit_packet_id_trace(&mut session);
                try_emit_blob_cache_telemetry(
                    &session,
                    &control_event_tx,
                    &mut last_blob_cache_stats,
                );
                pending_world_event = Some(wrap_inbound_world_event(
                    &mut sequencer,
                    &readiness_ingress,
                    *event,
                ));
                if let Some(transfer) = session.take_server_transfer() {
                    end_pump_with_transfer(
                        &session,
                        pending_world_event.take(),
                        transfer,
                        &world_event_tx,
                        &control_event_tx,
                        &mut shutdown_rx,
                    )
                    .await;
                    return;
                }
            }
            NetworkPumpWork::Inbound(WorldSideWork::Event(Err(error))) => {
                if let Some(transfer) = session.take_server_transfer() {
                    end_pump_with_transfer(
                        &session,
                        pending_world_event.take(),
                        transfer,
                        &world_event_tx,
                        &control_event_tx,
                        &mut shutdown_rx,
                    )
                    .await;
                    return;
                }
                let server_disconnect = session.take_server_disconnect();
                emit_network_pump_terminal_marker(
                    "receive",
                    &error.to_string(),
                    session.decode_error_count(),
                    server_disconnect.as_ref(),
                );
                send_final_blob_cache_telemetry(&session, &control_event_tx).await;
                let _ = send_control_event_or_cancel(
                    &control_event_tx,
                    &mut shutdown_rx,
                    NetworkControlEvent::Failed {
                        message: error.to_string(),
                        decode_error_count: session.decode_error_count(),
                        server_disconnect,
                        origin: NetworkFailureOrigin::Receive,
                    },
                )
                .await;
                return;
            }
        }
    }

    send_final_blob_cache_telemetry(&session, &control_event_tx).await;
    let _ = send_control_event_or_cancel(
        &control_event_tx,
        &mut shutdown_rx,
        NetworkControlEvent::Stopped {
            decode_error_count: session.decode_error_count(),
        },
    )
    .await;
}
