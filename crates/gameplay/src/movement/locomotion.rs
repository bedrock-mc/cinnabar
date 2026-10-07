//! Selects the simulated locomotion mode (flight, glide, swim, crawl) each tick.
//!
//! The chosen mode feeds the simulator and the wire start/stop edges from one
//! source, so a flag is never asserted for a mode the simulator did not run.

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
    /// Classifies a mount by entity identifier substring.
    #[must_use]
    pub fn from_identifier(identifier: &str) -> Self {
        let name = identifier.strip_prefix("minecraft:").unwrap_or(identifier);
        if name.contains("boat") || name.contains("raft") {
            Self::Boat
        } else if name.contains("minecart") {
            Self::Minecart
        } else if name.contains("horse") || matches!(name, "donkey" | "mule") {
            Self::Horse
        } else {
            Self::Other
        }
    }
}

/// Render-frame facts the tick-level selector cannot derive from simulation.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ModeIntent {
    /// The mount the player currently rides, if any.
    pub ride: Option<RideKind>,
    /// Feet position of the rider's seat on that mount, when the mount's placement is known.
    pub ride_seat: Option<[f32; 3]>,
    /// Abilities permit flight.
    pub can_fly: bool,
    /// The server's ability layers currently say the player is flying.
    pub server_flying: bool,
    /// Ability flight speed, when the server sent a usable one.
    pub fly_speed: Option<f64>,
    pub vertical_fly_speed: Option<f64>,
    /// Game mode is creative, which selects the stronger hover damping.
    pub creative_flight: bool,
    /// An unbroken elytra is equipped in the chest slot.
    pub elytra_ready: bool,
    /// Leather boots let the wearer stand on powder snow, which then also ends a glide.
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
    pub jumping: bool,
    /// A fresh jump press arrived this tick.
    pub jump_edge: bool,
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
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct ModeTracker {
    mode: MovementMode,
    last_server_flying: bool,
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

    pub(super) fn contact_height(&self) -> f64 {
        self.mode.hitbox_height(self.sneaking)
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
        let fly_toggle = self.fly_trigger(intent, observed.jump_edge);
        if intent.ride.is_some() {
            self.last_server_flying = intent.server_flying;
            self.mode = MovementMode::Riding;
            self.sprinting = false;
            self.sneaking = observed.sneaking;
            self.previous_feet = Some(observed.feet);
            self.fall_fly_ticks = 0;
            return Ok(ModeChoice {
                mode: MovementMode::Riding,
                forced_sneak: false,
                sprinting: false,
            });
        }
        // Server ability edges override locally retained flight.
        let server_rise = intent.server_flying && !self.last_server_flying;
        let server_fall = !intent.server_flying && self.last_server_flying;
        let flying = intent.can_fly
            && !server_fall
            && intent.ride.is_none()
            && match self.mode {
                MovementMode::Flying => {
                    !fly_toggle && (intent.server_flying || !observed.on_ground || observed.jumping)
                }
                _ => fly_toggle || server_rise,
            };
        let sampled = sim::Simulator::default().movement_environment(
            observed.feet,
            self.mode,
            self.sneaking,
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
        } else if pose_fits(world, observed.feet, MovementMode::Walking, false)? {
            (MovementMode::Walking, false)
        } else if pose_fits(world, observed.feet, MovementMode::Walking, true)? {
            (MovementMode::Walking, true)
        } else if matches!(self.mode, MovementMode::Swimming | MovementMode::Crawling)
            && pose_fits(world, observed.feet, MovementMode::Crawling, false)?
        {
            // Only a swimmer or an existing crawler is squeezed into a gap; walking never enters one.
            (MovementMode::Crawling, false)
        } else {
            (MovementMode::Walking, false)
        };
        self.last_server_flying = intent.server_flying;
        // Every glide entry counts from its own first tick.
        self.fall_fly_ticks = match (self.mode, mode) {
            (MovementMode::Gliding, MovementMode::Gliding) => self.fall_fly_ticks.saturating_add(1),
            (_, MovementMode::Gliding) => 1,
            _ => 0,
        };
        self.mode = mode;
        self.sneaking = observed.sneaking || forced_sneak;
        self.sprinting = sprinting
            && !matches!(
                mode,
                MovementMode::Crawling | MovementMode::Gliding | MovementMode::Riding
            );
        Ok(ModeChoice {
            mode,
            forced_sneak,
            sprinting: self.sprinting,
        })
    }

    /// Vanilla's flight double-tap: every tick shortens the window, a fresh jump
    /// press opens it, and a second press while it is open toggles and closes it.
    fn fly_trigger(&mut self, intent: ModeIntent, jump_edge: bool) -> bool {
        self.fly_countdown = (self.fly_countdown - 1).max(0);
        let eligible =
            self.mode == MovementMode::Flying || (intent.can_fly && intent.ride.is_none());
        if !eligible || !jump_edge {
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
mod tests {
    use sim::{Aabb, BlockPhysicsFacts, BlockPhysicsSample, CollisionQuery};

    use super::*;

    struct Ceiling(Option<f64>);

    impl CollisionWorld for Ceiling {
        fn primary_is_air(
            &self,
            block: [i32; 3],
        ) -> Result<Option<CollisionQuery<bool>>, WorldQueryError> {
            Ok(Some(CollisionQuery::synthetic(
                self.0.is_none_or(|height| f64::from(block[1]) < height),
            )))
        }

        fn collision_boxes(
            &self,
            query: Aabb,
        ) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
            Ok(CollisionQuery::synthetic(
                self.0
                    .map(|y| Aabb::new(Vec3::new(-8.0, y, -8.0), Vec3::new(8.0, y + 1.0, 8.0)))
                    .filter(|shape| shape.intersects(query))
                    .into_iter()
                    .collect(),
            ))
        }
    }

    /// Open water up to `surface`.
    struct Pool(f64);

    impl CollisionWorld for Pool {
        fn primary_is_air(
            &self,
            block: [i32; 3],
        ) -> Result<Option<CollisionQuery<bool>>, WorldQueryError> {
            Ok(Some(CollisionQuery::synthetic(
                f64::from(block[1]) >= self.0,
            )))
        }

        fn collision_boxes(
            &self,
            _query: Aabb,
        ) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
            Ok(CollisionQuery::synthetic(Vec::new()))
        }

        fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
            let depth = (self.0 - f64::from(block[1])).clamp(0.0, 1.0);
            let mut sample = Ceiling(None).block_physics(block)?;
            sample.layers = Box::new([BlockPhysicsFacts {
                fluid_height_blocks: depth,
                flags: if depth > 0.0 {
                    sim::BlockPhysicsFlags::WATER
                } else {
                    sim::BlockPhysicsFlags::default()
                },
                ..sample.layers[0]
            }]);
            Ok(sample)
        }
    }

    /// Every block carries `flags` with no collision.
    struct Filled(sim::BlockPhysicsFlags);

    impl CollisionWorld for Filled {
        fn collision_boxes(
            &self,
            _query: Aabb,
        ) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
            Ok(CollisionQuery::synthetic(Vec::new()))
        }

        fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
            let mut sample = Ceiling(None).block_physics(block)?;
            sample.layers = Box::new([BlockPhysicsFacts {
                fluid_height_blocks: if self.0.contains(sim::BlockPhysicsFlags::LAVA) {
                    1.0
                } else {
                    0.0
                },
                flags: self.0,
                ..sample.layers[0]
            }]);
            Ok(sample)
        }
    }

    fn airborne() -> ModeObservation {
        ModeObservation {
            feet: Vec3::new(0.0, 10.0, 0.0),
            on_ground: false,
            in_water: false,
            sprinting: false,
            sprint_blinded: false,
            sprint_down: false,
            input_mode: protocol::PlayerInputMode::Mouse,
            requested_movement: Vec3::ZERO,
            move_sideways: 0.0,
            move_forward: 0.0,
            sneaking: false,
            pitch: 0.0,
            yaw: 0.0,
            liquid_attach_height: protocol::STANDING_PLAYER_EYE_HEIGHT,
            jumping: false,
            jump_edge: false,
        }
    }

    fn pick(
        tracker: &mut ModeTracker,
        intent: ModeIntent,
        observed: ModeObservation,
    ) -> MovementMode {
        tracker
            .select(intent, observed, &Ceiling(None))
            .unwrap()
            .mode
    }

    fn pressed(observed: ModeObservation) -> ModeObservation {
        ModeObservation {
            jump_edge: true,
            ..observed
        }
    }

    /// Two jump presses on consecutive ticks: the flight double-tap.
    fn double_tap(
        tracker: &mut ModeTracker,
        intent: ModeIntent,
        observed: ModeObservation,
    ) -> MovementMode {
        pick(tracker, intent, pressed(observed));
        pick(tracker, intent, pressed(observed))
    }

    /// Unavailable world queries make mode selection retryable.
    struct Unavailable;

    impl CollisionWorld for Unavailable {
        fn collision_boxes(
            &self,
            _query: Aabb,
        ) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
            Err(WorldQueryError::InvalidBounds)
        }
        fn block_physics(&self, _block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
            Err(WorldQueryError::InvalidBounds)
        }
    }

    #[test]
    fn review_failed_mode_selection_retains_the_server_flight_clear() {
        let mut tracker = ModeTracker {
            mode: MovementMode::Flying,
            last_server_flying: true,
            ..ModeTracker::default()
        };
        let intent = ModeIntent {
            can_fly: true,
            ..Default::default()
        };
        assert!(tracker.select(intent, airborne(), &Unavailable).is_err());
        assert_eq!(
            pick(&mut tracker, intent, airborne()),
            MovementMode::Walking
        );
    }

    #[test]
    fn review_riding_does_not_query_swimming_or_pose_fit() {
        let mut tracker = ModeTracker::default();
        let intent = ModeIntent {
            ride: Some(RideKind::Boat),
            ..Default::default()
        };
        let observed = ModeObservation {
            in_water: true,
            sprinting: true,
            ..airborne()
        };
        assert_eq!(
            tracker.select(intent, observed, &Unavailable).unwrap().mode,
            MovementMode::Riding
        );
    }

    #[test]
    fn flight_toggles_on_and_off_and_lands_when_grounded_and_idle() {
        let mut tracker = ModeTracker::default();
        let intent = ModeIntent {
            can_fly: true,
            ..ModeIntent::default()
        };
        assert_eq!(
            pick(&mut tracker, intent, airborne()),
            MovementMode::Walking
        );
        assert_eq!(
            double_tap(&mut tracker, intent, airborne()),
            MovementMode::Flying
        );
        assert_eq!(pick(&mut tracker, intent, airborne()), MovementMode::Flying);
        let landed = ModeObservation {
            on_ground: true,
            ..airborne()
        };
        assert_eq!(pick(&mut tracker, intent, landed), MovementMode::Walking);

        double_tap(&mut tracker, intent, airborne());
        assert_eq!(
            double_tap(&mut tracker, intent, airborne()),
            MovementMode::Walking
        );
    }

    #[test]
    fn server_set_flight_starts_on_its_edge_and_survives_the_ground() {
        let mut tracker = ModeTracker::default();
        let server = ModeIntent {
            can_fly: true,
            server_flying: true,
            ..ModeIntent::default()
        };
        let landed = ModeObservation {
            on_ground: true,
            ..airborne()
        };
        assert_eq!(pick(&mut tracker, server, landed), MovementMode::Flying);
        assert_eq!(pick(&mut tracker, server, landed), MovementMode::Flying);
        assert_eq!(
            double_tap(&mut tracker, server, landed),
            MovementMode::Walking
        );
        assert_eq!(pick(&mut tracker, server, landed), MovementMode::Walking);
    }

    #[test]
    fn mounting_overrides_every_other_mode_and_dismounting_resumes_walking() {
        let mut tracker = ModeTracker::default();
        let mounted = ModeIntent {
            ride: Some(RideKind::Boat),
            can_fly: true,
            elytra_ready: true,
            ..ModeIntent::default()
        };
        assert_eq!(
            pick(&mut tracker, mounted, airborne()),
            MovementMode::Riding
        );
        assert_eq!(
            pick(&mut tracker, ModeIntent::default(), airborne()),
            MovementMode::Walking
        );
    }

    #[test]
    fn rides_classify_by_identifier() {
        for (id, kind) in [
            ("minecraft:boat", RideKind::Boat),
            ("minecraft:chest_boat", RideKind::Boat),
            ("minecraft:chest_raft", RideKind::Boat),
            ("minecraft:minecart", RideKind::Minecart),
            ("minecraft:hopper_minecart", RideKind::Minecart),
            ("minecraft:horse", RideKind::Horse),
            ("minecraft:skeleton_horse", RideKind::Horse),
            ("minecraft:mule", RideKind::Horse),
            ("minecraft:pig", RideKind::Other),
            ("minecraft:strider", RideKind::Other),
        ] {
            assert_eq!(RideKind::from_identifier(id), kind, "{id}");
        }
    }

    #[test]
    fn losing_flight_permission_ends_flight() {
        let mut tracker = ModeTracker::default();
        let can = ModeIntent {
            can_fly: true,
            ..ModeIntent::default()
        };
        double_tap(&mut tracker, can, airborne());
        assert_eq!(
            pick(&mut tracker, ModeIntent::default(), airborne()),
            MovementMode::Walking
        );
    }

    #[test]
    fn elytra_deploys_on_a_jump_press_and_stops_on_landing_or_water() {
        let mut tracker = ModeTracker::default();
        let intent = ModeIntent {
            elytra_ready: true,
            ..ModeIntent::default()
        };
        assert_eq!(
            pick(&mut tracker, intent, airborne()),
            MovementMode::Walking
        );
        assert_eq!(
            pick(&mut tracker, intent, pressed(airborne())),
            MovementMode::Gliding
        );
        assert_eq!(
            pick(&mut tracker, intent, airborne()),
            MovementMode::Gliding
        );
        let wet = ModeObservation {
            in_water: true,
            ..airborne()
        };
        assert_eq!(
            tracker.select(intent, wet, &Pool(20.0)).unwrap().mode,
            MovementMode::Walking
        );

        pick(&mut tracker, intent, pressed(airborne()));
        let landed = ModeObservation {
            on_ground: true,
            ..airborne()
        };
        assert_eq!(pick(&mut tracker, intent, landed), MovementMode::Walking);
    }

    /// Unlike water, lava neither prevents nor ends a glide.
    #[test]
    fn lava_does_not_end_a_glide() {
        let mut tracker = ModeTracker::default();
        let intent = ModeIntent {
            elytra_ready: true,
            ..ModeIntent::default()
        };
        let lava = Filled(sim::BlockPhysicsFlags::LAVA);
        for observed in [pressed(airborne()), airborne()] {
            assert_eq!(
                tracker.select(intent, observed, &lava).unwrap().mode,
                MovementMode::Gliding
            );
        }
    }

    #[test]
    fn unequipped_jump_never_glides() {
        let mut tracker = ModeTracker::default();
        assert_eq!(
            pick(&mut tracker, ModeIntent::default(), pressed(airborne())),
            MovementMode::Walking
        );
    }

    /// A second press ends the glide only once it has lasted eleven ticks.
    #[test]
    fn a_fresh_jump_press_cancels_a_glide_after_eleven_ticks() {
        let mut tracker = ModeTracker::default();
        let intent = ModeIntent {
            elytra_ready: true,
            ..ModeIntent::default()
        };
        assert_eq!(
            pick(&mut tracker, intent, pressed(airborne())),
            MovementMode::Gliding
        );
        for _ in 0..9 {
            assert_eq!(
                pick(&mut tracker, intent, airborne()),
                MovementMode::Gliding
            );
        }
        assert_eq!(
            pick(&mut tracker, intent, pressed(airborne())),
            MovementMode::Gliding
        );
        assert_eq!(
            pick(&mut tracker, intent, pressed(airborne())),
            MovementMode::Walking
        );
    }

    /// A server-ended glide never carries its tick count into the next glide.
    #[test]
    fn an_ended_glide_restarts_the_cancel_window() {
        let mut tracker = ModeTracker::default();
        let intent = ModeIntent {
            elytra_ready: true,
            ..ModeIntent::default()
        };
        pick(&mut tracker, intent, pressed(airborne()));
        for _ in 0..12 {
            pick(&mut tracker, intent, airborne());
        }
        tracker.end(MovementMode::Gliding);
        assert_eq!(
            pick(&mut tracker, intent, pressed(airborne())),
            MovementMode::Gliding
        );
        assert_eq!(
            pick(&mut tracker, intent, pressed(airborne())),
            MovementMode::Gliding,
            "a press one tick into the new glide cannot cancel it"
        );
    }

    /// Climbable feet end a glide; powder snow does only for a wearer who can stand on it.
    #[test]
    fn climbable_feet_end_a_glide() {
        let intent = ModeIntent {
            elytra_ready: true,
            ..ModeIntent::default()
        };
        let glide = |intent, world: &Filled| {
            let mut tracker = ModeTracker::default();
            tracker
                .select(intent, pressed(airborne()), world)
                .unwrap()
                .mode
        };
        let ladder = Filled(sim::BlockPhysicsFlags::CLIMBABLE);
        assert_eq!(glide(intent, &ladder), MovementMode::Walking);
        let snow = Filled(sim::BlockPhysicsFlags::POWDER_SNOW);
        assert_eq!(glide(intent, &snow), MovementMode::Gliding);
        let booted = ModeIntent {
            can_stand_on_snow: true,
            ..intent
        };
        assert_eq!(glide(booted, &snow), MovementMode::Walking);
    }

    /// A second press within six ticks of the first toggles flight.
    #[test]
    fn flight_double_tap_window_lasts_seven_ticks() {
        let intent = ModeIntent {
            can_fly: true,
            ..ModeIntent::default()
        };
        for (idle_ticks, toggles) in [(5, true), (6, false)] {
            let mut tracker = ModeTracker::default();
            assert_eq!(
                pick(&mut tracker, intent, pressed(airborne())),
                MovementMode::Walking
            );
            for _ in 0..idle_ticks {
                pick(&mut tracker, intent, airborne());
            }
            let mode = pick(&mut tracker, intent, pressed(airborne()));
            assert_eq!(mode == MovementMode::Flying, toggles, "{idle_ticks}");
        }
    }

    #[test]
    fn sprint_starts_swimming_and_continuation_survives_sprint_clear() {
        let mut tracker = ModeTracker::default();
        let swim = ModeObservation {
            in_water: true,
            sprinting: true,
            move_forward: 1.0,
            ..airborne()
        };
        let intent = ModeIntent::default();
        let deep = Pool(20.0);
        assert_eq!(
            tracker.select(intent, swim, &deep).unwrap().mode,
            MovementMode::Swimming
        );
        let stopped = ModeObservation {
            sprinting: false,
            ..swim
        };
        assert_eq!(
            tracker.select(intent, stopped, &deep).unwrap().mode,
            MovementMode::Swimming
        );
        let idle = ModeObservation {
            move_forward: 0.0,
            ..stopped
        };
        assert_eq!(
            tracker.select(intent, idle, &deep).unwrap().mode,
            MovementMode::Walking
        );
    }

    #[test]
    fn a_swimmer_retains_its_horizontal_pose_until_standing_fits() {
        let mut tracker = ModeTracker {
            mode: MovementMode::Swimming,
            ..ModeTracker::default()
        };
        let observed = ModeObservation {
            on_ground: true,
            feet: Vec3::new(0.0, 0.0, 0.0),
            ..airborne()
        };
        let low = tracker
            .select(ModeIntent::default(), observed, &Ceiling(Some(1.0)))
            .unwrap();
        assert_eq!(low.mode, MovementMode::Swimming);
        let open = tracker
            .select(ModeIntent::default(), observed, &Ceiling(None))
            .unwrap();
        assert_eq!(open.mode, MovementMode::Walking);
        assert!(!open.forced_sneak);
    }

    #[test]
    fn a_walker_is_never_squeezed_into_a_gap_it_does_not_fit() {
        let mut tracker = ModeTracker::default();
        let observed = ModeObservation {
            on_ground: true,
            feet: Vec3::new(0.0, 0.0, 0.0),
            ..airborne()
        };
        let choice = tracker
            .select(ModeIntent::default(), observed, &Ceiling(Some(1.0)))
            .unwrap();
        assert_eq!(choice.mode, MovementMode::Walking);
    }

    #[test]
    fn a_gap_that_fits_a_sneak_but_not_a_stand_forces_the_sneak_pose() {
        let mut tracker = ModeTracker::default();
        let observed = ModeObservation {
            on_ground: true,
            feet: Vec3::new(0.0, 0.0, 0.0),
            ..airborne()
        };
        let choice = tracker
            .select(ModeIntent::default(), observed, &Ceiling(Some(1.6)))
            .unwrap();
        assert_eq!(choice.mode, MovementMode::Walking);
        assert!(choice.forced_sneak);
    }

    /// Shallow water with the head above the surface never starts a swim; a swimmer keeps going.
    #[test]
    fn a_swim_starts_only_with_the_head_in_water() {
        let sprinting = ModeObservation {
            in_water: true,
            sprinting: true,
            move_forward: 1.0,
            ..airborne()
        };
        let shallow = Pool(10.9);
        let mut walker = ModeTracker::default();
        let choice = walker
            .select(ModeIntent::default(), sprinting, &shallow)
            .unwrap();
        assert_eq!(choice.mode, MovementMode::Walking);

        let mut swimmer = ModeTracker {
            mode: MovementMode::Swimming,
            ..ModeTracker::default()
        };
        let choice = swimmer
            .select(ModeIntent::default(), sprinting, &shallow)
            .unwrap();
        assert_eq!(choice.mode, MovementMode::Swimming);
    }
}
