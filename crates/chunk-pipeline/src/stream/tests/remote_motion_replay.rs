//! Replays a PocketMine-style remote player walk through the ordered stream and samples its
//! rendered position on the actor frame clock.

use std::collections::VecDeque;

use super::*;

const REMOTE: u64 = 2;
const TICK_NANOS: u64 = simulation_clock::TICK_DURATION.as_nanos() as u64;
const SERVER_TICKS: u64 = 80;
const WALK_PER_TICK: f32 = 0.225;
/// A busy decode pool finishes a streamed column three ticks after it arrives.
const DECODE_NANOS: u64 = 3 * TICK_NANOS;

fn remote_spawn() -> WorldEvent {
    WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
        dimension: 0,
        unique_id: REMOTE as i64,
        runtime_id: REMOTE,
        kind: ActorKind::Player {
            uuid: [2; 16],
            username: "Remote".into(),
        },
        position: [0.5, PLAYER_NETWORK_OFFSET, 8.0],
        velocity: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        body_yaw: 0.0,
        held_item: Default::default(),
        metadata: [].into(),
        attributes: [].into(),
        properties: [].into(),
        links: [].into(),
    }))
}

/// MoveActorAbsolute as PocketMine broadcasts it: full pose, ground flag, never teleport.
fn absolute_move(x: f32) -> WorldEvent {
    WorldEvent::Actor(ActorEvent::Move(ActorMoveEvent {
        dimension: 0,
        runtime_id: REMOTE,
        position: [Some(x), Some(PLAYER_NETWORK_OFFSET), Some(8.0)],
        position_origin: ActorPositionOrigin::NetworkOffset,
        pitch: Some(0.0),
        yaw: Some(-90.0),
        head_yaw: Some(-90.0),
        on_ground: Some(true),
        teleported: false,
        player_mode: None,
        source_tick: None,
        interpolation: Default::default(),
    }))
}

fn local_hunger(tick: u64) -> WorldEvent {
    WorldEvent::Actor(ActorEvent::Attributes(ActorAttributesUpdateEvent {
        dimension: 0,
        runtime_id: 1,
        attributes: Arc::from([ActorAttribute {
            name: "minecraft:player.exhaustion".into(),
            min: 0.0,
            max: 5.0,
            current: (tick % 50) as f32 * 0.1,
            default: Some(0.0),
            modifiers: Arc::from([]),
        }]),
        tick,
    }))
}

/// Wire packets with the nanosecond they reach the client, in ordered-transport order.
fn session(local_moving: bool, worst_case: bool) -> Vec<(u64, WorldEvent)> {
    let mut seed = 0x2545_f491_u32;
    let mut packets = Vec::new();
    let mut arrival = 0;
    for tick in 0..SERVER_TICKS {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        // 30 ms latency with up to two ticks of late jitter, never reordering the stream.
        let jitter = u64::from(seed >> 30) % 3 * TICK_NANOS;
        arrival = (tick * TICK_NANOS + 30_000_000 + jitter).max(arrival);
        let walking = (10..SERVER_TICKS - 10).contains(&tick);
        let x = 0.5 + WALK_PER_TICK * tick.clamp(10, SERVER_TICKS - 10).saturating_sub(10) as f32;
        if tick == 10 || tick == SERVER_TICKS - 10 {
            packets.push((
                arrival,
                WorldEvent::ActorMotion(ActorMotionEvent {
                    actor_runtime_id: REMOTE,
                    motion: if walking {
                        [WALK_PER_TICK, 0.0, 0.0]
                    } else {
                        [0.0; 3]
                    },
                    tick: 0,
                }),
            ));
        }
        if (10..=SERVER_TICKS - 10).contains(&tick) {
            packets.push((arrival, absolute_move(x)));
        }
        if !local_moving {
            continue;
        }
        // Anticheat probes each tick, exhaustion updates and chunk streaming as the local
        // player crosses columns.
        packets.push((arrival, WorldEvent::NetworkStackLatency(tick)));
        if tick % 4 == 0 {
            packets.push((arrival, local_hunger(tick)));
        }
        if tick % 16 == 8 {
            let column = 4 + (tick / 16) as i32;
            packets.push((arrival, inline_air_event(column)));
            if worst_case {
                packets.push((arrival, block_update_at(column)));
            }
        }
        if tick % 10 == 5 {
            packets.push((arrival, block_update_at(0)));
        }
    }
    packets
}

fn block_update_at(chunk_x: i32) -> WorldEvent {
    WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
        dimension: 0,
        position: [chunk_x * 16 + 3, -64, 3],
        layer: 0,
        network_id: 1,
    }])
}

/// Rendered remote x per frame at `fps`, on the actor frame clock that drives placement.
fn replay(fps: u64, local_moving: bool, worst_case: bool) -> Vec<f32> {
    let mut stream = block_entity_visual_stream();
    let mut sequence = 0;
    for column in 0..4 {
        sequence += 1;
        stream.submit(sequence, inline_air_event(column)).unwrap();
    }
    complete_pending_decode_jobs(&mut stream);
    // Local physics owns retention, as in play.
    let owner = stream.authority().actor_session_id();
    let epoch = stream.authority().form_dimension_epoch();
    stream.retain_for_local_player(owner, 0, epoch, [8.0, 0.0, 8.0]);
    assert!(stream.local_player_chunk.is_some());
    sequence += 1;
    stream.submit(sequence, remote_spawn()).unwrap();
    let mut packets = VecDeque::from(session(local_moving, worst_case));
    let mut decoding = VecDeque::new();
    let frame_nanos = 1_000_000_000 / fps;
    let mut accumulated = 0;
    let mut rendered = Vec::new();
    for frame in 0.. {
        let now = frame * frame_nanos;
        if now > (SERVER_TICKS + 10) * TICK_NANOS {
            break;
        }
        while packets.front().is_some_and(|(arrival, _)| *arrival <= now) {
            let (_, event) = packets.pop_front().unwrap();
            sequence += 1;
            stream.submit(sequence, event).unwrap();
        }
        while let Some(job) = stream.pending_decode.pop_front() {
            decoding.push_back((now + DECODE_NANOS, job));
        }
        while decoding.front().is_some_and(|(due, _)| *due <= now) {
            let (_, job) = decoding.pop_front().unwrap();
            complete_decode_job(&mut stream, job);
        }
        stream.apply_ready();
        stream.promote_deferred_ingress();
        // The app drains committed consumers every frame.
        stream.take_committed_controls();
        stream.take_committed_ui();
        stream.take_committed_audio();
        stream.take_committed_particles();
        stream.take_committed_camera();
        accumulated += frame_nanos;
        let ticks = (accumulated / TICK_NANOS) as u32;
        accumulated %= TICK_NANOS;
        stream.predict_remote_actor_motion(ticks);
        stream.advance_actor_interpolation_frame(ticks);
        let alpha = accumulated as f32 / TICK_NANOS as f32;
        let actor = stream.authority().actor(REMOTE).expect("remote player");
        rendered.push(actor.interpolated_position(alpha).unwrap()[0]);
    }
    rendered
}

/// Asserts every walking frame advances by a bounded step: no stall, jump or 20 Hz stepping.
fn assert_continuous(label: &str, path: &[f32], walk: std::ops::Range<usize>, nominal: f32) {
    for (frame, pair) in path[walk.clone()].windows(2).enumerate() {
        let step = pair[1] - pair[0];
        assert!(
            (nominal * 0.2..=nominal * 3.0).contains(&step),
            "{label} frame {}: step {step} against nominal {nominal}",
            walk.start + frame
        );
    }
}

/// A walking remote player renders the same continuous path whether the local player idles
/// or streams terrain, answers latency probes and edits blocks.
#[test]
fn remote_walk_renders_continuously_regardless_of_local_activity() {
    for fps in [60, 144] {
        let idle = replay(fps, false, false);
        let nominal = WALK_PER_TICK * 20.0 / fps as f32;
        let start = idle
            .iter()
            .position(|x| *x > 0.5)
            .expect("the walk renders");
        let end = idle
            .iter()
            .rposition(|x| *x < idle[idle.len() - 1])
            .unwrap();
        assert_continuous(&format!("{fps} fps idle"), &idle, start..end, nominal);
        let busy = replay(fps, true, false);
        assert_eq!(
            idle.iter().zip(&busy).position(|(idle, busy)| idle != busy),
            None,
            "{fps} fps: local activity changed the remote path"
        );
        // A latency probe after a block update on a still-decoding column holds later moves
        // until that column commits; the path may lag a tick but must stay continuous.
        let held = replay(fps, true, true);
        assert_continuous(&format!("{fps} fps held"), &held, start..end, nominal);
        let lag = idle
            .iter()
            .zip(&held)
            .map(|(idle, held)| (idle - held).abs())
            .fold(0.0, f32::max);
        assert!(
            lag <= WALK_PER_TICK * 1.01,
            "{fps} fps: held moves lag {lag}"
        );
    }
}
