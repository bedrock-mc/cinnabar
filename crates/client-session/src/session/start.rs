use super::*;

/// Starts the login task and hands admitted world state to the play pump.
pub fn spawn_network<P: Send + 'static>(
    config: NetworkConfig,
    prepare_presentation: impl FnOnce(
        &PackPreparation,
        &protocol::GameData,
        &(dyn Fn() -> bool + Sync),
    ) -> Option<Result<P, crate::RequiredPackRejected>>
    + Send
    + 'static,
    observation: SessionTrace,
) -> Result<NetworkHandle<P>, std::io::Error> {
    let session_generation = config.session_generation;
    let (control_event_tx, control_events) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (world_event_tx, world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    let (physics_reanchor, _physics_reanchor_rx) = watch::channel(0);
    let (shutdown, mut shutdown_rx) = watch::channel(false);
    let readiness_ingress = Arc::new(ReadinessIngressCounter::default());
    let network_readiness_ingress = Arc::clone(&readiness_ingress);
    let experience_gate = Arc::new(experience::ExperienceGate::default());
    let network_experience_gate = Arc::clone(&experience_gate);
    let thread = thread::Builder::new()
        .name("bedrock-network".to_owned())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = control_event_tx.try_send(NetworkControlEvent::Failed {
                        message: format!("failed to create network runtime: {error}"),
                        decode_error_count: 0,
                        server_disconnect: None,
                        origin: NetworkFailureOrigin::Startup,
                    });
                    return;
                }
            };
            run_session_runtime(runtime, async move {
                let Some(login) = wait_for_login_or_cancel(
                    LoginSequence::connect_session(
                        &config.socket_dir,
                        &config.display_name,
                        Some(config.client_blob_cache.clone()),
                        Some(config.player_skin),
                    ),
                    &mut shutdown_rx,
                )
                .await
                else {
                    return;
                };
                let (mut session, game_data) = match login {
                    Ok(connected) => connected,
                    Err(error) => {
                        send_login_error(&control_event_tx, &mut shutdown_rx, error).await;
                        return;
                    }
                };
                tracing::info!(
                    base_game_version = %game_data.start_game.settings.base_game_version,
                    "world lighting compatibility version"
                );
                // The login handoff is one-shot. Take and validate it before
                // publishing any StartGame state; optional semantic rejection
                // remains a live base-assets session, a required one ends it.
                let handoff = session.take_resource_pack_handoff();
                let cancelled = shutdown_rx.clone();
                let Some((session_packs, game_data, packs)) = run_blocking_or_cancel(
                    move || {
                        let (session_packs, packs) = prepare_session(
                            handoff,
                            &game_data,
                            config.physical_memory_bytes,
                            &cancelled,
                            prepare_presentation,
                        );
                        (session_packs, game_data, packs)
                    },
                    &mut shutdown_rx,
                )
                .await
                else {
                    return;
                };
                let packs = match packs {
                    None => return,
                    Some(Ok(packs)) => packs,
                    Some(Err(error)) => {
                        send_startup_failure(&control_event_tx, &mut shutdown_rx, error, None)
                            .await;
                        return;
                    }
                };
                let SessionPacks {
                    custom_blocks,
                    applied: packs_applied,
                } = session_packs;
                let bootstrap = WorldBootstrap::from_game_data(&game_data);
                let server_authoritative_block_breaking =
                    protocol::server_authoritative_block_breaking(&game_data);
                let environment = WorldEnvironmentBootstrap::from_game_data(&game_data);
                let hardcore = protocol::is_hardcore(&game_data);
                let hud_rules = protocol::HudRules::from_game_data(&game_data);
                let death_rules = protocol::DeathRules::from_game_data(&game_data);
                let inventory = start_game_inventory_authority(&game_data);
                let item_registry = match start_game_item_registry(&game_data, bootstrap.dimension)
                {
                    Ok(registry) => registry,
                    Err(error) => {
                        send_startup_failure(&control_event_tx, &mut shutdown_rx, error, None)
                            .await;
                        return;
                    }
                };
                let player_game_mode = PlayerGameMode::from_game_data(&game_data);
                let world_default_game_mode =
                    PlayerGameMode::world_default_update_from_game_data(&game_data);
                let player_game_mode_uses_world_default =
                    PlayerGameMode::bootstrap_uses_world_default(&game_data);
                if !send_control_event_or_cancel(
                    &control_event_tx,
                    &mut shutdown_rx,
                    NetworkControlEvent::Bootstrap {
                        session_generation,
                        world: bootstrap,
                        environment,
                        custom_blocks,
                        inventory,
                        item_registry,
                        player_game_mode,
                        world_default_game_mode,
                        player_game_mode_uses_world_default,
                        server_authoritative_block_breaking,
                        rewind_history_size: protocol::rewind_history_size(&game_data),
                        hardcore,
                        hud_rules,
                        death_rules,
                        packs,
                        terrain_before_spawn: session.terrain_before_spawn(),
                    },
                )
                .await
                {
                    return;
                }
                let sequencer = NetworkSequencer::new(
                    session_generation,
                    bootstrap.dimension,
                    bootstrap.local_player_runtime_id,
                );
                let pump = async {
                    run_network_pump_with_readiness_ingress(
                        session,
                        sequencer,
                        command_rx,
                        control_event_tx,
                        world_event_tx,
                        shutdown_rx,
                        (network_readiness_ingress, network_experience_gate),
                        observation,
                    )
                    .await;
                };
                if packs_applied {
                    let socket_dir = config.socket_dir.clone();
                    run_with_pack_report(pump, move |applied| {
                        let socket_dir = socket_dir.clone();
                        async move {
                            protocol::report_pack_application(&socket_dir, applied).await;
                        }
                    })
                    .await;
                } else {
                    pump.await;
                }
            });
        })?;
    Ok(NetworkHandle {
        session_generation,
        control_events,
        world_events,
        commands,
        pending_latency_reply: Mutex::new(None),
        physics_reanchor,
        shutdown,
        thread: Some(thread),
        readiness_ingress,
        experience_gate,
        unflushed: AtomicBool::new(false),
    })
}

/// What a session keeps of its pack preparation once presentation is prepared.
struct SessionPacks {
    custom_blocks: protocol::CustomBlocks,
    applied: bool,
}

/// Validates and prepares the login handoff unless cancelled, retaining needed session facts.
/// Reused presentation carries the retained archives so equivalent packs have one copy.
fn prepare_session<P>(
    handoff: protocol::ResourcePackHandoff,
    game_data: &protocol::GameData,
    physical_memory_bytes: u64,
    cancelled: &watch::Receiver<bool>,
    prepare_presentation: impl FnOnce(
        &PackPreparation,
        &protocol::GameData,
        &(dyn Fn() -> bool + Sync),
    ) -> Option<Result<P, crate::RequiredPackRejected>>,
) -> (SessionPacks, Option<Result<P, crate::RequiredPackRejected>>) {
    let preparation = crate::prepare_session_packs(handoff, game_data, physical_memory_bytes);
    let packs = unless_cancelled(cancelled, || {
        prepare_presentation(&preparation, game_data, &|| *cancelled.borrow())
    })
    .flatten();
    let session_packs = SessionPacks {
        custom_blocks: preparation.inputs.blocks.clone(),
        applied: preparation.has_applied_packs(),
    };
    (session_packs, packs)
}

/// Drives the session without waiting on a cancelled preparation that is still compiling.
fn run_session_runtime(runtime: tokio::runtime::Runtime, session: impl Future<Output = ()>) {
    runtime.block_on(session);
    runtime.shutdown_background();
}

/// Routes startup transfers to the same reconnect owner as play-phase transfers.
async fn send_login_error<P>(
    controls: &mpsc::Sender<NetworkControlEvent<P>>,
    shutdown: &mut watch::Receiver<bool>,
    error: protocol::ProtocolError,
) {
    if let Some(transfer) = error.server_transfer() {
        send_startup_transfer(controls, shutdown, transfer).await;
    } else {
        let disconnect = error.server_disconnect();
        send_startup_failure(controls, shutdown, error, disconnect).await;
    }
}

/// Publishes the terminal transfer only while the startup task still owns the session.
async fn send_startup_transfer<P>(
    controls: &mpsc::Sender<NetworkControlEvent<P>>,
    shutdown: &mut watch::Receiver<bool>,
    transfer: protocol::ServerTransferEvent,
) {
    let target = SessionTransferTarget {
        host: transfer.host,
        port: transfer.port,
    };
    emit_network_pump_transfer_marker(&target, transfer.reload_world, 0);
    let _ = send_control_event_or_cancel(
        controls,
        shutdown,
        NetworkControlEvent::Transferred {
            target,
            decode_error_count: 0,
        },
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct LoadingSession(std::sync::Arc<std::sync::atomic::AtomicUsize>);

    impl NetworkSession for LoadingSession {
        type Error = &'static str;
        type Outbound = super::super::tests::PacketOutbound<&'static str>;

        fn outbound(&mut self) -> Result<Self::Outbound, Self::Error> {
            let calls = Arc::clone(&self.0);
            Ok(super::super::tests::PacketOutbound::new(move |_| {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                std::future::ready(Err("completion write failed"))
            })
            .with_finish_loading(vec![protocol::modal_form_cancel_response(1)]))
        }

        async fn receive_world_event(&mut self, _: i32) -> Result<WorldEvent, Self::Error> {
            std::future::pending().await
        }

        fn decode_error_count(&self) -> u64 {
            0
        }
    }

    #[tokio::test]
    async fn pump_waits_for_readiness_command_and_reports_completion_send_failure() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (commands, command_rx) = mpsc::channel(2);
        let (controls, mut events) = mpsc::channel(1);
        let (world, _world_rx) = mpsc::channel(1);
        let (_shutdown, shutdown_rx) = watch::channel(false);
        let mut pump = std::pin::pin!(run_network_pump(
            LoadingSession(Arc::clone(&calls)),
            NetworkSequencer::new(1, 0, 42),
            command_rx,
            controls,
            world,
            shutdown_rx
        ));
        assert!(
            std::future::poll_fn(|cx| std::task::Poll::Ready(pump.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        commands.try_send(NetworkCommand::FinishLoading).unwrap();
        commands.try_send(NetworkCommand::FlushFrame).unwrap();
        pump.await;
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(matches!(
            events.recv().await,
            Some(NetworkControlEvent::Failed {
                origin: NetworkFailureOrigin::Send,
                ..
            })
        ));
    }

    // Cancelling during pack preparation ends the network thread while the compile still runs.
    #[test]
    fn cancelled_preparation_releases_the_network_thread() {
        let (release, blocked) = std::sync::mpsc::channel::<()>();
        let (finished, thread_done) = std::sync::mpsc::channel();
        let (shutdown, mut shutdown_rx) = watch::channel(false);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let thread = thread::spawn(move || {
            run_session_runtime(runtime, async move {
                let prepared =
                    run_blocking_or_cancel(move || blocked.recv().is_ok(), &mut shutdown_rx).await;
                assert!(prepared.is_none());
            });
            finished.send(()).unwrap();
        });
        shutdown.send_replace(true);
        let ended = thread_done.recv_timeout(std::time::Duration::from_secs(5));
        let _ = release.send(());
        thread.join().unwrap();
        ended.expect("the network thread outlived its cancelled preparation");
    }

    // Shutdown must not wait on the watch channel while a compile is running.
    #[test]
    fn shutdown_returns_while_presentation_compiles() {
        let (shutdown, cancelled) = watch::channel(false);
        let (started, compiling) = std::sync::mpsc::channel();
        let (release, blocked) = std::sync::mpsc::channel::<()>();
        let worker = thread::spawn(move || {
            unless_cancelled(&cancelled, || {
                started.send(()).unwrap();
                blocked.recv().is_ok()
            })
        });
        compiling.recv().unwrap();
        let (sent, shutdown_done) = std::sync::mpsc::channel();
        let notifier = thread::spawn(move || {
            shutdown.send_replace(true);
            sent.send(()).unwrap();
        });
        let returned = shutdown_done.recv_timeout(std::time::Duration::from_secs(5));
        release.send(()).unwrap();
        assert_eq!(worker.join().unwrap(), Some(true));
        notifier.join().unwrap();
        returned.expect("shutdown blocked behind the running compile");
    }

    // A presentation that reuses an earlier compile holds its own copy of the archives, so the
    // session must not also keep the copy this login validated.
    #[test]
    fn preparing_a_session_releases_the_archives_it_validated() {
        let archive = protocol::ResourcePackArchive::unencrypted(
            "11111111-2222-3333-4444-555555555555".parse().unwrap(),
            "1.2.3".into(),
            String::new(),
            vec![0; 32],
        );
        let game_data = protocol::GameData {
            start_game: Default::default(),
            item_registry: Default::default(),
            biome_definitions: None,
            entity_identifiers: None,
            creative_content: None,
        };
        let (_shutdown, cancelled) = watch::channel(false);
        let mut validated = None;
        let (session_packs, packs) = prepare_session(
            protocol::ResourcePackHandoff::from_archives(vec![archive]),
            &game_data,
            u64::MAX,
            &cancelled,
            |preparation, _, _| {
                let resource_pack::PackAdmission::Validated(stack) = &preparation.admission else {
                    panic!("the handoff admits a stack");
                };
                validated = Some(Arc::downgrade(stack));
                Some(Ok(()))
            },
        );
        assert!(matches!(packs, Some(Ok(()))));
        assert!(!session_packs.applied, "the only pack was rejected");
        assert!(session_packs.custom_blocks.blocks.is_empty());
        assert!(validated.unwrap().upgrade().is_none());
    }

    #[tokio::test]
    async fn uncancelled_preparation_returns_its_output() {
        let (_shutdown, mut shutdown_rx) = watch::channel(false);
        assert_eq!(
            run_blocking_or_cancel(|| 7, &mut shutdown_rx).await,
            Some(7)
        );
    }

    #[tokio::test]
    async fn startup_transfer_reaches_reconnect_unless_cancelled() {
        for cancelled in [false, true] {
            let (controls, mut events) = mpsc::channel(1);
            let (_shutdown, mut receiver) = watch::channel(cancelled);
            let transfer = protocol::ServerTransferEvent {
                host: "next.example.test".into(),
                port: 19133,
                reload_world: false,
            };
            send_startup_transfer::<()>(&controls, &mut receiver, transfer.clone()).await;
            drop(controls);
            let event = events.recv().await;
            if cancelled {
                assert!(event.is_none(), "cancelled joins must not start a redial");
            } else {
                assert!(
                    matches!(event, Some(NetworkControlEvent::Transferred { target, .. })
                    if target.host == transfer.host && target.port == transfer.port)
                );
                assert!(
                    events.recv().await.is_none(),
                    "transfer must not become a failure"
                );
            }
        }
    }
}

/// Keeps application and reversion reports around an admitted session.
async fn run_with_pack_report<P, R, F>(pump: P, report: R)
where
    P: std::future::Future<Output = ()>,
    R: Fn(bool) -> F,
    F: std::future::Future<Output = ()>,
{
    report(true).await;
    pump.await;
    report(false).await;
}

#[cfg(test)]
mod pack_report_tests {
    use super::run_with_pack_report;
    #[tokio::test]
    async fn review_pack_application_finishes_before_the_pump_and_reversion() {
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let pump_events = events.clone();
        let report_events = events.clone();
        run_with_pack_report(
            async move {
                pump_events.lock().unwrap().push("pump");
            },
            move |applied| {
                let events = report_events.clone();
                async move {
                    if applied {
                        tokio::task::yield_now().await;
                    }
                    events
                        .lock()
                        .unwrap()
                        .push(if applied { "applied" } else { "reverted" });
                }
            },
        )
        .await;
        assert_eq!(*events.lock().unwrap(), ["applied", "pump", "reverted"]);
    }
}
