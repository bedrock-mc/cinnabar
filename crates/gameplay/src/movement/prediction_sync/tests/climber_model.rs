use super::*;

/// A reproducible stream of metadata, input and terrain choices.
struct Random(u64);

impl Random {
    /// Advances a fixed-seed generator without depending on a random library version.
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Static, seeded scaffold layers let movement regenerate different contact each tick.
struct ContactWorld(u64);

impl sim::CollisionWorld for ContactWorld {
    /// Leaves travel unconstrained so contact and descent determine the result.
    fn collision_boxes(
        &self,
        _query: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        Ok(sim::CollisionQuery::synthetic(Vec::new()))
    }

    /// Selects scaffolding from the cell identity, independently of query order.
    fn block_physics(
        &self,
        block: [i32; 3],
    ) -> Result<sim::BlockPhysicsSample, sim::WorldQueryError> {
        let mut hash = self.0;
        for coordinate in block {
            hash = hash
                .wrapping_mul(0x9e37_79b9)
                .wrapping_add(coordinate as u64);
        }
        Ok(sim::BlockPhysicsSample {
            layers: Box::new([sim::BlockPhysicsFacts {
                friction: 0.6,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: 0.0,
                flags: if hash.rotate_right(13) & 1 == 0 {
                    sim::BlockPhysicsFlags::SCAFFOLDING
                } else {
                    sim::BlockPhysicsFlags::AIR
                },
                surface_response: sim::SurfaceResponse::None,
            }]),
            identity: sim::CollisionQuery::synthetic(()).identity,
        })
    }
}

/// An accepted edit in the reference stream, retaining its arrival order.
#[derive(Clone, Copy)]
struct Edit {
    mask: u64,
    word: u64,
}

/// A completed live input and its immutable comparison capture.
#[derive(Clone)]
struct ModelTick {
    input: sim::MovementInput,
    entry: u64,
    edits: Vec<Edit>,
}

/// Reads only the locally owned climber flags, without consuming a sync countdown.
fn flags(physics: &LocalPhysicsController) -> u64 {
    physics.prediction_sync_flags().extended_flags.unwrap_or(0) & ClimberFlags::mask()
}

/// Applies reference edits as ordinary live writes before the next input.
fn apply_edits(physics: &mut LocalPhysicsController, edits: &[Edit]) {
    for edit in edits {
        let word = (flags(physics) & !edit.mask) | (edit.word & edit.mask);
        assert_eq!(
            physics.apply_server_movement_flags(
                0,
                client_world::MovementFlagUpdate {
                    extended_flags: Some(word),
                    ..Default::default()
                },
            ),
            None
        );
    }
}

/// Rebuilds the reference with live ticks only; it never uses correction replay.
fn run_live_model(
    ticks: &[ModelTick],
    pending: &[Edit],
    world: &ContactWorld,
) -> (
    LocalPhysicsController,
    Vec<crate::movement::PhysicsMovementSample>,
) {
    let mut physics = due_physics();
    let mut samples = Vec::new();
    for tick in ticks {
        apply_edits(&mut physics, &tick.edits);
        let frame = physics.advance(simulation_clock::TICK_DURATION, tick.input, world);
        assert_eq!(frame.completed_ticks, 1);
        samples.extend(frame.samples);
    }
    apply_edits(&mut physics, pending);
    (physics, samples)
}

/// Compares every replayed movement sample and the final flags with fresh live execution.
fn check_replay(
    physics: &mut LocalPhysicsController,
    anchor: u64,
    ticks: &[ModelTick],
    pending: &[Edit],
    world: &ContactWorld,
    seed: u64,
) {
    let (expected, samples) = run_live_model(ticks, pending, world);
    let replay = physics.replay_retained_from(anchor, world).unwrap();
    let expected_samples: Vec<_> = samples
        .iter()
        .filter(|sample| sample.tick > anchor)
        .collect();
    assert_eq!(replay.replayed_samples.len(), expected_samples.len());
    for (actual, expected) in replay.replayed_samples.iter().zip(expected_samples) {
        assert_eq!(
            actual.movement, expected.movement,
            "seed={seed} anchor={anchor} tick={}",
            actual.tick
        );
        assert_eq!(
            actual.position, expected.position,
            "seed={seed} anchor={anchor} tick={}",
            actual.tick
        );
        assert_eq!(
            actual.velocity, expected.velocity,
            "seed={seed} anchor={anchor} tick={}",
            actual.tick
        );
    }
    assert_eq!(
        flags(physics),
        flags(&expected),
        "seed={seed} anchor={anchor}"
    );
}

#[test]
fn seeded_climber_streams_match_live_execution_after_arbitrary_replay() {
    for seed in 1..=32 {
        let mut random = Random(seed);
        let world = ContactWorld(random.next());
        let mut physics = due_physics();
        let first = sim::MovementInput::default();
        physics.advance(simulation_clock::TICK_DURATION, first, &world);
        let mut ticks = vec![ModelTick {
            input: first,
            entry: 0,
            edits: Vec::new(),
        }];
        let mut pending = Vec::<Edit>::new();
        for _ in 0..20 {
            let current = physics.state().unwrap().tick;
            for _ in 0..random.next() % 4 {
                let stamp = match random.next() % 4 {
                    0 => 0,
                    1 => current,
                    2 => current + 1 + random.next() % 100,
                    _ => 101 + random.next() % (current - 100),
                };
                let word = random.next() & ClimberFlags::mask();
                let historical = stamp != 0 && stamp < current;
                let baseline = if historical {
                    ticks[(stamp - 100) as usize].entry
                } else {
                    flags(&physics)
                };
                let edit = Edit {
                    mask: (baseline ^ word) & ClimberFlags::mask(),
                    word,
                };
                if historical {
                    ticks[(stamp - 100) as usize].edits.push(edit);
                    for pending in &mut pending {
                        pending.mask &= !edit.mask;
                    }
                } else {
                    pending.push(edit);
                }
                let rewind = physics.apply_server_movement_flags(
                    stamp,
                    client_world::MovementFlagUpdate {
                        extended_flags: Some(word),
                        ..Default::default()
                    },
                );
                assert_eq!(rewind, (historical && edit.mask != 0).then_some(stamp));
                if let Some(anchor) = rewind {
                    check_replay(&mut physics, anchor, &ticks, &pending, &world, seed);
                }
            }
            let input = sim::MovementInput {
                sneaking: random.next() & 1 != 0,
                forward: (random.next() % 3) as f64 - 1.0,
                strafe: (random.next() % 3) as f64 - 1.0,
                ..Default::default()
            };
            let entry = flags(&physics);
            let frame = physics.advance(simulation_clock::TICK_DURATION, input, &world);
            assert_eq!(frame.completed_ticks, 1);
            ticks.push(ModelTick {
                input,
                entry,
                edits: std::mem::take(&mut pending),
            });
            let (expected, samples) = run_live_model(&ticks, &pending, &world);
            assert_eq!(
                frame.samples[0].movement,
                samples.last().unwrap().movement,
                "seed={seed} tick={}",
                current + 1
            );
            assert_eq!(
                flags(&physics),
                flags(&expected),
                "seed={seed} tick={}",
                current + 1
            );
            let anchor = 101 + random.next() % ticks.len() as u64;
            check_replay(&mut physics, anchor, &ticks, &pending, &world, seed);
        }

        // Every live set capture must still admit a clear after arbitrary replay history.
        for index in 1..ticks.len() {
            let bit = 1 << (ActorFlag::OverDescendingBlock.wire_value() % 64);
            if ticks[index].entry & bit == 0 {
                continue;
            }
            let mut cleared = physics.clone();
            let stamp = 100 + index as u64;
            let word = ticks[index].entry & !bit;
            assert_eq!(
                cleared.apply_server_movement_flags(
                    stamp,
                    client_world::MovementFlagUpdate {
                        extended_flags: Some(word),
                        ..Default::default()
                    }
                ),
                Some(stamp)
            );
            let mut corrected_ticks = ticks.clone();
            corrected_ticks[index]
                .edits
                .push(Edit { mask: bit, word: 0 });
            for anchor in [stamp, 101, stamp] {
                check_replay(&mut cleared, anchor, &corrected_ticks, &[], &world, seed);
            }
        }
    }
}
