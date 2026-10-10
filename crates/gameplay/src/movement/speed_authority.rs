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

impl EffectiveMovementSpeed {
    /// Restores the server current, its sprint modifier, and the matching actor flag.
    pub(crate) fn authoritative(attribute: MovementSpeedAttribute, sprinting: bool) -> Self {
        Self {
            attribute: Some(attribute),
            sprinting,
        }
    }

    /// Metadata changes the actor flag without installing an attribute modifier.
    fn adopt_server_sprinting(&mut self, sprinting: Option<bool>) {
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
        true
    }

    /// Returns the effective attribute current after local sprint transitions.
    pub fn current(&self) -> Option<f64> {
        self.speed.attribute.map(|attribute| attribute.current)
    }

    /// Returns the pre-sprint value expected by the simulator.
    pub(crate) fn prediction_speed(&self) -> Option<f64> {
        self.speed.prediction_speed()
    }

    /// Applies one local sprint transition to the effective attribute.
    pub(crate) fn set_sprinting(&mut self, sprinting: bool) {
        self.speed.set_sprinting(sprinting);
    }

    /// Adopts an authoritative flag without adding a local modifier.
    pub(crate) fn adopt_server_sprinting(&mut self, sprinting: Option<bool>) {
        self.speed.adopt_server_sprinting(sprinting);
    }

    /// Carries the replayed modifier state into future movement frames.
    pub(crate) fn adopt_replayed_speed(&mut self, speed: EffectiveMovementSpeed) {
        self.speed = speed;
    }
}

#[cfg(test)]
#[path = "speed_authority_tests.rs"]
pub(crate) mod tests;
