//! Selects the simulated locomotion mode (flight, glide, swim, crawl) each tick.
//!
//! The chosen mode feeds the simulator and the wire start/stop edges from one
//! source, so a flag is never asserted for a mode the simulator did not run.

use sim::{CollisionWorld, MovementMode, Vec3, WorldQueryError, pose_fits};

mod swimming_trigger;

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
    /// The flight double-tap completed this frame.
    pub fly_toggle: bool,
    /// Ability flight speed, when the server sent a usable one.
    pub fly_speed: Option<f64>,
    pub vertical_fly_speed: Option<f64>,
    /// Game mode is creative, which selects the stronger hover damping.
    pub creative_flight: bool,
    /// An elytra is equipped in the chest slot.
    pub elytra_ready: bool,
    /// Boot enchantment levels the simulator reads.
    pub depth_strider: u8,
    pub soul_speed: u8,
    /// Native sprint-stop request caused by unavailable/low hunger without flight permission.
    pub swim_hunger_blocked: bool,
}

/// Per-tick simulation facts read before the tick runs.
#[derive(Debug, Clone, Copy)]
pub(super) struct ModeObservation {
    pub feet: Vec3,
    pub on_ground: bool,
    pub velocity_y: f64,
    pub in_water: bool,
    pub in_lava: bool,
    pub sprinting: bool,
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

    /// Restores a retained authoritative mode override during correction replay.
    pub(super) fn restore_mode(&mut self, mode: MovementMode) {
        self.mode = mode;
    }

    /// Ends `mode` when it is current, as a server flag clear does.
    pub(super) fn end(&mut self, mode: MovementMode) {
        if self.mode == mode {
            self.mode = MovementMode::Walking;
        }
    }

    /// Picks this tick's mode. `fly_toggle` must be true only on the first tick of its frame.
    pub(super) fn select(
        &mut self,
        intent: ModeIntent,
        fly_toggle: bool,
        observed: ModeObservation,
        world: &(impl CollisionWorld + ?Sized),
    ) -> Result<ModeChoice, WorldQueryError> {
        if intent.ride.is_some() {
            self.last_server_flying = intent.server_flying;
            self.mode = MovementMode::Riding;
            self.sprinting = false;
            self.sneaking = observed.sneaking;
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
            in_lava: sampled.value.in_lava,
            ..observed
        };
        // SprintTrigger runs before SwimTrigger and keeps the previous actor
        // sprint flag while its previous swimming pose still contacts water.
        let sprinting =
            (self.mode == MovementMode::Swimming && observed.in_water && self.sprinting)
                || (observed.sprinting && observed.move_forward > 0.0 && !observed.sneaking);
        let liquid = observed.in_water || observed.in_lava;
        let gliding = !flying
            && intent.elytra_ready
            && !observed.on_ground
            && !liquid
            && match self.mode {
                MovementMode::Gliding => true,
                _ => observed.jump_edge && observed.velocity_y < 0.0,
            };
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

    fn airborne() -> ModeObservation {
        ModeObservation {
            feet: Vec3::new(0.0, 10.0, 0.0),
            on_ground: false,
            velocity_y: -0.5,
            in_water: false,
            in_lava: false,
            sprinting: false,
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
        toggle: bool,
        observed: ModeObservation,
    ) -> MovementMode {
        tracker
            .select(intent, toggle, observed, &Ceiling(None))
            .unwrap()
            .mode
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
        assert!(
            tracker
                .select(intent, false, airborne(), &Unavailable)
                .is_err()
        );
        assert_eq!(
            pick(&mut tracker, intent, false, airborne()),
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
            tracker
                .select(intent, false, observed, &Unavailable)
                .unwrap()
                .mode,
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
            pick(&mut tracker, intent, false, airborne()),
            MovementMode::Walking
        );
        assert_eq!(
            pick(&mut tracker, intent, true, airborne()),
            MovementMode::Flying
        );
        assert_eq!(
            pick(&mut tracker, intent, false, airborne()),
            MovementMode::Flying
        );
        let landed = ModeObservation {
            on_ground: true,
            ..airborne()
        };
        assert_eq!(
            pick(&mut tracker, intent, false, landed),
            MovementMode::Walking
        );

        pick(&mut tracker, intent, true, airborne());
        assert_eq!(
            pick(&mut tracker, intent, true, airborne()),
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
        assert_eq!(
            pick(&mut tracker, server, false, landed),
            MovementMode::Flying
        );
        assert_eq!(
            pick(&mut tracker, server, false, landed),
            MovementMode::Flying
        );
        assert_eq!(
            pick(&mut tracker, server, true, landed),
            MovementMode::Walking
        );
        assert_eq!(
            pick(&mut tracker, server, false, landed),
            MovementMode::Walking
        );
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
            pick(&mut tracker, mounted, true, airborne()),
            MovementMode::Riding
        );
        assert_eq!(
            pick(&mut tracker, ModeIntent::default(), false, airborne()),
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
        pick(&mut tracker, can, true, airborne());
        assert_eq!(
            pick(&mut tracker, ModeIntent::default(), false, airborne()),
            MovementMode::Walking
        );
    }

    #[test]
    fn elytra_deploys_on_a_falling_jump_press_and_stops_on_landing_or_water() {
        let mut tracker = ModeTracker::default();
        let intent = ModeIntent {
            elytra_ready: true,
            ..ModeIntent::default()
        };
        assert_eq!(
            pick(&mut tracker, intent, false, airborne()),
            MovementMode::Walking
        );
        let press = ModeObservation {
            jump_edge: true,
            ..airborne()
        };
        assert_eq!(
            pick(&mut tracker, intent, false, press),
            MovementMode::Gliding
        );
        assert_eq!(
            pick(&mut tracker, intent, false, airborne()),
            MovementMode::Gliding
        );
        let wet = ModeObservation {
            in_water: true,
            ..airborne()
        };
        assert_eq!(
            tracker
                .select(intent, false, wet, &Pool(20.0))
                .unwrap()
                .mode,
            MovementMode::Walking
        );

        pick(&mut tracker, intent, false, press);
        let landed = ModeObservation {
            on_ground: true,
            ..airborne()
        };
        assert_eq!(
            pick(&mut tracker, intent, false, landed),
            MovementMode::Walking
        );
    }

    #[test]
    fn rising_or_unequipped_jump_never_glides() {
        let mut tracker = ModeTracker::default();
        let ready = ModeIntent {
            elytra_ready: true,
            ..ModeIntent::default()
        };
        let rising = ModeObservation {
            jump_edge: true,
            velocity_y: 0.3,
            ..airborne()
        };
        assert_eq!(
            pick(&mut tracker, ready, false, rising),
            MovementMode::Walking
        );
        let falling = ModeObservation {
            jump_edge: true,
            ..airborne()
        };
        assert_eq!(
            pick(&mut tracker, ModeIntent::default(), false, falling),
            MovementMode::Walking
        );
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
            tracker.select(intent, false, swim, &deep).unwrap().mode,
            MovementMode::Swimming
        );
        let stopped = ModeObservation {
            sprinting: false,
            ..swim
        };
        assert_eq!(
            tracker.select(intent, false, stopped, &deep).unwrap().mode,
            MovementMode::Swimming
        );
        let idle = ModeObservation {
            move_forward: 0.0,
            ..stopped
        };
        assert_eq!(
            tracker.select(intent, false, idle, &deep).unwrap().mode,
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
            .select(ModeIntent::default(), false, observed, &Ceiling(Some(1.0)))
            .unwrap();
        assert_eq!(low.mode, MovementMode::Swimming);
        let open = tracker
            .select(ModeIntent::default(), false, observed, &Ceiling(None))
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
            .select(ModeIntent::default(), false, observed, &Ceiling(Some(1.0)))
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
            .select(ModeIntent::default(), false, observed, &Ceiling(Some(1.6)))
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
            .select(ModeIntent::default(), false, sprinting, &shallow)
            .unwrap();
        assert_eq!(choice.mode, MovementMode::Walking);

        let mut swimmer = ModeTracker {
            mode: MovementMode::Swimming,
            ..ModeTracker::default()
        };
        let choice = swimmer
            .select(ModeIntent::default(), false, sprinting, &shallow)
            .unwrap();
        assert_eq!(choice.mode, MovementMode::Swimming);
    }
}
