//! Tick-stamped authoritative edits (motion, attributes, flags) entering the
//! rewind timeline.

use super::server_flags::FlagWrite;
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
            self.knockback_sequence = self.knockback_sequence.saturating_add(1);
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
        self.knockback_sequence = self.knockback_sequence.saturating_add(1);
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
        attribute: client_world::MovementSpeedAttribute,
    ) -> Option<(
        Option<u64>,
        crate::movement::speed_authority::EffectiveMovementSpeed,
    )> {
        let TimelineSlot::Rewind(tick) = self.timeline_slot(tick) else {
            return None;
        };
        let sprinting = self.history.input_at(tick)?.sprinting;
        let mut speed = crate::movement::speed_authority::EffectiveMovementSpeed::authoritative(
            attribute, sprinting,
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

    /// Rewrites the air-drag modifier of retained ticks after an `UpdateAttributes`
    /// stamped `tick`; returns the tick to replay from when an input changed.
    pub(crate) fn retime_air_drag_modifier(&mut self, tick: u64, current: f32) -> Option<u64> {
        let tick = match self.timeline_slot(tick) {
            TimelineSlot::Live => return None,
            TimelineSlot::Rewind(tick) => tick,
            TimelineSlot::Stale => self.history.oldest_tick()?,
        };
        rewrite_server_owned(
            self.history.retained_inputs_after_mut(tick),
            |input| &mut input.vertical_physics.air_drag_modifier,
            Some(f64::from(current)),
        )
        .then_some(tick)
    }

    /// Records a server boost stamped `tick`, which starts on the tick after
    /// its stamp; an unstamped or live boost starts on the next simulated tick.
    /// Returns the tick to replay from when a retained tick simulated a
    /// different boost.
    pub fn record_server_boost(
        &mut self,
        boost: crate::movement::MovementBoost,
        tick: u64,
        span: crate::movement::BoostSpan,
    ) -> Option<u64> {
        let next = self.next_boost_tick();
        let start = if tick == 0 {
            next
        } else {
            tick.saturating_add(1).min(next)
        };
        self.boosts.record_server(boost, start, span);
        self.retire_boosts();
        self.first_boost_change()
    }

    /// Predicts the glide boost of a rocket fired for the tick `start` onward;
    /// it cannot boost a tick already simulated.
    pub fn predict_rocket_boost(&mut self, start: u64) {
        self.boosts
            .predict_rocket(start.max(self.next_boost_tick()));
        self.retire_boosts();
    }

    /// Whether `boost` applies to the input simulated on `tick`, live or replayed.
    #[must_use]
    pub fn boosted(&self, boost: crate::movement::MovementBoost, tick: u64) -> bool {
        self.boosts.boosted(boost, tick)
    }

    /// Each retained tick after the oldest with its recorded glide lane and resulting state.
    #[cfg(test)]
    pub(in crate::movement) fn retained_glide(&self) -> Vec<(u64, bool, sim::PlayerState)> {
        let (Some(oldest), Some(newest)) = (self.history.oldest_tick(), self.history.newest_tick())
        else {
            return Vec::new();
        };
        (oldest + 1..=newest)
            .filter_map(|tick| {
                let input = self.history.input_at(tick)?;
                Some((
                    tick,
                    input.effects.glide_boost,
                    self.history.state_at(tick)?.clone(),
                ))
            })
            .collect()
    }

    /// The oldest retained tick, which a full replay rewinds to.
    #[cfg(test)]
    pub(in crate::movement) fn oldest_retained_tick(&self) -> Option<u64> {
        self.history.oldest_tick()
    }

    fn next_boost_tick(&self) -> u64 {
        self.state.as_ref().map_or(0, |state| state.tick) + 1
    }

    /// Forgets boost intervals no replay can reach: replays simulate the ticks after the oldest retained one.
    fn retire_boosts(&mut self) {
        if let Some(oldest) = self.history.oldest_tick() {
            self.boosts.forget_before(oldest + 1);
        }
    }

    /// The tick before the first retained tick whose recorded boosts differ from the boost timeline.
    fn first_boost_change(&self) -> Option<u64> {
        let oldest = self.history.oldest_tick()?;
        let newest = self.history.newest_tick()?;
        (oldest + 1..=newest).find_map(|tick| {
            let recorded = self.history.input_at(tick)?.effects;
            let mut expected = recorded;
            self.boosts.apply(tick, &mut expected);
            (expected != recorded).then_some(tick - 1)
        })
    }

    /// Rewrites the liquid speed attributes of retained ticks after an
    /// `UpdateAttributes` stamped `tick`; returns the tick to replay from when
    /// an input changed. Live and stale stamps need no rewrite.
    pub(crate) fn retime_liquid_movement_speeds(
        &mut self,
        tick: u64,
        speeds: crate::movement::speed_authority::LiquidMovementSpeeds,
    ) -> Option<u64> {
        let TimelineSlot::Rewind(tick) = self.timeline_slot(tick) else {
            return None;
        };
        let mut changed = false;
        for input in self.history.retained_inputs_after_mut(tick) {
            for (field, value) in [
                (&mut input.underwater_movement_speed, speeds.underwater),
                (&mut input.lava_movement_speed, speeds.lava),
            ] {
                if value.is_some() && *field != value {
                    *field = value;
                    changed = true;
                }
            }
        }
        changed.then_some(tick)
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
    /// the tick to replay from when retained history changed.
    ///
    /// A retained tick rewrites only the client's unchanged run after it, so a
    /// later client transition still wins; stale ticks clamp to the oldest
    /// frame as vanilla's frame correction does. Pose and spin flags
    /// follow vanilla's flag corrections (see `server_flags`).
    pub fn apply_server_movement_flags(
        &mut self,
        tick: u64,
        flags: client_world::MovementFlagUpdate,
    ) -> Option<u64> {
        self.sync_state
            .server_flags(client_world::MovementFlagUpdate {
                sneaking: None,
                sprinting: None,
                gliding: None,
                swimming: None,
                crawling: None,
                spinning: None,
                ..flags
            });
        self.state.as_ref()?;
        let rewind = match self.timeline_slot(tick) {
            TimelineSlot::Live => None,
            TimelineSlot::Rewind(tick) => Some(tick),
            TimelineSlot::Stale => self.history.oldest_tick(),
        };
        let Some(tick) = rewind else {
            self.adopt_live_server_flags(tick, FlagWrite::from_flags(&flags));
            self.adopt_live_flags(flags);
            return None;
        };
        let anchor = *self.history.input_at(tick)?;
        let mut present = client_world::MovementFlagUpdate::default();
        let mut changed = self.retain_server_flags(tick, FlagWrite::from_flags(&flags));
        if let Some(immobile) = flags.immobile.filter(|value| *value != anchor.immobile) {
            let (edited, _) = rewrite_run(
                self.history.retained_inputs_after_mut(tick),
                |input| input.immobile == anchor.immobile,
                |input| input.immobile = immobile,
            );
            changed |= edited;
        }
        if let Some(has_gravity) = flags.has_gravity {
            changed |= rewrite_server_owned(
                self.history.retained_inputs_after_mut(tick),
                |input| &mut input.vertical_physics.has_gravity,
                has_gravity,
            );
        }
        if let Some(uniform) = flags.uniform_air_drag {
            changed |= rewrite_server_owned(
                self.history.retained_inputs_after_mut(tick),
                |input| &mut input.vertical_physics.uniform_air_drag,
                uniform,
            );
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
        if pose_flag_controls_sneak(anchor.mode)
            && let Some(sneaking) = flags.sneaking.filter(|value| *value != anchor.sneaking)
        {
            let (edited, reached) = rewrite_run(
                self.history.retained_inputs_after_mut(tick),
                |input| pose_flag_controls_sneak(input.mode) && input.sneaking == anchor.sneaking,
                |input| input.sneaking = sneaking,
            );
            changed |= edited;
            present.sneaking = reached.then_some(sneaking);
        }
        self.adopt_live_flags(present);
        changed.then_some(tick)
    }

    /// Takes the sprint/sneak states the control latches must adopt.
    pub fn take_server_control_flags(&mut self) -> Option<ServerControlFlags> {
        self.server_control_flags.take()
    }

    /// Adopts present control flags after retained local transitions have been reconciled.
    fn adopt_live_flags(&mut self, mut flags: client_world::MovementFlagUpdate) {
        if !pose_flag_controls_sneak(self.modes.mode()) {
            flags.sneaking = None;
        }
        if flags.sprinting.is_none() && flags.sneaking.is_none() {
            return;
        }
        self.modes.restore_controls(
            flags.sprinting.unwrap_or(self.modes.sprinting()),
            flags.sneaking.unwrap_or(self.modes.sneaking()),
        );
        self.sync_state
            .server_flags(client_world::MovementFlagUpdate {
                sneaking: flags.sneaking,
                sprinting: flags.sprinting,
                ..Default::default()
            });
        self.live_posture.sprinting = flags.sprinting.or(self.live_posture.sprinting);
        self.live_posture.sneaking = flags.sneaking.or(self.live_posture.sneaking);
        let pending = self.server_control_flags.get_or_insert_default();
        pending.sprinting = flags.sprinting.or(pending.sprinting);
        pending.sneaking = flags.sneaking.or(pending.sneaking);
    }
}

/// Crouch metadata owns the sneak pose; in flight the same input lane is a held descent request.
fn pose_flag_controls_sneak(mode: sim::MovementMode) -> bool {
    mode != sim::MovementMode::Flying
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

/// Sets a value only the server authors on every input after the stamp; the
/// latest update wins there, even over an earlier one with the same stamp.
fn rewrite_server_owned<'a, T: PartialEq + Copy + 'a>(
    inputs: impl Iterator<Item = &'a mut MovementInput>,
    field: fn(&mut MovementInput) -> &mut T,
    value: T,
) -> bool {
    let mut changed = false;
    for input in inputs {
        let slot = field(input);
        changed |= *slot != value;
        *slot = value;
    }
    changed
}

/// Whether a velocity keeps the next tick's collision sweep inside the query extent.
pub(super) fn motion_is_simulable(motion: [f32; 3]) -> bool {
    motion.into_iter().all(|axis| {
        axis.is_finite()
            && f64::from(axis.abs()) + sim::PLAYER_HEIGHT < sim::MAX_COLLISION_QUERY_EXTENT
    })
}
