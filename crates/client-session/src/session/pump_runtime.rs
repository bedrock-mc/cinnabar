use super::*;

struct NetworkPumpRuntime<F, W> {
    readiness_ingress: Arc<ReadinessIngressCounter>,
    experience_gate: Arc<experience::ExperienceGate>,
    trace_line: F,
    write_trace: W,
    observation: SessionTrace,
}

#[cfg(test)]
pub(super) async fn run_network_pump<S: NetworkSession>(
    session: S,
    sequencer: NetworkSequencer,
    command_rx: mpsc::Receiver<NetworkCommand>,
    control_event_tx: mpsc::Sender<NetworkControlEvent<()>>,
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
        (Arc::default(), Arc::default()),
        SessionTrace::default(),
    )
    .await;
}

#[cfg(test)]
pub(super) async fn run_network_pump_with_trace<S, F, W>(
    session: S,
    sequencer: NetworkSequencer,
    command_rx: mpsc::Receiver<NetworkCommand>,
    control_event_tx: mpsc::Sender<NetworkControlEvent<()>>,
    world_event_tx: mpsc::Sender<WorldIngress>,
    shutdown_rx: watch::Receiver<bool>,
    trace: (F, W),
) where
    S: NetworkSession,
    F: FnMut(u64, &Packet) -> Option<String> + Send + 'static,
    W: FnMut(&str) + Send + 'static,
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
            experience_gate: Arc::default(),
            trace_line,
            write_trace,
            observation: SessionTrace::default(),
        },
    )
    .await;
}

#[allow(clippy::too_many_arguments)] // Keep the distinct transport endpoints explicit.
pub(super) async fn run_network_pump_with_readiness_ingress<
    S: NetworkSession,
    P: Send + 'static,
>(
    session: S,
    sequencer: NetworkSequencer,
    command_rx: mpsc::Receiver<NetworkCommand>,
    control_event_tx: mpsc::Sender<NetworkControlEvent<P>>,
    world_event_tx: mpsc::Sender<WorldIngress>,
    shutdown_rx: watch::Receiver<bool>,
    gates: (
        Arc<ReadinessIngressCounter>,
        Arc<experience::ExperienceGate>,
    ),
    observation: SessionTrace,
) {
    let (readiness_ingress, experience_gate) = gates;
    run_network_pump_with_readiness_ingress_and_trace(
        session,
        sequencer,
        command_rx,
        control_event_tx,
        world_event_tx,
        shutdown_rx,
        NetworkPumpRuntime {
            readiness_ingress,
            experience_gate,
            trace_line: observation.movement_line,
            write_trace: observation.write_movement,
            observation,
        },
    )
    .await;
}

async fn run_network_pump_with_readiness_ingress_and_trace<S, F, W, P>(
    mut session: S,
    mut sequencer: NetworkSequencer,
    command_rx: mpsc::Receiver<NetworkCommand>,
    control_event_tx: mpsc::Sender<NetworkControlEvent<P>>,
    world_event_tx: mpsc::Sender<WorldIngress>,
    mut shutdown_rx: watch::Receiver<bool>,
    runtime: NetworkPumpRuntime<F, W>,
) where
    S: NetworkSession,
    F: FnMut(u64, &Packet) -> Option<String> + Send + 'static,
    W: FnMut(&str) + Send + 'static,
    P: Send + 'static,
{
    let NetworkPumpRuntime {
        readiness_ingress,
        experience_gate,
        trace_line,
        write_trace,
        observation,
    } = runtime;
    let outbound = match session.outbound() {
        Ok(outbound) => outbound,
        Err(error) => {
            let _ = send_control_event_or_cancel(
                &control_event_tx,
                &mut shutdown_rx,
                NetworkControlEvent::Failed {
                    message: error.to_string(),
                    decode_error_count: session.decode_error_count(),
                    server_disconnect: session.take_server_disconnect(),
                    origin: NetworkFailureOrigin::Startup,
                },
            )
            .await;
            return;
        }
    };
    let (hooks, mut hook_rx) = mpsc::unbounded_channel();
    let _outbound_task = OutboundTask(tokio::spawn(run_outbound(OutboundRuntime {
        outbound,
        commands: command_rx,
        control_event_tx: control_event_tx.clone(),
        hooks,
        shutdown_rx: shutdown_rx.clone(),
        experience_gate: Arc::clone(&experience_gate),
        trace_line,
        write_trace,
        fast_transfer_action_marker: observation.fast_transfer_action_marker,
    })));
    // Holds the command receiver after a write failure until the terminal event is queued.
    let mut _failed_commands = None;
    // Between arming a transfer trace and its write result, nothing inbound is decoded, so
    // the transfer barrier lands exactly at the write.
    let mut awaiting_traced_write = false;
    let experience_start = Instant::now();
    let mut experience_rate = None;
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

    async fn end_pump_with_transfer<S: NetworkSession, P>(
        session: &S,
        pending: Option<WorldIngress>,
        transfer: protocol::ServerTransferEvent,
        world_event_tx: &mpsc::Sender<WorldIngress>,
        control_event_tx: &mpsc::Sender<NetworkControlEvent<P>>,
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
        let inbound = async {
            if awaiting_traced_write {
                std::future::pending().await
            } else {
                wait_for_world_side_work(
                    &mut session,
                    sequencer.current_dimension(),
                    &world_event_tx,
                    pending_world_event.is_some(),
                )
                .await
            }
        };
        match wait_for_network_work_or_cancel(inbound, hook_rx.recv(), &mut shutdown_rx).await {
            NetworkPumpWork::Shutdown => break,
            NetworkPumpWork::Hook(None | Some(PumpHook::CommandsClosed)) => break,
            NetworkPumpWork::Hook(Some(PumpHook::BeginTrace(armed))) => {
                session.begin_packet_id_trace();
                awaiting_traced_write = true;
                let _ = armed.send(());
            }
            NetworkPumpWork::Hook(Some(PumpHook::TracedWriteSettled)) => {
                awaiting_traced_write = false;
            }
            NetworkPumpWork::Hook(Some(PumpHook::FastTransferSent(chat))) => {
                session.arm_blob_cache_reset_for_fast_transfer();
                if let Some(pending) = pending_world_event.take()
                    && !send_event_or_cancel(&world_event_tx, &mut shutdown_rx, pending).await
                {
                    return;
                }
                let barrier = sequencer.wrap_fast_transfer_barrier(chat.sequence);
                if !send_event_or_cancel(&world_event_tx, &mut shutdown_rx, barrier).await {
                    return;
                }
                if !send_control_event_or_cancel(
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
            NetworkPumpWork::Hook(Some(PumpHook::SendFailed {
                message,
                chats,
                trace_armed,
                commands,
            })) => {
                _failed_commands = Some(commands);
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
                for chat in chats {
                    let _ = send_control_event_or_cancel(
                        &control_event_tx,
                        &mut shutdown_rx,
                        NetworkControlEvent::ChatPacketSendFailed {
                            session: chat.session,
                            sequence: chat.sequence,
                            message: message.clone(),
                        },
                    )
                    .await;
                }
                emit_network_pump_terminal_marker(
                    "send",
                    &message,
                    session.decode_error_count(),
                    server_disconnect.as_ref(),
                );
                send_final_blob_cache_telemetry(&session, &control_event_tx).await;
                let _ = send_control_event_or_cancel(
                    &control_event_tx,
                    &mut shutdown_rx,
                    NetworkControlEvent::Failed {
                        message,
                        decode_error_count: session.decode_error_count(),
                        server_disconnect,
                        origin: NetworkFailureOrigin::Send,
                    },
                )
                .await;
                return;
            }
            NetworkPumpWork::Inbound(WorldSideWork::Capacity(Ok(permit))) => {
                let pending = pending_world_event
                    .take()
                    .expect("world capacity is reserved only for a pending event");
                permit.send(pending);
                if let Some(transfer) = session.take_server_transfer() {
                    end_pump_with_transfer(
                        &session,
                        None,
                        transfer,
                        &world_event_tx,
                        &control_event_tx,
                        &mut shutdown_rx,
                    )
                    .await;
                    return;
                }
            }
            NetworkPumpWork::Inbound(WorldSideWork::Capacity(Err(_))) => return,
            NetworkPumpWork::Inbound(WorldSideWork::Event(Ok(event))) => {
                let now_ms =
                    u64::try_from(experience_start.elapsed().as_millis()).unwrap_or(u64::MAX);
                if !experience_gate.admit(&event, &mut experience_rate, now_ms) {
                    continue;
                }
                emit_packet_id_trace(&mut session, &observation);
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
