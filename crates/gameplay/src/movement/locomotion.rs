//! Selects the simulated locomotion mode (flight, glide, swim, crawl) each tick.
//!
//! The chosen mode feeds simulation and mode edges. Flight requests remain
//! separate from effective flight when an ability layer fixes its value.

use client_world::game_mode_capabilities::FlightAuthority;
use sim::{BlockPhysicsFlags, CollisionWorld, MovementMode, Vec3, WorldQueryError, pose_fits};

mod sprint_trigger;
mod swimming_trigger;

/// A second jump press within this many ticks of the first toggles flight.
const FLY_TRIGGER_TICKS: i32 = 7;
/// Gliding ticks after which a fresh jump press ends the glide.
const GLIDE_CANCEL_TICKS: u32 = 11;

/// What the local player is mounted on; only the steering-relevant classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RideKind {
    /// Jump-charge mounts: horse, donkey, mule and undead horses.
    Horse,
    Boat,
    Minecart,
    /// Item-steered or otherwise server-driven mounts (pig, strider, and the rest).
    Other,
}

impl RideKind {
    /// Uses the actor's admitted native class, including a custom entity's base.
    #[must_use]
    pub const fn from_native_class(class: assets::facts::entity::NativeEntityClass) -> Self {
        use assets::facts::entity::NativeEntityClass;
        match class {
            NativeEntityClass::Boat => Self::Boat,
            NativeEntityClass::Minecart => Self::Minecart,
            NativeEntityClass::Horse => Self::Horse,
            _ => Self::Other,
        }
    }
}

/// Render-frame facts the tick-level selector cannot derive from simulation.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ModeIntent {
    /// Spectator flight stays active independently of the ordinary flight toggle.
    pub spectator: bool,
    /// Terrain collision is independent of the selected travel mode.
    pub no_clip: bool,
    /// The mount the player currently rides, if any.
    pub ride: Option<RideKind>,
    /// Feet position of the rider's seat on that mount, when the mount's placement is known.
    pub ride_seat: Option<[f32; 3]>,
    /// Abilities permit flight.
    pub can_fly: bool,
    /// Effective flight value and authority retained from the winning ability layer.
    pub flight: FlightAuthority,
    /// Arrival order of the server ability update behind `flight`.
    pub ability_sequence: Option<u64>,
    /// Ability flight speed, when the server sent a usable one.
    pub fly_speed: Option<f64>,
    pub vertical_fly_speed: Option<f64>,
    /// Game mode is creative, which selects the stronger hover damping.
    pub creative_flight: bool,
    /// An unbroken elytra is equipped in the chest slot.
    pub elytra_ready: bool,
    /// Leather boots let the wearer stand on powder snow, which then also ends
    /// a glide and climbs like a ladder.
    pub can_stand_on_snow: bool,
    /// Boot enchantment levels the simulator reads.
    pub depth_strider: u8,
    pub soul_speed: u8,
    /// Leggings enchantment that raises the sneak/crawl input multiplier.
    pub swift_sneak: u8,
    /// Native sprint-stop request caused by unavailable/low hunger without flight permission.
    pub swim_hunger_blocked: bool,
    pub sprint_blocked: bool,
    pub sprint_start_blocked: bool,
    /// A user setting explicitly ended the sprint latch.
    pub stop_sprinting: bool,
}

/// Per-tick simulation facts read before the tick runs.
#[derive(Debug, Clone, Copy)]
pub(super) struct ModeObservation {
    pub feet: Vec3,
    pub on_ground: bool,
    pub in_water: bool,
    pub sprinting: bool,
    pub sprint_blinded: bool,
    pub sprint_down: bool,
    pub input_mode: protocol::PlayerInputMode,
    /// Previous tick displacement requested before collision clipping.
    pub requested_movement: Vec3,
    pub move_sideways: f32,
    pub move_forward: f32,
    pub sneaking: bool,
    pub pitch: f32,
    pub yaw: f32,
    /// Retained pre-tick pose offset, shared with swimming steering and replay.
    pub liquid_attach_height: f32,
    /// The preceding pose's liquid body probe, shared with this tick's travel input.
    pub liquid_contact_height: Option<f64>,
    /// A fresh jump press arrived this tick.
    pub jump_edge: bool,
    /// Jump state sent as `JumpDown` this tick, which the flight double-tap reads.
    pub jump_down: bool,
}

impl ModeObservation {
    /// Intent systems see item slowdown before the later sneak/crawl multiplier.
    pub(super) fn input_vector(input: sim::MovementInput) -> [f32; 2] {
        sim::MovementInput {
            sneaking: false,
            mode: MovementMode::Walking,
            ..input
        }
        .processed_controls()
        .move_vector
        .map(|axis| axis as f32)
    }
}

/// One tick's selected mode plus whether a low ceiling forces the sneak pose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ModeChoice {
    pub mode: MovementMode,
    pub forced_sneak: bool,
    pub sprinting: bool,
    /// The double-tap requested starting (true) or stopping (false) flight this tick.
    pub flight_request: Option<bool>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct ModeTracker {
    mode: MovementMode,
    last_flight: FlightAuthority,
    last_ability_sequence: Option<u64>,
    /// `Jumping` sent on the previous tick; a flight press needs it clear.
    previous_jumping: bool,
    sprinting: bool,
    sneaking: bool,
    previous_feet: Option<Vec3>,
    sprint_trigger: sprint_trigger::SprintTrigger,
    /// Ticks left for a second jump press to toggle flight; decremented every tick.
    fly_countdown: i32,
    /// Consecutive gliding ticks, including the one that started the glide.
    fall_fly_ticks: u32,
}

impl ModeTracker {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) const fn mode(&self) -> MovementMode {
        self.mode
    }

    pub(super) const fn fall_fly_ticks(&self) -> u32 {
        self.fall_fly_ticks
    }

    pub(super) fn contact_height(&self) -> f64 {
        self.mode.hitbox_height(self.sneaking)
    }

    /// Uses the compact spin body for liquid sensing, preserving the prior pose otherwise.
    pub(super) fn liquid_contact_height(&self, spinning: bool) -> f64 {
        if spinning {
            sim::PLAYER_WIDTH
        } else {
            self.contact_height()
        }
    }

    pub(super) const fn sneaking(&self) -> bool {
        self.sneaking
    }

    pub(super) const fn sprinting(&self) -> bool {
        self.sprinting
    }

    pub(super) fn restore_controls(&mut self, sprinting: bool, sneaking: bool) {
        self.sprinting = sprinting;
        self.sneaking = sneaking;
    }

    /// Captures the final primary controls for the next fixed tick's double-tap detector.
    pub(super) fn record_controls(&mut self, input: sim::MovementInput, sneak_down: bool) {
        self.previous_jumping = input.jumping;
        self.sprint_trigger
            .record_controls(input.processed_controls().move_vector[1] as f32, sneak_down);
    }

    /// Restores a retained authoritative mode override during correction replay.
    pub(super) fn restore_mode(&mut self, mode: MovementMode) {
        if mode != MovementMode::Gliding {
            self.fall_fly_ticks = 0;
        }
        self.mode = mode;
    }

    /// Ends `mode` when it is current, as a server flag clear does.
    pub(super) fn end(&mut self, mode: MovementMode) {
        if self.mode == mode {
            self.mode = MovementMode::Walking;
            self.fall_fly_ticks = 0;
        }
    }

    /// Picks this tick's mode.
    pub(super) fn select(
        &mut self,
        intent: ModeIntent,
        observed: ModeObservation,
        world: &(impl CollisionWorld + ?Sized),
    ) -> Result<ModeChoice, WorldQueryError> {
        if intent.spectator {
            self.sprinting =
                self.sprint_trigger
                    .select(self.sprinting, self.previous_feet, intent, observed);
            self.mode = MovementMode::Flying;
            self.last_flight = intent.flight;
            self.last_ability_sequence = intent.ability_sequence;
            self.fly_countdown = 0;
            self.fall_fly_ticks = 0;
            self.previous_feet = Some(observed.feet);
            self.sneaking = observed.sneaking;
            return Ok(ModeChoice {
                mode: self.mode,
                forced_sneak: false,
                sprinting: self.sprinting,
                flight_request: None,
            });
        }
        // Every ability update overwrites local flight, even when it repeats the previous value.
        let server_update = intent.flight != self.last_flight
            || intent.ability_sequence != self.last_ability_sequence;
        let held_flight = if server_update {
            intent.flight.is_flying()
        } else {
            self.mode == MovementMode::Flying
        };
        let fly_toggle = self.fly_trigger(intent, held_flight, observed.jump_down);
        let flight_request = fly_toggle.then_some(!held_flight);
        if intent.ride.is_some() {
            self.last_flight = intent.flight;
            self.last_ability_sequence = intent.ability_sequence;
            self.mode = MovementMode::Riding;
            self.sprinting = false;
            self.sneaking = false;
            self.previous_feet = Some(observed.feet);
            self.fall_fly_ticks = 0;
            return Ok(ModeChoice {
                mode: MovementMode::Riding,
                forced_sneak: false,
                sprinting: false,
                flight_request,
            });
        }
        // Flight survives ground contact; server flight does not require MayFly.
        let flying = match intent.flight {
            FlightAuthority::Fixed(value) => value,
            FlightAuthority::Mutable(server_flying) => {
                (intent.can_fly || server_flying) && held_flight != fly_toggle
            }
        };
        let sampled = sim::Simulator::default().movement_environment_with_liquid_contact(
            observed.feet,
            self.mode,
            self.sneaking,
            observed.liquid_contact_height,
            world,
        )?;
        let observed = ModeObservation {
            in_water: sampled.value.in_water,
            ..observed
        };
        // The sprint trigger runs before the swim trigger and keeps the previous actor
        // sprint flag while its previous swimming pose still contacts water.
        let sprint_candidate =
            self.sprint_trigger
                .select(self.sprinting, self.previous_feet, intent, observed);
        let sprinting =
            (self.mode == MovementMode::Swimming && observed.in_water && self.sprinting)
                || sprint_candidate;
        self.previous_feet = Some(observed.feet);
        let observed = ModeObservation {
            sprinting,
            ..observed
        };
        // A fresh jump press deploys the elytra even while rising; once the glide
        // has lasted long enough, another press ends it. Water, ground, flight and
        // climbable feet end it too, including on the tick it would start.
        let gliding = !flying
            && intent.elytra_ready
            && !observed.on_ground
            && !observed.in_water
            && match self.mode {
                MovementMode::Gliding => {
                    !observed.jump_edge || self.fall_fly_ticks < GLIDE_CANCEL_TICKS
                }
                _ => observed.jump_edge,
            }
            && !feet_end_glide(world, observed.feet, intent.can_stand_on_snow)?;
        let swimming =
            !flying && !gliding && swimming_trigger::select(self.mode, intent, observed, world)?;

        let (mode, forced_sneak) = if intent.ride.is_some() {
            (MovementMode::Riding, false)
        } else if flying {
            (MovementMode::Flying, false)
        } else if gliding {
            (MovementMode::Gliding, false)
        } else if swimming {
            (MovementMode::Swimming, false)
        } else if intent.no_clip || pose_fits(world, observed.feet, MovementMode::Walking, false)? {
            (MovementMode::Walking, false)
        } else if pose_fits(world, observed.feet, MovementMode::Walking, true)? {
            (MovementMode::Walking, true)
        } else if pose_fits(world, observed.feet, MovementMode::Crawling, false)? {
            // Low-space entry does not require an existing horizontal pose or sprint input.
            let low_mode = if observed.in_water {
                MovementMode::Swimming
            } else {
                MovementMode::Crawling
            };
            (low_mode, false)
        } else {
            (MovementMode::Walking, false)
        };
        self.last_flight = intent.flight;
        self.last_ability_sequence = intent.ability_sequence;
        // Every glide entry counts from its own first tick.
        self.fall_fly_ticks = match (self.mode, mode) {
            (MovementMode::Gliding, MovementMode::Gliding) => self.fall_fly_ticks.saturating_add(1),
            (_, MovementMode::Gliding) => 1,
            _ => 0,
        };
        self.mode = mode;
        self.sneaking = observed.sneaking || forced_sneak;
        self.sprinting =
            sprinting && !matches!(mode, MovementMode::Crawling | MovementMode::Riding);
        Ok(ModeChoice {
            mode,
            forced_sneak,
            sprinting: self.sprinting,
            flight_request,
        })
    }

    /// Vanilla's flight double-tap: every tick shortens the window, a press opens it,
    /// and a second press while it is open toggles and closes it. A press is `JumpDown`
    /// after a tick without `Jumping`, so the server replays the same toggles.
    fn fly_trigger(&mut self, intent: ModeIntent, flying: bool, jump_down: bool) -> bool {
        self.fly_countdown = (self.fly_countdown - 1).max(0);
        let eligible = flying || (intent.can_fly && intent.ride.is_none());
        if !eligible || !jump_down || self.previous_jumping {
            return false;
        }
        if self.fly_countdown < 1 {
            self.fly_countdown = FLY_TRIGGER_TICKS;
            return false;
        }
        self.fly_countdown = 0;
        true
    }
}

/// Whether the block at the feet ends a glide: anything climbable, or powder
/// snow for a wearer who can stand on it.
fn feet_end_glide(
    world: &(impl CollisionWorld + ?Sized),
    feet: Vec3,
    can_stand_on_snow: bool,
) -> Result<bool, WorldQueryError> {
    let block = [feet.x, feet.y, feet.z].map(|axis| axis.floor());
    if block
        .iter()
        .any(|axis| !axis.is_finite() || *axis < f64::from(i32::MIN) || *axis > f64::from(i32::MAX))
    {
        return Err(WorldQueryError::CoordinateOutOfRange);
    }
    let flags = world
        .block_physics(block.map(|axis| axis as i32))?
        .primary()
        .flags;
    Ok(flags.contains(BlockPhysicsFlags::CLIMBABLE)
        || (can_stand_on_snow && flags.contains(BlockPhysicsFlags::POWDER_SNOW)))
}

#[cfg(test)]
mod tests;
