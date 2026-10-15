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
    pub mining: DestroyInput<'a>,
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
    pub cancelled_mining_requests: Vec<i32>,
}

/// Advances the queue in order before movement drains. A later pending press never blocks
/// delivery of an earlier admitted break, and expiry only removes unadmitted entries.
pub fn admit_attack_frame(
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
                    || !movement.attach_survival_mining(tick, *payload)
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
                let request = payload
                    .mine_block
                    .as_ref()
                    .map(|request| request.request_id());
                let broken = payload.broken;
                let swing = payload.swing;
                if !movement.attach_survival_mining(tick, payload) {
                    result.cancelled_mining_requests.extend(request);
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
                let DestroyInput::Held(Some(target)) = frame.mining else {
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
                let before_input = movement.is_first_unsent_tick(press.tick);
                let between = movement
                    .between_ticks_sample()
                    .is_some_and(|sample| sample.tick == press.tick);
                if !before_input && !between {
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
    // All accepted carriers above advance regardless of new input. Only a fresh start waits.
    if !runtime.queue.pending() {
        let unsent = mining.step_ticks(
            movement,
            frame.mining,
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
        result.cancelled_mining_requests.extend(unsent);
    }
    result
}
