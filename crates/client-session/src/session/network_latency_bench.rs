//! Offline command-to-encode timing with captured skin traffic and the real network pump.

use super::*;
use bytes::Bytes;
use std::sync::mpsc as blocking;

const CODEC_SESSION: protocol::BedrockSession = protocol::BedrockSession { shield_item_id: 0 };

struct Decoded {
    elapsed: Duration,
    allocations: u64,
}

struct Sent {
    selected: Instant,
    completed: Instant,
    wire: Bytes,
    allocations: u64,
}

struct ReplaySession {
    wire: Option<Bytes>,
    decoded: Arc<Mutex<Vec<Decoded>>>,
    sent: blocking::Sender<Sent>,
}

/// Decode and normalize one captured packet through the production owned codec.
fn decode_event(wire: Bytes, dimension: i32) -> WorldEvent {
    let mut packets = protocol::decode_batch(wire, &CODEC_SESSION).unwrap();
    assert_eq!(packets.len(), 1);
    protocol::into_world_event(packets.pop().unwrap(), dimension)
        .unwrap()
        .expect("the fixture must produce a world event")
}

impl NetworkSession for ReplaySession {
    type Error = std::convert::Infallible;

    /// Keep captured packet decoding ready until the real pump applies backpressure.
    async fn receive_world_event(&mut self, dimension: i32) -> Result<WorldEvent, Self::Error> {
        let Some(wire) = self.wire.as_ref() else {
            return future::pending().await;
        };
        let allocations = crate::session::tests::alloc_count::thread_allocations();
        let started = Instant::now();
        let event = decode_event(wire.clone(), dimension);
        let elapsed = started.elapsed();
        let allocations = crate::session::tests::alloc_count::thread_allocations() - allocations;
        self.decoded.lock().unwrap().push(Decoded {
            elapsed,
            allocations,
        });
        Ok(event)
    }

    /// Encode the actual outgoing packet and report completion without a simulated socket delay.
    async fn send_packet(&mut self, packet: protocol::Packet) -> Result<(), Self::Error> {
        let selected = Instant::now();
        let allocations = crate::session::tests::alloc_count::thread_allocations();
        let wire = protocol::encode(&packet, &CODEC_SESSION).unwrap();
        let completed = Instant::now();
        let allocations = crate::session::tests::alloc_count::thread_allocations() - allocations;
        self.sent
            .send(Sent {
                selected,
                completed,
                wire,
                allocations,
            })
            .unwrap();
        Ok(())
    }

    /// The fixed fixture must decode without suppressed errors.
    fn decode_error_count(&self) -> u64 {
        0
    }
}

/// Report measured samples; the cold operation is separate from the warm distribution.
fn distribution(name: &str, values: &[Duration]) {
    let mut warm: Vec<_> = values[1..].iter().map(Duration::as_secs_f64).collect();
    warm.sort_by(f64::total_cmp);
    eprintln!(
        "NETWORK_DISTRIBUTION {name} n={} cold_us={:.3} median_us={:.3} p99_us={:.3} max_us={:.3}",
        warm.len(),
        values[0].as_secs_f64() * 1e6,
        warm[warm.len() / 2] * 1e6,
        warm[warm.len() * 99 / 100] * 1e6,
        warm.last().unwrap() * 1e6,
    );
}

/// Replay ready traffic or a held world queue while sending every fixed simulation tick.
fn run_case(name: &str, wire: Option<Bytes>, hold_world: bool) {
    let ticks: u64 = std::env::var("CINNABAR_NETWORK_BENCH_TICKS")
        .ok()
        .map(|value| value.parse().unwrap())
        .unwrap_or(128);
    assert!((2..=2048).contains(&ticks));
    let interval = Duration::from_secs_f64(1.0 / sim::TICKS_PER_SECOND as f64);
    let decoded = Arc::new(Mutex::new(Vec::new()));
    let (sent_tx, sent_rx) = blocking::channel();
    let mut queued = Vec::new();
    let mut encoded = Vec::new();
    let mut producer_lateness = Vec::new();
    let mut encode_cost = Vec::new();
    let mut send_allocations = Vec::new();
    thread::scope(|scope| {
        let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
        let (control_tx, mut control_rx) = mpsc::channel(CONTROL_EVENT_CAPACITY);
        let (world_tx, mut world_rx) = mpsc::channel(WORLD_EVENT_CAPACITY);
        let (shutdown, shutdown_rx) = watch::channel(false);
        let (release, held) = blocking::channel::<()>();
        let receipt_worker = scope.spawn(move || {
            let mut receipts = Vec::new();
            while let Some(event) = control_rx.blocking_recv() {
                if let NetworkControlEvent::PhysicsPacketSent { identity } = event {
                    receipts.push(identity.tick);
                } else {
                    assert!(matches!(event, NetworkControlEvent::Stopped { .. }));
                }
            }
            receipts
        });
        let world_worker = scope.spawn(move || {
            if hold_world {
                let _ = held.recv();
            }
            let mut consumed = 0;
            while let Some(event) = world_rx.blocking_recv() {
                let WorldIngress::Event(event) = event else {
                    panic!("skin replay must produce ordinary world ingress")
                };
                consumed += 1;
                assert_eq!(event.sequence, consumed);
            }
            consumed
        });
        let session = ReplaySession {
            wire,
            decoded: Arc::clone(&decoded),
            sent: sent_tx,
        };
        let network_worker = scope.spawn(move || {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap()
                .block_on(run_network_pump(
                    session,
                    NetworkSequencer::new(7, 0, 42),
                    command_rx,
                    control_tx,
                    world_tx,
                    shutdown_rx,
                ));
        });
        let epoch = Instant::now();
        for tick in 1..=ticks {
            let packet = traced_movement_packets(tick).1;
            let expected = protocol::encode(&packet, &CODEC_SESSION).unwrap();
            let deadline = epoch + interval * (tick - 1) as u32;
            thread::sleep(deadline.saturating_duration_since(Instant::now()));
            let started = Instant::now();
            commands
                .try_send(NetworkCommand::Send {
                    packet,
                    sub_chunk: None,
                    chat: None,
                    physics: Some(protocol::PhysicsSendIdentity {
                        session_generation: 7,
                        tick,
                        admission_id: tick,
                        reanchor_epoch: 0,
                    }),
                    physics_reanchor: None,
                    interaction: None,
                })
                .unwrap();
            let sent = sent_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            assert_eq!(sent.wire, expected, "movement bytes changed at tick {tick}");
            queued.push(sent.selected - started);
            encoded.push(sent.completed - started);
            encode_cost.push(sent.completed - sent.selected);
            producer_lateness.push(started.saturating_duration_since(deadline));
            send_allocations.push(sent.allocations);
        }
        shutdown.send_replace(true);
        network_worker.join().unwrap();
        assert_eq!(
            receipt_worker.join().unwrap(),
            (1..=ticks).collect::<Vec<_>>()
        );
        drop(release);
        eprintln!(
            "NETWORK_WORKLOAD {name} consumed={}",
            world_worker.join().unwrap()
        );
    });
    distribution(&format!("{name}/queue"), &queued);
    distribution(&format!("{name}/encode_complete"), &encoded);
    distribution(&format!("{name}/encode"), &encode_cost);
    distribution(&format!("{name}/producer_lateness"), &producer_lateness);
    send_allocations.sort_unstable();
    eprintln!(
        "NETWORK_ALLOCATIONS {name}/encode median={}",
        send_allocations[send_allocations.len() / 2]
    );
    let decoded = decoded.lock().unwrap();
    eprintln!("NETWORK_WORKLOAD {name} decoded={}", decoded.len());
    if hold_world {
        assert_eq!(decoded.len(), WORLD_EVENT_CAPACITY + 1);
    }
    if decoded.len() > 1 {
        distribution(
            &format!("{name}/decode"),
            &decoded
                .iter()
                .map(|sample| sample.elapsed)
                .collect::<Vec<_>>(),
        );
        let mut allocations: Vec<_> = decoded.iter().map(|sample| sample.allocations).collect();
        allocations.sort_unstable();
        eprintln!(
            "NETWORK_ALLOCATIONS {name}/decode median={}",
            allocations[allocations.len() / 2]
        );
    }
}

/// Measure real pump scheduling with no inbound packet work.
#[test]
#[ignore = "offline timing fixture; run serially without task builds"]
fn network_latency_idle_bench() {
    run_case("idle", None, false);
}

/// Measure skin decode interference and verify that a full world queue cannot stop movement sends.
#[test]
#[ignore = "requires CINNABAR_NETWORK_BENCH_WIRE; captured bytes stay outside git"]
fn network_latency_skin_bench() {
    let path = std::env::var_os("CINNABAR_NETWORK_BENCH_WIRE").expect("captured packet fixture");
    let wire = Bytes::from(std::fs::read(path).unwrap());
    assert!(matches!(
        decode_event(wire.clone(), 0),
        WorldEvent::Actor(protocol::ActorEvent::PlayerList(_))
    ));
    eprintln!("NETWORK_FIXTURE bytes={}", wire.len());
    run_case("skin_drained", Some(wire.clone()), false);
    run_case("skin_held", Some(wire), true);
}
