//! Tick-stamped authoritative edits (motion, attributes, flags) entering the
//! rewind timeline.

use super::*;

/// Placement of a tick-stamped authoritative update on the prediction timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimelineSlot {
    /// Zero, current or future tick: applies to live state.
    Live,
    /// Retained tick: edit the following frame and replay to the present.
    Rewind(u64),
    /// Older than the retained history window.
    Stale,
}

impl LocalPhysicsController {
    /// Where an authoritative update stamped with `tick` lands on the timeline.
    ///
    /// Vanilla edits the frame after `tick` and replays when that frame is
    /// retained; zero, current and future ticks apply to live state.
    fn timeline_slot(&self, tick: u64) -> TimelineSlot {
        let Some(state) = self.state.as_ref() else {
            return TimelineSlot::Live;
        };
        if tick == 0 || tick >= state.tick {
            return TimelineSlot::Live;
        }
        let retained = self.history.state_at(tick).is_some()
            && self.sample_history.iter().any(|sample| sample.tick == tick);
        if retained {
            TimelineSlot::Rewind(tick)
        } else {
            TimelineSlot::Stale
        }
    }

    /// Whether `tick` names a retained frame a correction can edit.
    pub(in crate::movement) fn retains_tick(&self, tick: u64) -> bool {
        tick != 0
            && self.history.state_at(tick).is_some()
            && self.sample_history.iter().any(|sample| sample.tick == tick)
    }

    /// Records one server velocity replacement (`SetActorMotion`) and returns
    /// the tick to rewind from when it lands inside retained history.
    ///
    /// Zero-stamped motion only replaces live velocity; it is not replay input.
    /// Stamped motion replaces velocity before the tick after its stamp.
    /// Live ticks replace velocity now; stale ticks clamp to the oldest retained
    /// frame as vanilla's frame correction does. Non-finite
    /// motion is ignored; when inactive there is no timeline to enter.
    pub fn queue_server_motion(&mut self, motion: [f32; 3], tick: u64) -> Option<u64> {
        if !motion.into_iter().all(f32::is_finite) {
            return None;
        }
        if !motion_is_simulable(motion) {
            super::super::diagnostics::note_skipped_authority(
                "motion",
                motion.into_iter().map(f32::abs).fold(0.0, f32::max).into(),
            );
            return None;
        }
        let velocity = Vec3::new(
            f64::from(motion[0]),
            f64::from(motion[1]),
            f64::from(motion[2]),
        );
        if tick == 0 {
            self.state.as_mut()?.velocity = velocity;
            return None;
        }
        let rewind = match self.timeline_slot(tick) {
            TimelineSlot::Live => None,
            TimelineSlot::Rewind(tick) => Some(tick),
            TimelineSlot::Stale => self.history.oldest_tick(),
        };
        let state = self.state.as_mut()?;
        let applies_before = match rewind {
            Some(tick) => tick.checked_add(1)?,
            None => {
                state.velocity = velocity;
                state.tick.checked_add(1)?
            }
        };
        let oldest = self.history.oldest_tick().unwrap_or(state.tick);
        self.deferred_corrections
            .replace_motion(applies_before, motion);
        self.server_motions.retain(|overlay| overlay.tick > oldest);
        if self.server_motions.len() >= self.history_capacity {
            self.server_motions.pop_front();
        }
        self.server_motions.push_back(sim::MotionOverlay {
            tick: applies_before,
            velocity,
        });
        rewind
    }

    /// Rewrites the movement speed of retained ticks after an `UpdateAttributes`
    /// stamped `tick`, replaying only actual local sprint transitions.
    ///
    /// Live and stale stamps need no rewrite: the live authority already
    /// carries the value into future ticks.
    pub(crate) fn retime_movement_speed(
        &mut self,
        tick: u64,
        current: f64,
        sprint_modifier: Option<f32>,
    ) -> Option<(
        Option<u64>,
        crate::movement::speed_authority::EffectiveMovementSpeed,
    )> {
        let TimelineSlot::Rewind(tick) = self.timeline_slot(tick) else {
            return None;
        };
        let sprinting = self.history.input_at(tick)?.sprinting;
        let mut speed = crate::movement::speed_authority::EffectiveMovementSpeed::authoritative(
            current,
            sprint_modifier,
            sprinting,
        );
        let mut changed = false;
        for input in self.history.retained_inputs_after_mut(tick) {
            speed.set_sprinting(input.sprinting);
            let predicted = speed.prediction_speed();
            if input.movement_speed != predicted {
                input.movement_speed = predicted;
                changed = true;
            }
        }
        Some((changed.then_some(tick), speed))
    }

    /// Writes a glide boost stamped `tick` into the retained inputs after it.
    ///
    /// Returns the tick to replay from when a retained input changed, and the
    /// span left for live ticks. Live stamps start with the next tick; stale
    /// stamps clamp to the oldest retained frame, as motion does.
    pub(crate) fn retime_glide_boost(
        &mut self,
        tick: u64,
        span: crate::movement::BoostSpan,
    ) -> (Option<u64>, Option<crate::movement::BoostSpan>) {
        let anchor = match self.timeline_slot(tick) {
            TimelineSlot::Live => None,
            TimelineSlot::Rewind(tick) => Some(tick),
            TimelineSlot::Stale => self.history.oldest_tick(),
        };
        let Some(anchor) = anchor else {
            return (None, Some(span));
        };
        let mut changed = false;
        let mut elapsed = 0;
        for input in self.history.retained_inputs_after_mut(anchor) {
            elapsed += 1;
            let boosted = span.covers(elapsed);
            changed |= input.effects.glide_boost != boosted;
            input.effects.glide_boost = boosted;
        }
        (changed.then_some(anchor), span.after(elapsed))
    }

    /// Replaces the live velocity, for timeline edits whose replay failed.
    pub fn replace_live_velocity(&mut self, motion: [f32; 3]) {
        if let Some(state) = self.state.as_mut()
            && motion.into_iter().all(f32::is_finite)
        {
            state.velocity = Vec3::new(
                f64::from(motion[0]),
                f64::from(motion[1]),
                f64::from(motion[2]),
            );
        }
    }
}

/// Sprint and sneak states the live control latches adopt from the server.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ServerControlFlags {
    pub sprinting: Option<bool>,
    pub sneaking: Option<bool>,
}

impl LocalPhysicsController {
    /// Applies server movement flags stamped `tick` (`SetActorData`); returns
    /// the tick to replay from when retained inputs changed.
    ///
    /// A retained tick rewrites only the client's unchanged run after it, so a
    /// later client transition still wins; stale ticks clamp to the oldest
    /// frame as vanilla's frame correction does. Modes are only ended, never
    /// started: entry needs environment predicates the flag does not carry.
    pub fn apply_server_movement_flags(
        &mut self,
        tick: u64,
        flags: client_world::MovementFlagUpdate,
    ) -> Option<u64> {
        self.state.as_ref()?;
        let rewind = match self.timeline_slot(tick) {
            TimelineSlot::Live => None,
            TimelineSlot::Rewind(tick) => Some(tick),
            TimelineSlot::Stale => self.history.oldest_tick(),
        };
        let Some(tick) = rewind else {
            self.adopt_live_flags(flags);
            return None;
        };
        let anchor = *self.history.input_at(tick)?;
        let mut present = client_world::MovementFlagUpdate::default();
        let mut changed = false;
        if let Some(immobile) = flags.immobile.filter(|value| *value != anchor.immobile) {
            let (edited, _) = rewrite_run(
                self.history.retained_inputs_after_mut(tick),
                |input| input.immobile == anchor.immobile,
                |input| input.immobile = immobile,
            );
            changed |= edited;
        }
        if let Some(sprinting) = flags.sprinting.filter(|value| *value != anchor.sprinting) {
            let (edited, reached) = rewrite_run(
                self.history.retained_inputs_after_mut(tick),
                |input| input.sprinting == anchor.sprinting && (!sprinting || input.forward > 0.0),
                |input| {
                    let previous = input.sprinting;
                    input.sprinting = sprinting;
                    crate::movement::speed_authority::preserve_effective_speed(input, previous);
                },
            );
            changed |= edited;
            present.sprinting = reached.then_some(sprinting);
        }
        if let Some(sneaking) = flags.sneaking.filter(|value| *value != anchor.sneaking) {
            let (edited, reached) = rewrite_run(
                self.history.retained_inputs_after_mut(tick),
                |input| input.sneaking == anchor.sneaking,
                |input| input.sneaking = sneaking,
            );
            changed |= edited;
            present.sneaking = reached.then_some(sneaking);
        }
        for (flag, mode) in [
            (flags.gliding, sim::MovementMode::Gliding),
            (flags.swimming, sim::MovementMode::Swimming),
            (flags.crawling, sim::MovementMode::Crawling),
        ] {
            if flag != Some(false) || anchor.mode != mode {
                continue;
            }
            let (edited, reached) = rewrite_run(
                self.history.retained_inputs_after_mut(tick),
                |input| input.mode == mode,
                |input| input.mode = sim::MovementMode::Walking,
            );
            changed |= edited;
            if reached {
                self.modes.end(mode);
            }
        }
        self.adopt_live_flags(client_world::MovementFlagUpdate {
            gliding: None,
            swimming: None,
            crawling: None,
            ..present
        });
        changed.then_some(tick)
    }

    /// Takes the sprint/sneak states the control latches must adopt.
    pub fn take_server_control_flags(&mut self) -> Option<ServerControlFlags> {
        self.server_control_flags.take()
    }

    fn adopt_live_flags(&mut self, flags: client_world::MovementFlagUpdate) {
        for (flag, mode) in [
            (flags.gliding, sim::MovementMode::Gliding),
            (flags.swimming, sim::MovementMode::Swimming),
            (flags.crawling, sim::MovementMode::Crawling),
        ] {
            if flag == Some(false) {
                self.modes.end(mode);
            }
        }
        if flags.sprinting.is_none() && flags.sneaking.is_none() {
            return;
        }
        self.modes.restore_controls(
            flags.sprinting.unwrap_or(self.modes.sprinting()),
            flags.sneaking.unwrap_or(self.modes.sneaking()),
        );
        let pending = self.server_control_flags.get_or_insert_default();
        pending.sprinting = flags.sprinting.or(pending.sprinting);
        pending.sneaking = flags.sneaking.or(pending.sneaking);
    }
}

/// Edits the leading run of inputs still matching `unchanged`; returns whether
/// anything changed and whether the run reached the newest input.
fn rewrite_run<'a>(
    inputs: impl Iterator<Item = &'a mut MovementInput>,
    unchanged: impl Fn(&MovementInput) -> bool,
    edit: impl Fn(&mut MovementInput),
) -> (bool, bool) {
    let mut edited = false;
    for input in inputs {
        if !unchanged(input) {
            return (edited, false);
        }
        edit(input);
        edited = true;
    }
    (edited, true)
}

/// Whether a velocity keeps the next tick's collision sweep inside the query extent.
pub(super) fn motion_is_simulable(motion: [f32; 3]) -> bool {
    motion.into_iter().all(|axis| {
        axis.is_finite()
            && f64::from(axis.abs()) + sim::PLAYER_HEIGHT < sim::MAX_COLLISION_QUERY_EXTENT
    })
}
