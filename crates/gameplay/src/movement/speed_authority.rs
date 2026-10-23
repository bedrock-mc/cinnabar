use client_world::MovementSpeedAttribute;
use sim::MAX_SAFE_LIQUID_VELOCITY;

/// Largest effective speed that stays inside the collision query extent.
const MAX_SIMULABLE_MOVEMENT_SPEED: f64 = sim::MAX_COLLISION_QUERY_EXTENT / 4.0;
/// Sprint drag 0.9 settles water velocity at nine accelerations and a dolphin
/// boost doubles them, with a tenth of headroom for liquid currents.
pub(crate) const MAX_SIMULABLE_UNDERWATER_SPEED: f64 = MAX_SAFE_LIQUID_VELOCITY / 20.0;
/// Lava's 0.5 drag settles at one acceleration, again with headroom for currents.
pub(crate) const MAX_SIMULABLE_LAVA_SPEED: f64 = MAX_SAFE_LIQUID_VELOCITY / 1.1;

/// Attribute current and the native sprint modifier currently installed on it.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(crate) struct EffectiveMovementSpeed {
    attribute: Option<MovementSpeedAttribute>,
    sprinting: bool,
}

/// Accepted edits retain their operation so replay preserves unrelated modifiers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum MovementSpeedUpdate {
    Attribute(MovementSpeedAttribute),
    Effect {
        action: protocol::ActorEffectAction,
        amplifier: i32,
    },
}

impl MovementSpeedUpdate {
    /// Applies this edit to the modifier state reached at its position in the timeline.
    pub(crate) fn apply(self, speed: &mut EffectiveMovementSpeed) {
        match self {
            Self::Attribute(attribute) => {
                *speed = EffectiveMovementSpeed::authoritative(attribute, speed.sprinting());
            }
            Self::Effect { action, amplifier } => {
                let sprinting = speed.sprinting();
                let attribute = speed.attribute.get_or_insert_with(|| {
                    let mut attribute =
                        MovementSpeedAttribute::unmodified(sim::DEFAULT_MOVEMENT_SPEED as f32);
                    if sprinting {
                        attribute.set_sprint_modifier(Some(sim::SPRINT_SPEED_MULTIPLIER as f32));
                    }
                    attribute
                });
                attribute.apply_speed_effect(action, amplifier);
            }
        }
    }
}

impl EffectiveMovementSpeed {
    /// Restores the server current, its sprint modifier, and the matching actor flag.
    pub(crate) fn authoritative(attribute: MovementSpeedAttribute, sprinting: bool) -> Self {
        Self {
            attribute: Some(attribute),
            sprinting,
        }
    }

    /// Metadata changes the actor flag without installing an attribute modifier.
    pub(crate) fn adopt_server_sprinting(&mut self, sprinting: Option<bool>) {
        if let Some(sprinting) = sprinting {
            self.sprinting = sprinting;
        }
    }

    /// Vanilla sprint toggling is edge-triggered and adds/removes only the
    /// sprint speed modifier. An attribute packet replaces that modifier set.
    pub(crate) fn set_sprinting(&mut self, sprinting: bool) {
        if self.sprinting == sprinting {
            return;
        }
        self.sprinting = sprinting;
        let Some(attribute) = self.attribute.as_mut() else {
            return;
        };
        if sprinting {
            if attribute.sprint_modifier.is_none() {
                attribute.set_sprint_modifier(Some(sim::SPRINT_SPEED_MULTIPLIER as f32));
            }
        } else {
            attribute.set_sprint_modifier(None);
        }
    }

    /// Returns the actor flag used to decide whether the next sprint write is an edge.
    pub(crate) fn sprinting(self) -> bool {
        self.sprinting
    }

    /// Returns the attribute including the currently installed modifiers.
    pub(crate) fn attribute(self) -> Option<MovementSpeedAttribute> {
        self.attribute
    }

    /// The simulator's public input uses pre-sprint speed. Cancel its one fixed
    /// multiplier so its result reads our effective attribute current exactly once.
    pub(crate) fn prediction_speed(self) -> Option<f64> {
        self.attribute
            .map(|attribute| prediction_speed(attribute.current, self.sprinting))
    }
}

/// Removes the simulator sprint factor from an already effective attribute value.
fn prediction_speed(current: f64, sprinting: bool) -> f64 {
    if sprinting {
        f64::from(current as f32 / sim::SPRINT_SPEED_MULTIPLIER as f32)
    } else {
        current
    }
}

/// An authoritative flag or mode rewrite preserves effective current; it does
/// not create a local sprint modifier edge.
pub(crate) fn preserve_effective_speed(input: &mut sim::MovementInput, previous_sprinting: bool) {
    if previous_sprinting == input.sprinting {
        return;
    }
    if let Some(speed) = input.movement_speed {
        let current = if previous_sprinting {
            f64::from(speed as f32 * sim::SPRINT_SPEED_MULTIPLIER as f32)
        } else {
            speed
        };
        input.movement_speed = Some(prediction_speed(current, input.sprinting));
    }
}

#[derive(Debug, Default)]
pub struct LocalMovementSpeedAuthority {
    session_id: u64,
    dimension: i32,
    last_sequence: Option<u64>,
    speed: EffectiveMovementSpeed,
    /// Accepted edits after the latest completed tick, in arrival order.
    pending_updates: Vec<MovementSpeedUpdate>,
    /// Render-only sprint response while the next fixed tick is still pending.
    preview_current: Option<f64>,
    active_speed_effect: Option<i32>,
    /// Liquid speed attributes share the movement attribute's packet, so they order separately.
    last_liquid_sequence: Option<u64>,
    liquid: LiquidMovementSpeeds,
}

/// Effective underwater and lava movement attribute currents; `None` keeps the vanilla default.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(crate) struct LiquidMovementSpeeds {
    pub underwater: Option<f64>,
    pub lava: Option<f64>,
}

impl LocalMovementSpeedAuthority {
    /// Starts speed authority for a new session and dimension.
    pub fn begin_session(&mut self, session_id: u64, dimension: i32) {
        self.session_id = session_id;
        self.dimension = dimension;
        self.last_sequence = None;
        self.speed = EffectiveMovementSpeed::default();
        self.pending_updates.clear();
        self.preview_current = None;
        self.active_speed_effect = None;
        self.last_liquid_sequence = None;
        self.liquid = LiquidMovementSpeeds::default();
    }

    /// Clears speed authority when the active session changes dimension.
    pub fn replace_dimension(&mut self, session_id: u64, dimension: i32) {
        if session_id != self.session_id {
            return;
        }
        self.dimension = dimension;
        self.last_sequence = None;
        self.speed = EffectiveMovementSpeed::default();
        self.pending_updates.clear();
        self.preview_current = None;
        self.active_speed_effect = None;
        self.last_liquid_sequence = None;
        self.liquid = LiquidMovementSpeeds::default();
    }

    /// Accepts the next valid liquid attribute update; returns the values it adopted.
    pub(crate) fn apply_liquid(
        &mut self,
        session_id: u64,
        sequence: u64,
        dimension: i32,
        underwater: Option<f64>,
        lava: Option<f64>,
    ) -> Option<LiquidMovementSpeeds> {
        if session_id != self.session_id
            || dimension != self.dimension
            || self
                .last_liquid_sequence
                .is_some_and(|last| sequence <= last)
        {
            return None;
        }
        self.last_liquid_sequence = Some(sequence);
        let admitted = |name, value: Option<f64>, maximum: f64| {
            value.filter(|current| {
                let valid = (0.0..=maximum).contains(current);
                if !valid {
                    super::diagnostics::note_skipped_authority(name, *current);
                }
                valid
            })
        };
        let update = LiquidMovementSpeeds {
            underwater: admitted(
                "underwater_movement",
                underwater,
                MAX_SIMULABLE_UNDERWATER_SPEED,
            ),
            lava: admitted("lava_movement", lava, MAX_SIMULABLE_LAVA_SPEED),
        };
        self.liquid.underwater = update.underwater.or(self.liquid.underwater);
        self.liquid.lava = update.lava.or(self.liquid.lava);
        Some(update)
    }

    /// Liquid speed attributes for the simulator input.
    pub(crate) const fn liquid(&self) -> LiquidMovementSpeeds {
        self.liquid
    }

    /// Accepts the next valid attribute update for the active session and dimension.
    pub fn apply(
        &mut self,
        session_id: u64,
        sequence: u64,
        dimension: i32,
        attribute: MovementSpeedAttribute,
    ) -> bool {
        if session_id != self.session_id
            || dimension != self.dimension
            || self.last_sequence.is_some_and(|last| sequence <= last)
        {
            return false;
        }
        self.last_sequence = Some(sequence);
        let current = attribute.current;
        if !(0.0..=MAX_SIMULABLE_MOVEMENT_SPEED).contains(&current)
            || attribute.sprint_modifier.is_some_and(|factor| {
                !factor.is_finite()
                    || factor <= 0.0
                    || !(0.0..=MAX_SIMULABLE_MOVEMENT_SPEED)
                        .contains(&f64::from(current as f32 / factor))
            })
        {
            super::diagnostics::note_skipped_authority("movement_speed", current);
            return false;
        }
        for factor in [None, Some(sim::SPRINT_SPEED_MULTIPLIER as f32)] {
            let mut recalculated = attribute;
            recalculated.set_sprint_modifier(factor);
            if !(0.0..=MAX_SIMULABLE_MOVEMENT_SPEED).contains(&recalculated.current) {
                super::diagnostics::note_skipped_authority("movement_speed", recalculated.current);
                return false;
            }
        }
        self.speed = EffectiveMovementSpeed::authoritative(attribute, self.speed.sprinting);
        self.pending_updates.clear();
        self.pending_updates
            .push(MovementSpeedUpdate::Attribute(attribute));
        self.preview_current = None;
        true
    }

    /// Returns movement current, including the render response to pending sprint input.
    pub fn current(&self) -> Option<f64> {
        self.preview_current
            .or(self.speed.attribute.map(|attribute| attribute.current))
    }

    /// Previews sprint speed for rendering without committing an unsimulated attribute edge.
    pub(crate) fn preview_sprinting(&mut self, sprinting: bool) {
        let mut speed = self.speed;
        speed.set_sprinting(sprinting);
        self.preview_current = speed.attribute.map(|attribute| attribute.current);
    }

    /// Applies arrival-ordered Speed effect packets to effective movement current.
    pub(crate) fn apply_effect(
        &mut self,
        session_id: u64,
        sequence: u64,
        event: protocol::ActorEffectEvent,
    ) {
        if session_id != self.session_id
            || event.dimension != self.dimension
            || self.last_sequence.is_some_and(|last| sequence <= last)
        {
            return;
        }
        if event.effect_id != protocol_ids::Effect::Speed.wire_value() {
            return;
        }
        self.last_sequence = Some(sequence);
        let (action, active_speed_effect) = match event.action {
            protocol::ActorEffectAction::Add | protocol::ActorEffectAction::Update => {
                let action = if self.active_speed_effect.is_some() {
                    protocol::ActorEffectAction::Update
                } else {
                    protocol::ActorEffectAction::Add
                };
                let amplifier = self
                    .active_speed_effect
                    .map_or(event.amplifier, |current| current.max(event.amplifier));
                (action, Some(amplifier))
            }
            protocol::ActorEffectAction::Remove => (event.action, None),
            protocol::ActorEffectAction::Unknown(_) => return,
        };
        let mut attribute = self.speed.attribute.unwrap_or_else(|| {
            let mut attribute =
                MovementSpeedAttribute::unmodified(sim::DEFAULT_MOVEMENT_SPEED as f32);
            if self.speed.sprinting {
                attribute.set_sprint_modifier(Some(sim::SPRINT_SPEED_MULTIPLIER as f32));
            }
            attribute
        });
        attribute.apply_speed_effect(action, active_speed_effect.unwrap_or_default());
        if !(0.0..=MAX_SIMULABLE_MOVEMENT_SPEED).contains(&attribute.current) {
            super::diagnostics::note_skipped_authority("movement_speed", attribute.current);
            return;
        }
        for factor in [None, Some(sim::SPRINT_SPEED_MULTIPLIER as f32)] {
            let mut recalculated = attribute;
            recalculated.set_sprint_modifier(factor);
            if !(0.0..=MAX_SIMULABLE_MOVEMENT_SPEED).contains(&recalculated.current) {
                super::diagnostics::note_skipped_authority("movement_speed", recalculated.current);
                return;
            }
        }
        self.speed.attribute = Some(attribute);
        self.pending_updates.push(MovementSpeedUpdate::Effect {
            action,
            amplifier: active_speed_effect.unwrap_or_default(),
        });
        self.preview_current = None;
        self.active_speed_effect = active_speed_effect;
    }

    /// Returns the attribute fixed-tick prediction must update with sprint edges.
    pub(crate) fn attribute(&self) -> Option<MovementSpeedAttribute> {
        self.speed.attribute()
    }

    /// Edits remain pending until a successful tick or retained rewrite records them.
    pub(crate) fn pending_updates(&self) -> &[MovementSpeedUpdate] {
        &self.pending_updates
    }

    /// Returns the pre-sprint value expected by the simulator.
    pub(crate) fn prediction_speed(&self) -> Option<f64> {
        self.speed.prediction_speed()
    }

    /// Applies one local sprint transition to the effective attribute.
    pub(crate) fn set_sprinting(&mut self, sprinting: bool) {
        self.speed.set_sprinting(sprinting);
        self.preview_current = None;
    }

    /// Adopts an authoritative flag without adding a local modifier.
    pub(crate) fn adopt_server_sprinting(&mut self, sprinting: Option<bool>) {
        self.speed.adopt_server_sprinting(sprinting);
        self.preview_current = None;
    }

    /// Applies unrecorded server edits to the modifier state reached by replay.
    pub(crate) fn adopt_replayed_speed(&mut self, speed: EffectiveMovementSpeed) {
        self.speed = speed;
        for update in &self.pending_updates {
            update.apply(&mut self.speed);
        }
        self.preview_current = None;
    }

    /// Adopts speed after a completed live tick or a retained attribute rewrite.
    pub(crate) fn adopt_recorded_speed(&mut self, speed: EffectiveMovementSpeed) {
        self.speed = speed;
        self.pending_updates.clear();
        self.preview_current = None;
    }

    /// Stops sprint and folds accepted edits into loading's new prediction baseline.
    pub(crate) fn stop_for_loading(&mut self) {
        self.set_sprinting(false);
        self.pending_updates.clear();
    }
}

#[cfg(test)]
#[path = "speed_authority_tests.rs"]
mod tests;
