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
        force_local_move: false,
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

/// How server ticks reach the client.
#[derive(Clone, Copy, Debug)]
enum Arrival {
    /// Zero to two ticks late at random.
    Jitter,
    /// Four ticks' packets arrive together.
    Bunched,
}

/// One rendered frame of the remote walk.
#[derive(Clone, Copy)]
struct Frame {
    /// Vanilla's drawn position: the previous and current tick poses at the frame fraction.
    render: f32,
    /// The current tick pose that picks read, without frame interpolation.
    pick: f32,
    /// Newest received target.
    target: f32,
    nanos: u64,
}

/// Wire packets with the nanosecond they reach the client, in ordered-transport order.
fn session(
    local_moving: bool,
    worst_case: bool,
    arrival_pattern: Arrival,
) -> Vec<(u64, WorldEvent)> {
    let mut seed = 0x2545_f491_u32;
    let mut packets = Vec::new();
    let mut arrival = 0;
    for tick in 0..SERVER_TICKS {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        // 30 ms latency plus late jitter or bunching, never reordering the stream.
        let jitter = match arrival_pattern {
            Arrival::Jitter => u64::from(seed >> 30) % 3 * TICK_NANOS,
            Arrival::Bunched => (3 - tick % 4) * TICK_NANOS,
        };
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
    replay_frames(fps, local_moving, worst_case, Arrival::Jitter)
        .into_iter()
        .map(|frame| frame.render)
        .collect()
}

fn replay_frames(fps: u64, local_moving: bool, worst_case: bool, arrival: Arrival) -> Vec<Frame> {
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
    let mut packets = VecDeque::from(session(local_moving, worst_case, arrival));
    let mut decoding = VecDeque::new();
    let frame_nanos = 1_000_000_000 / fps;
    let mut accumulated = 0;
    let mut rendered = Vec::new();
    let mut target = 0.5;
    for frame in 0.. {
        let now = frame * frame_nanos;
        if now > (SERVER_TICKS + 10) * TICK_NANOS {
            break;
        }
        while packets.front().is_some_and(|(arrival, _)| *arrival <= now) {
            let (_, event) = packets.pop_front().unwrap();
            if let WorldEvent::Actor(ActorEvent::Move(movement)) = &event {
                target = movement.position[0].unwrap();
            }
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
        rendered.push(Frame {
            render: actor.interpolated_position(alpha).unwrap()[0],
            pick: actor.position[0],
            target,
            nanos: now,
        });
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

/// Vanilla's own render-to-pick distance this frame, which bounds every candidate.
fn corridor(frame: &Frame) -> f32 {
    (frame.pick - frame.render).abs()
}

/// Finite-duration decay: one at receipt, zero from 100 ms.
fn receipt_weight(nanos: u64) -> f32 {
    let millis = nanos as f32 / 1.0e6;
    let tail = (-100.0_f32 / 40.0).exp();
    if millis >= 100.0 {
        0.0
    } else {
        ((-millis / 40.0).exp() - tail) / (1.0 - tail)
    }
}

/// Decays from the last drawn position toward each new target, projected onto the segment
/// from the pick toward that target within the corridor.
fn receipt_anchored(frames: &[Frame]) -> Vec<f32> {
    let (mut target, mut received, mut anchor) = (frames[0].target, 0, frames[0].render);
    let mut drawn = frames[0].render;
    frames
        .iter()
        .map(|frame| {
            if frame.target != target {
                (target, received, anchor) = (frame.target, frame.nanos, drawn);
            }
            let candidate = target + (anchor - target) * receipt_weight(frame.nanos - received);
            let ahead = target - frame.pick;
            let along = ((candidate - frame.pick) * ahead.signum())
                .clamp(0.0, ahead.abs().min(corridor(frame)));
            drawn = frame.pick + along * ahead.signum();
            drawn
        })
        .collect()
}

/// Integrates a low-passed vanilla render velocity within the corridor around the pick, short
/// of the newest target; `lead_only` also keeps it at or ahead of vanilla's render.
fn velocity_tube(frames: &[Frame], tau_millis: f32, lead_only: bool) -> Vec<f32> {
    let (mut drawn, mut velocity) = (frames[0].render, 0.0);
    let mut previous = frames[0];
    frames
        .iter()
        .map(|frame| {
            let elapsed = (frame.nanos - previous.nanos) as f32 / 1.0e6;
            if elapsed > 0.0 {
                let render_velocity = (frame.render - previous.render) / elapsed;
                velocity += (render_velocity - velocity) * (1.0 - (-elapsed / tau_millis).exp());
                drawn += velocity * elapsed;
            }
            previous = *frame;
            let width = corridor(frame);
            let ahead = (frame.pick + width).min(frame.target.max(frame.pick));
            let behind = if lead_only {
                frame.render.max(frame.pick - width)
            } else {
                frame.pick - width
            };
            drawn = drawn.clamp(behind.min(ahead), ahead);
            drawn
        })
        .collect()
}

/// Second-difference jitter, mean receipt-to-drawn latency per received target, and the
/// largest amount a path exceeds vanilla's render-to-pick distance.
fn walk_metrics(path: &[f32], frames: &[Frame]) -> (f32, f32, f32, f32) {
    let walking: Vec<usize> = (0..frames.len())
        .filter(|&index| frames[index].target > 0.5 && frames[index].render > 0.5)
        .collect();
    let (start, end) = (walking[0] + 1, *walking.last().unwrap());
    let jerk: Vec<f32> = (start..end)
        .map(|index| path[index + 1] - 2.0 * path[index] + path[index - 1])
        .collect();
    let rms = (jerk.iter().map(|value| value * value).sum::<f32>() / jerk.len() as f32).sqrt();
    let max = jerk.iter().fold(0.0_f32, |max, value| max.max(value.abs()));
    let latencies: Vec<f32> = (start..end)
        .filter(|&index| frames[index].target != frames[index - 1].target)
        .filter_map(|index| {
            let reached =
                (index..path.len()).find(|&later| path[later] >= frames[index].target - 1.0e-4)?;
            Some((frames[reached].nanos - frames[index].nanos) as f32 / 1.0e6)
        })
        .collect();
    let latency = latencies.iter().sum::<f32>() / latencies.len() as f32;
    let excess = path
        .iter()
        .zip(frames)
        .map(|(drawn, frame)| (drawn - frame.pick).abs() - corridor(frame))
        .fold(f32::MIN, f32::max);
    (rms, max, latency, excess)
}

/// Render-only smoothing candidates never draw a remote player farther from the position
/// picks read than vanilla's own render; `--nocapture` prints jitter and latency.
#[test]
fn smoothing_candidates_stay_within_vanillas_render_to_pick_gap() {
    for arrival in [Arrival::Jitter, Arrival::Bunched] {
        for fps in [60, 144] {
            let frames = replay_frames(fps, false, false, arrival);
            let vanilla: Vec<f32> = frames.iter().map(|frame| frame.render).collect();
            let vanilla_gap = frames.iter().map(corridor).fold(0.0_f32, f32::max);
            eprintln!("{arrival:?} {fps} fps vanilla render-to-pick gap {vanilla_gap:.3}");
            for (name, path) in [
                ("vanilla", vanilla),
                ("receipt-anchored", receipt_anchored(&frames)),
                ("tube 50 ms", velocity_tube(&frames, 50.0, false)),
                ("lead-only 40 ms", velocity_tube(&frames, 40.0, true)),
                ("lead-only 50 ms", velocity_tube(&frames, 50.0, true)),
            ] {
                let (rms, max, latency, excess) = walk_metrics(&path, &frames);
                eprintln!(
                    "{arrival:?} {fps} fps {name}: jitter rms {rms:.4} max {max:.4}, latency {latency:.1} ms, gap excess {excess:.4}"
                );
                assert!(
                    excess <= 1.0e-5,
                    "{name} exceeded vanilla's pick gap by {excess}"
                );
            }
        }
    }
}
