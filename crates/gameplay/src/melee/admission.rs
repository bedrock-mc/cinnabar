//! Ordered admission of presses, their mining starts and their movement carriers.

use super::{Crosshair, MeleeRuntime, PressContext, SwingTracker, queue::Work, resolve_and_send};
use crate::{
    BatchSendError,
    movement::MovementTicker,
    survival_mining::{BlockBreakingAuthority, DestroyInput, SurvivalMiningRuntime},
};

/// Current facts used when the next queued press can enter the outbound FIFO.
pub struct AttackFrame<'a> {
    pub attack: Option<(Crosshair, PressContext)>,
    /// A queued press may retain a target even after the button is released.
    pub mining_target: Option<&'a crate::survival_mining::DestroyTarget>,
    /// Current button state, independent of queued presses and target eligibility.
    pub attack_held: bool,
    /// Existing progress and cleanup continue while fresh held mining is fenced.
    pub allow_mining_start: bool,
    pub authority: BlockBreakingAuthority,
    pub now_millis: u64,
    pub local_runtime_id: u64,
}

/// Edge effects used by both pressed and held mining at the same admission boundary.
pub struct AttackEffects<Send, Duration, Request, Predict> {
    pub send: Send,
    pub swing_duration: Duration,
    pub request_id: Request,
    pub predict_break: Predict,
}

/// Accepted press identity, action-facing tick and inventory requests no carrier accepted.
#[derive(Debug, Default)]
pub struct AttackAdmission {
    pub admitted: Option<u64>,
    pub rotation_tick: Option<u64>,
    pub mining: crate::survival_mining::MiningAdmission,
}

/// Advances the queue in order before movement drains. A later pending press never blocks
/// delivery of an earlier admitted break, and expiry only removes unadmitted entries.
/// Admits a press and advances held mining exactly once for the frame.
pub fn admit_attack_frame(
    runtime: &mut MeleeRuntime,
    mining: &mut SurvivalMiningRuntime,
    swings: &mut SwingTracker,
    movement: &mut MovementTicker,
    frame: AttackFrame<'_>,
    effects: AttackEffects<
        impl FnMut(Vec<protocol::Packet>) -> Result<(), BatchSendError>,
        impl Fn(u64) -> i32,
        impl FnMut(u8, i32) -> Option<i32>,
        impl FnMut([i32; 3]),
    >,
) -> AttackAdmission {
    admit(runtime, mining, swings, movement, frame, effects, true)
}

/// Attempts one ordered press once, preserving only accepted mining work.
pub fn admit_attack_press(
    runtime: &mut MeleeRuntime,
    mining: &mut SurvivalMiningRuntime,
    swings: &mut SwingTracker,
    movement: &mut MovementTicker,
    frame: AttackFrame<'_>,
    effects: AttackEffects<
        impl FnMut(Vec<protocol::Packet>) -> Result<(), BatchSendError>,
        impl Fn(u64) -> i32,
        impl FnMut(u8, i32) -> Option<i32>,
        impl FnMut([i32; 3]),
    >,
) -> AttackAdmission {
    let result = admit(runtime, mining, swings, movement, frame, effects, false);
    if runtime.press_pending() {
        runtime.cancel_press();
    }
    result
}

/// Shares carrier admission between per-event work and frame maintenance.
fn admit(
    runtime: &mut MeleeRuntime,
    mining: &mut SurvivalMiningRuntime,
    swings: &mut SwingTracker,
    movement: &mut MovementTicker,
    frame: AttackFrame<'_>,
    mut effects: AttackEffects<
        impl FnMut(Vec<protocol::Packet>) -> Result<(), BatchSendError>,
        impl Fn(u64) -> i32,
        impl FnMut(u8, i32) -> Option<i32>,
        impl FnMut([i32; 3]),
    >,
    maintain: bool,
) -> AttackAdmission {
    runtime.synchronize(movement.interaction_authority_identity());
    mining.synchronize(movement.interaction_authority_identity());
    runtime.defer(frame.now_millis);
    let mut result = AttackAdmission::default();
    let mut attempted = false;
    while let Some(entry) = runtime.queue.entries.front().cloned() {
        match entry.work {
            Work::Carrying { tick } => {
                if movement.unsent_sample_at(tick).is_some() {
                    break;
                }
                runtime.queue.entries.pop_front();
            }
            Work::Predicted { tick, payload } => {
                if movement.unsent_sample_at(tick).is_none()
                    || !payload.attach(movement, tick, &mut result.mining)
                {
                    break;
                }
                mining.carried_prediction(movement.completed_tick());
                runtime.queue.entries.front_mut().unwrap().work = Work::Carrying { tick };
            }
            Work::Start {
                tick,
                target,
                authority,
            } => {
                let Some((_, motion)) = movement
                    .unstepped_interaction_ticks(None)
                    .into_iter()
                    .find(|(candidate, _)| *candidate == tick)
                else {
                    break;
                };
                let (machine, mut payload) = mining.preview_press(&target, motion, authority);
                payload.assign_request(&mut effects.request_id);
                let broken = payload.broken;
                let swing = payload.swing;
                if !payload.attach(movement, tick, &mut result.mining) {
                    break;
                }
                mining.commit_press(machine, tick, movement.completed_tick(), &target, broken);
                if swing && swings.try_swing(tick, (effects.swing_duration)(tick)) {
                    let _ = (effects.send)(vec![protocol::swing_arm_packet(
                        frame.local_runtime_id,
                        protocol::SwingSource::Mine,
                    )]);
                }
                broken.into_iter().for_each(&mut effects.predict_break);
                runtime.queue.entries.front_mut().unwrap().work = Work::Carrying { tick };
            }
            Work::Block { tick } => {
                let Some(target) = frame.mining_target else {
                    runtime.queue.entries.pop_front();
                    continue;
                };
                if let Some((predicted_tick, payload)) =
                    mining.predict_press(movement, target, frame.authority)
                {
                    debug_assert_eq!(predicted_tick, tick);
                    if payload.swing
                        && swings.try_attack_swing(tick, (effects.swing_duration)(tick))
                    {
                        let _ = (effects.send)(vec![protocol::swing_arm_packet(
                            frame.local_runtime_id,
                            protocol::SwingSource::Mine,
                        )]);
                    }
                    payload
                        .broken
                        .into_iter()
                        .for_each(&mut effects.predict_break);
                    runtime.queue.entries.front_mut().unwrap().work = Work::Predicted {
                        tick,
                        payload: Box::new(payload),
                    };
                } else {
                    runtime.queue.entries.front_mut().unwrap().work = Work::Start {
                        tick,
                        target: Box::new(target.clone()),
                        authority: frame.authority,
                    };
                }
            }
            Work::Pending(_) => {
                if attempted
                    || movement.has_held_release()
                    || !movement.accepts_block_interactions()
                {
                    break;
                }
                let Some((crosshair, press)) = frame.attack.as_ref() else {
                    break;
                };
                if !movement.interaction_tick_available(press.tick)
                    || movement.carries_survival_mining(press.tick)
                {
                    break;
                }
                attempted = true;
                let mut accepted = false;
                let mut rotates = false;
                let missed = resolve_and_send(
                    runtime,
                    swings,
                    *crosshair,
                    press,
                    frame.now_millis,
                    |packets| {
                        rotates = packets.iter().any(protocol::is_aim_assist_rotation_action);
                        let sent = (effects.send)(packets);
                        accepted = sent.is_ok();
                        sent
                    },
                );
                if !accepted {
                    break;
                }
                result.admitted = Some(entry.id);
                if rotates {
                    result.rotation_tick = Some(press.tick);
                }
                if missed {
                    movement.mark_missed_swing(press.tick);
                }
            }
        }
    }
    if !maintain {
        return result;
    }
    // Only a fresh mining start waits; existing progress, aborts and held delay still advance.
    let input = if !frame.attack_held {
        DestroyInput::Released
    } else if frame.allow_mining_start && !runtime.queue.pending() {
        DestroyInput::Held(frame.mining_target)
    } else {
        DestroyInput::Continue(frame.mining_target)
    };
    let admitted = mining.step_ticks(
        movement,
        input,
        frame.authority,
        |tick| {
            if swings.try_swing(tick, (effects.swing_duration)(tick)) {
                let _ = (effects.send)(vec![protocol::swing_arm_packet(
                    frame.local_runtime_id,
                    protocol::SwingSource::Mine,
                )]);
            }
        },
        &mut effects.request_id,
        &mut effects.predict_break,
    );
    result.mining.extend(admitted);
    result
}
