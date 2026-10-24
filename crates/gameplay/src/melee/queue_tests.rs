//! Generated event sequences compare admission and encoded delivery with an independent model.

use std::collections::{BTreeMap, VecDeque};

use super::*;
use crate::{
    movement::{
        LocalMovementEffectTimeline, MAX_LOCAL_PHYSICS_TICKS_PER_FRAME, MovementEffectSource,
        MovementTicker, flush_player_auth_inputs,
    },
    survival_mining::{
        BlockBreakingAuthority, DestroyInput, DestroyMachine, DestroyTarget, SurvivalMiningRuntime,
    },
};

/// One input event in the independent pending-press model.
#[derive(Clone, Copy, Debug)]
struct ExpectedPress {
    id: u64,
    since: u64,
    block: bool,
}

/// A separate tick-driven machine records maintenance expected even while presses wait.
#[derive(Default)]
struct Maintenance {
    machine: DestroyMachine,
    last_tick: Option<u64>,
    actions: BTreeMap<u64, Vec<(i32, [i32; 3], i32)>>,
    target: Option<u64>,
    instant: bool,
    pending_ticks: usize,
    held_gaps: usize,
    block_presses: usize,
}

/// Captures real packet output while keeping only input deadlines and delivered prefixes in the model.
struct Harness {
    runtime: MeleeRuntime,
    mining: SurvivalMiningRuntime,
    swings: SwingTracker,
    movement: MovementTicker,
    effects: LocalMovementEffectTimeline,
    pending: VecDeque<ExpectedPress>,
    next_id: u64,
    now: u64,
    predictions: Vec<[i32; 3]>,
    delivered: Vec<[i32; 3]>,
    admitted: Vec<u64>,
    expired_behind_prediction: usize,
    maintenance: Option<Maintenance>,
}

impl Harness {
    /// Begins with an admitted movement anchor and no pending input or predicted block.
    fn new() -> Self {
        let mut movement = crate::test_support::survival_mining::ticker_with_ticks(1);
        let mut written = Vec::new();
        flush_player_auth_inputs(
            &mut movement,
            8,
            Some(crate::test_support::survival_mining::evidence()),
            |identity, _| {
                written.push(identity);
                Ok::<_, ()>(())
            },
        )
        .unwrap();
        for identity in written {
            assert!(movement.acknowledge_physics_send(identity));
        }
        let _ = movement.take_tick_evidence();
        Self {
            runtime: Default::default(),
            mining: Default::default(),
            swings: Default::default(),
            movement,
            effects: Default::default(),
            pending: VecDeque::new(),
            next_id: 0,
            now: 1,
            predictions: Vec::new(),
            delivered: Vec::new(),
            admitted: Vec::new(),
            expired_behind_prediction: 0,
            maintenance: None,
        }
    }

    /// Adds a distinct physical press to both the real owner and the independent model.
    fn press(&mut self, block: bool) {
        self.next_id += 1;
        self.pending.push_back(ExpectedPress {
            id: self.next_id,
            since: self.now,
            block,
        });
        self.runtime.observe_input(true, false);
    }

    /// Creates real completed movement samples and the matching current-frame tick count.
    fn ticks(&mut self, count: usize) {
        if !self.movement.can_advance_physics_frame() {
            return;
        }
        for _ in 0..count {
            let next = self.movement.completed_tick() + 1;
            self.movement
                .enqueue_completed_physics(crate::test_support::survival_mining::completed(next))
                .unwrap();
            self.effects.commit_successful_tick();
        }
    }

    /// Applies cancellation only to pending model entries; accepted predictions are never removed.
    fn cancel(&mut self) {
        self.pending.clear();
        self.runtime.cancel();
    }

    /// Runs the actual frame admission and compares accepted identities and surviving input order.
    fn frame(&mut self, full: bool, held: bool) {
        let before_expiry = self.pending.len();
        self.pending
            .retain(|press| self.now.saturating_sub(press.since) <= MAX_PENDING_INTERACTION_MILLIS);
        if self.predictions.len() > self.delivered.len() {
            self.expired_behind_prediction += before_expiry - self.pending.len();
        }
        let first = self.pending.front().copied();
        let mut target = target(first.map_or(self.next_id + 1_000, |press| press.id));
        if let Some(model) = &self.maintenance {
            target = self::target(model.target.unwrap_or(1_000));
            target.instant = model.instant;
        }
        let crosshair = first.map(|press| {
            if press.block {
                Crosshair::Block
            } else {
                Crosshair::Actor(ActorHit {
                    runtime_id: press.id,
                    distance: 2.0,
                    point: [0.0, 1.5, -2.0],
                })
            }
        });
        self.runtime
            .synchronize(self.movement.interaction_authority_identity());
        self.runtime.defer(self.now);
        self.swings.sync_ticks(
            self.movement.interaction_authority_identity(),
            self.movement.completed_tick(),
            &self.effects,
        );
        let attack = crosshair.and_then(|crosshair| {
            let sample = self
                .runtime
                .between_ticks_press(&self.movement)
                .or_else(|| {
                    self.runtime.press_sample(
                        crosshair,
                        &self.movement,
                        self.effects.recent_tick_count(),
                        self.now,
                    )
                })?;
            Some((
                crosshair,
                PressContext {
                    refused_destroy: None,
                    tick: sample.tick,
                    player_position: sample.position,
                    input_mode: protocol::PlayerInputMode::Mouse,
                    local_runtime_id: 8,
                    selection: Some(target.selection.clone()),
                    swing_duration: actor_animation::ACTOR_SWING_TICKS,
                    now_millis: self.now,
                    item_attack: None,
                },
            ))
        });
        let input = if let Some(model) = &self.maintenance {
            if held {
                DestroyInput::Held(model.target.map(|_| &target))
            } else {
                DestroyInput::Released
            }
        } else if first.is_some_and(|press| press.block) || (first.is_none() && held) {
            DestroyInput::Held(Some(&target))
        } else {
            DestroyInput::Released
        };
        // This corpus admits block starts on completed inputs; the first corpus covers
        // instant previews between ticks and their later carriers.
        let full = full
            || (self.maintenance.is_some()
                && first.is_some_and(|press| press.block)
                && matches!(input, DestroyInput::Held(Some(_)))
                && self.movement.between_ticks_sample().is_some());
        let attack_tick = attack.as_ref().map(|(_, press)| press.tick);
        let mut packets = Vec::new();
        let mut predicted = Vec::new();
        let outcome = admit_attack_frame(
            &mut self.runtime,
            &mut self.mining,
            &mut self.swings,
            &mut self.movement,
            AttackFrame {
                attack,
                mining_target: match input {
                    DestroyInput::Held(target) | DestroyInput::Continue(target) => target,
                    DestroyInput::Released => None,
                },
                allow_mining_start: true,
                attack_held: held,
                authority: BlockBreakingAuthority::Server,
                now_millis: self.now,
                local_runtime_id: 8,
            },
            AttackEffects {
                send: |batch| {
                    if full {
                        Err(BatchSendError::Full)
                    } else {
                        packets.extend(batch);
                        Ok(())
                    }
                },
                swing_duration: |_| actor_animation::ACTOR_SWING_TICKS,
                request_id: |_, _| None,
                predict_break: |position| predicted.push(position),
            },
        );
        if let Some(id) = outcome.admitted {
            let expected = self
                .pending
                .pop_front()
                .expect("no invented press admission");
            assert_eq!(
                id, expected.id,
                "presses must enter the FIFO in input order"
            );
            if self.maintenance.is_none() {
                assert_eq!(
                    self.predictions, self.delivered,
                    "a later press overtook a predicted break's carrier"
                );
            }
            if !expected.block {
                let actor = packets
                    .iter()
                    .filter_map(protocol::test_support::transactions::observe_transaction)
                    .find_map(|transaction| transaction.actor_use().map(|actor| actor.runtime_id))
                    .unwrap();
                assert_eq!(actor, expected.id);
            }
            self.admitted.push(id);
        }
        let admitted_block = outcome.admitted.is_some() && first.is_some_and(|press| press.block);
        if let Some(model) = &mut self.maintenance {
            model.block_presses += usize::from(admitted_block);
            // A newly admitted start can replace earlier idle work on retained inputs.
            let after = if admitted_block && matches!(input, DestroyInput::Held(Some(_))) {
                attack_tick.and_then(|tick| tick.checked_sub(1))
            } else {
                model.last_tick
            };
            let ticks = self.movement.unstepped_interaction_ticks(after);
            if let Some(&(newest, _)) = ticks.last() {
                model.last_tick = Some(newest);
            }
            let mut expected_predictions = Vec::new();
            for (tick, motion) in ticks {
                let maintaining = model.machine.destroying_target();
                model.pending_ticks +=
                    usize::from(!self.pending.is_empty() && maintaining.is_some());
                model.held_gaps += usize::from(input == DestroyInput::Held(None));
                let step_input = match input {
                    DestroyInput::Held(Some(target))
                        if !self.pending.is_empty()
                            && !(admitted_block && attack_tick == Some(tick))
                            && maintaining
                                .is_none_or(|(position, _)| position != target.position) =>
                    {
                        DestroyInput::Held(None)
                    }
                    _ => input,
                };
                let payload =
                    model
                        .machine
                        .step(step_input, motion, BlockBreakingAuthority::Server);
                model.actions.insert(
                    tick,
                    payload
                        .actions
                        .iter()
                        .map(|action| {
                            (
                                action.kind.wire_value(),
                                action.position,
                                i32::from(action.face),
                            )
                        })
                        .collect(),
                );
                if let Some(position) = payload.broken {
                    expected_predictions.push(position);
                    break;
                }
            }
            assert_eq!(
                predicted, expected_predictions,
                "held input and destroy maintenance must advance on every eligible tick"
            );
            assert_eq!(
                self.mining.destroying_target(),
                model.machine.destroying_target(),
                "pending presses suppressed destroy maintenance"
            );
        }
        self.predictions.extend(predicted);
        let actual: Vec<_> = self
            .runtime
            .queue
            .entries
            .iter()
            .filter_map(|entry| {
                matches!(entry.work, super::queue::Work::Pending(_)).then_some(entry.id)
            })
            .collect();
        assert_eq!(
            actual,
            self.pending
                .iter()
                .map(|press| press.id)
                .collect::<Vec<_>>(),
            "expiry or admission removed another entry"
        );
        assert!(self.admitted.windows(2).all(|ids| ids[0] < ids[1]));
        self.swings
            .published_progress(self.movement.completed_tick());
    }

    /// Hands inputs to the real encoder, sometimes refusing the transport to retain its head.
    fn flush(&mut self, limit: usize, full: bool) {
        let mut packets = Vec::new();
        let mut written = Vec::new();
        let _ = flush_player_auth_inputs(
            &mut self.movement,
            limit,
            Some(crate::test_support::survival_mining::evidence()),
            |identity, packet| {
                if full {
                    Err(())
                } else {
                    packets.push(packet);
                    written.push(identity);
                    Ok(())
                }
            },
        );
        for identity in written {
            assert!(self.movement.acknowledge_physics_send(identity));
        }
        let _ = self.movement.take_tick_evidence();
        for packet in packets {
            if let Some(model) = &mut self.maintenance {
                let tick = protocol::player_auth_input_trace_sample(&packet)
                    .unwrap()
                    .tick;
                assert_eq!(
                    protocol::test_support::packets::input_block_actions(&packet),
                    model.actions.remove(&tick).unwrap_or_default(),
                    "destroy progress or abort actions stalled at tick {tick}"
                );
            }
            self.delivered
                .extend(protocol::test_support::packets::predicted_block_breaks(
                    &packet,
                ));
        }
        assert!(
            self.delivered.len() <= self.predictions.len(),
            "a carrier invented a predicted break"
        );
        assert_eq!(
            self.delivered,
            self.predictions[..self.delivered.len()],
            "predicted breaks were dropped, duplicated or reordered"
        );
    }

    /// Drains accepted work without advancing deadlines, proving eventual delivery after backpressure.
    fn finish(&mut self) {
        for _ in 0..1_024 {
            if self.runtime.queue.entries.is_empty()
                && self.movement.newest_unsent_sample().is_none()
            {
                break;
            }
            self.effects.begin_frame();
            self.ticks(1);
            self.frame(false, false);
            self.flush(usize::MAX, false);
        }
        assert!(self.pending.is_empty());
        assert!(
            self.runtime.queue.entries.is_empty(),
            "accepted work did not drain: queue={:?}, completed={}, first={:?}",
            self.runtime.queue.entries,
            self.movement.completed_tick(),
            self.movement.newest_unsent_sample()
        );
        assert_eq!(
            self.delivered, self.predictions,
            "an admitted prediction was lost"
        );
    }
}

/// A unique instant target gives each encoded prediction an independently observable identity.
fn target(id: u64) -> DestroyTarget {
    let stack = protocol::NetworkItemStack::empty();
    DestroyTarget {
        local_runtime_id: 8,
        position: [id as i32, 2, 3],
        face: 1,
        runtime_id: 9,
        wire_runtime_id: 9,
        relative_hit: [0.5; 3],
        block: sim::BlockDestroyInfo::from_facts(
            assets::compiled::block_destroy::builtin_destroy_facts("minecraft:stone"),
        ),
        conditions: sim::DestroyConditions::default(),
        selection: crate::mining::FrozenMiningSelection {
            slot: 0,
            item: protocol::VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest)
                .unwrap(),
        },
        wear: None,
        instant: true,
    }
}

/// Deterministic xorshift input makes every generated failure replayable from its seed.
fn next(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

#[test]
fn generated_attack_mining_and_expiry_sequences_preserve_admitted_fifo_prefixes() {
    let mut expired_behind_prediction = 0;
    for initial in 1..=64 {
        let mut rng = initial;
        let mut harness = Harness::new();
        // Keep an admitted prediction, expire only the older of two later presses.
        harness.press(true);
        harness.frame(false, false);
        harness.now += 1;
        harness.press(false);
        harness.frame(false, false);
        harness.now += MAX_PENDING_INTERACTION_MILLIS / 2;
        harness.press(false);
        harness.frame(false, false);
        harness.now = 3 + MAX_PENDING_INTERACTION_MILLIS;
        harness.frame(false, false);
        assert_eq!(
            harness.pending.len(),
            1,
            "seed {initial}: expiry removed a newer press"
        );
        for _ in 0..256 {
            harness.effects.begin_frame();
            harness.now += 1;
            let value = next(&mut rng);
            match value % 8 {
                0 | 1 => harness.press(value & 8 != 0),
                2 => harness.ticks(1 + (value as usize % MAX_LOCAL_PHYSICS_TICKS_PER_FRAME)),
                3 => harness.flush(
                    1 + (value as usize % MAX_LOCAL_PHYSICS_TICKS_PER_FRAME),
                    value & 16 != 0,
                ),
                4 => harness.now += 1 + value % (MAX_PENDING_INTERACTION_MILLIS + 1),
                5 => harness.cancel(),
                _ => {}
            }
            harness.frame(value & 32 != 0, value & 64 != 0);
        }
        harness.finish();
        expired_behind_prediction += harness.expired_behind_prediction;
    }
    let mut pending_ticks = 0;
    let mut held_gaps = 0;
    let mut block_presses = 0;
    for initial in 1..=64 {
        let mut rng = initial;
        let mut harness = Harness::new();
        harness.maintenance = Some(Maintenance {
            target: Some(1_000),
            instant: initial % 2 == 0,
            ..Default::default()
        });
        harness.effects.begin_frame();
        harness.ticks(1);
        harness.frame(false, true);
        harness.flush(usize::MAX, false);
        harness.press(false);
        harness.effects.begin_frame();
        harness.ticks(2);
        harness.frame(true, true);
        for _ in 0..256 {
            harness.effects.begin_frame();
            harness.now += 1;
            let value = next(&mut rng);
            match value % 8 {
                0 => harness.press(value & 8 != 0),
                1 => {
                    harness.maintenance.as_mut().unwrap().target = match value % 3 {
                        0 => None,
                        slot => Some(1_000 + slot),
                    }
                }
                2 => harness.ticks(1 + value as usize % MAX_LOCAL_PHYSICS_TICKS_PER_FRAME),
                3 => harness.flush(
                    1 + value as usize % MAX_LOCAL_PHYSICS_TICKS_PER_FRAME,
                    value & 16 != 0,
                ),
                4 => harness.now += 1 + value % (MAX_PENDING_INTERACTION_MILLIS + 1),
                5 => harness.cancel(),
                _ => {}
            }
            harness.frame(value & 32 != 0, value & 64 != 0);
        }
        harness.finish();
        let model = harness.maintenance.unwrap();
        assert!(model.actions.is_empty());
        pending_ticks += model.pending_ticks;
        held_gaps += model.held_gaps;
        block_presses += model.block_presses;
    }
    assert!(
        block_presses > 0,
        "the maintenance corpus must admit block presses"
    );
    assert!(
        pending_ticks > 0,
        "the generated corpus must maintain ongoing destroys behind waiting presses"
    );
    assert!(
        held_gaps > 0,
        "the generated corpus must preserve held input with no target"
    );
    assert!(
        expired_behind_prediction > 0,
        "the generated corpus must expire newer presses behind predictions"
    );
}
