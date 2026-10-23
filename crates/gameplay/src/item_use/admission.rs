use super::*;

/// Queues an item's swing and transactions together before committing local prediction.
/// Returns whether this tick started an admitted held use.
pub fn step_and_send(
    runtime: &mut ItemUseRuntime,
    swings: &mut SwingTracker,
    frame: &UseFrame,
    local_runtime_id: u64,
    swing_duration: i32,
    send: impl FnOnce(Vec<protocol::Packet>) -> Result<(), BatchSendError>,
) -> bool {
    runtime.observe_riptide_release(
        frame.tick,
        frame.held,
        frame.riptide,
        frame.selection.as_ref(),
        frame.riptide_facing,
    );
    if let Some(expected) = &runtime.pending.deferred_selection {
        match &frame.selection {
            Some(current) if current != expected => runtime.cancel_pending_input(),
            None if !runtime.release_pending => return false,
            _ => {}
        }
    }
    let mut candidate = runtime.clone();
    let mut candidate_swings = swings.clone();
    let defer_click_until_verified = runtime.pending.deferred_selection.is_some()
        && frame.selection.is_none()
        && runtime.release_pending;
    candidate.pending.deferred_selection = None;
    if defer_click_until_verified {
        candidate.pending.latched_press = false;
    }
    let outcome = candidate.step(frame);
    if defer_click_until_verified {
        candidate.pending.latched_press = true;
        candidate.pending.deferred_selection = runtime.pending.deferred_selection.clone();
    }
    if outcome.swung
        && swings.tick_is_published(frame.tick)
        && runtime.pending.rejected_swing_tick != Some((frame.tick, swings.authority_identity()))
    {
        return false;
    }
    candidate.pending.rejected_swing_tick = None;
    let mut packets = outcome.packets;
    if outcome.swung && candidate_swings.try_swing(frame.tick, swing_duration) {
        packets.insert(
            0,
            protocol::swing_arm_packet(
                local_runtime_id,
                outcome
                    .swing_source
                    .unwrap_or(protocol::SwingSource::ThrowItem),
            ),
        );
    }
    let admission = if packets.is_empty() {
        Ok(())
    } else {
        send(packets)
    };
    match admission {
        Ok(()) => {
            *runtime = candidate;
            *swings = candidate_swings;
            outcome.started
        }
        Err(BatchSendError::Full) => {
            swings.defer_unadmitted_attempt(&candidate_swings);
            if outcome.swung {
                runtime.pending.rejected_swing_tick =
                    Some((frame.tick, swings.authority_identity()));
            }
            if outcome.used {
                runtime.pending.latched_press = true;
                runtime.pending.deferred_selection = frame.selection.clone();
            }
            runtime.release_pending |= outcome.released;
            false
        }
        Err(BatchSendError::Closed) => {
            runtime.cancel();
            false
        }
    }
}
