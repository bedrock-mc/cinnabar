//! Liquid sensing agrees across pose selection, travel, published flags and replay.

use super::*;
use crate::movement::{MovementSource, MovementTicker, reconcile_candidate_physics_correction};
use protocol::PlayerInputFlags;

struct SurfaceWater;

impl CollisionWorld for SurfaceWater {
    /// Leaves solid collision out of the liquid sensing comparison.
    fn collision_boxes(
        &self,
        _: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        Ok(sim::CollisionQuery::synthetic(Vec::new()))
    }

    /// Places water only in the layer below the partial-contact actor.
    fn block_physics(
        &self,
        block: [i32; 3],
    ) -> Result<sim::BlockPhysicsSample, sim::WorldQueryError> {
        let water = block[1] == 0;
        Ok(sim::BlockPhysicsSample {
            layers: Box::new([sim::BlockPhysicsFacts {
                friction: 0.6,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: if water { 1.0 } else { 0.0 },
                flags: if water {
                    sim::BlockPhysicsFlags::WATER
                } else {
                    sim::BlockPhysicsFlags::default()
                },
                surface_response: sim::SurfaceResponse::None,
            }]),
            identity: sim::CollisionQuery::synthetic(()).identity,
        })
    }
}

/// Anchors real local physics and its outbound movement owner at one liquid boundary.
fn controller(feet_y: f32) -> (LocalPhysicsController, MovementTicker) {
    let origin = [0.0, feet_y + PLAYER_NETWORK_OFFSET, 0.0];
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(origin, 100, false);
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, origin);
    ticker.set_source(MovementSource::Physics);
    (physics, ticker)
}

/// Publishes one tick with a usable elytra and an optional fresh jump request.
fn advance(
    physics: &mut LocalPhysicsController,
    ticker: &mut MovementTicker,
    jump: bool,
) -> PhysicsMovementSample {
    let frame = physics.advance_with_context(
        simulation_clock::TICK_DURATION,
        MovementInput {
            jumping: jump,
            ..Default::default()
        },
        PhysicsSampleContext {
            mode_intent: ModeIntent {
                elytra_ready: true,
                ..Default::default()
            },
            ..Default::default()
        },
        &SurfaceWater,
    );
    assert!(frame.blocked.is_none(), "{:?}", frame.blocked);
    assert_eq!(frame.samples.len(), 1);
    let sample = frame.samples[0].clone();
    ticker.enqueue_completed_physics(sample.clone()).unwrap();
    sample
}

/// Replays the retained input stream and verifies both simulation and outbound mode flags.
fn assert_replay(
    physics: &mut LocalPhysicsController,
    ticker: &mut MovementTicker,
    anchor: &PhysicsMovementSample,
) {
    let expected = physics.clone();
    let flags: Vec<_> = ticker
        .pending_snapshots()
        .iter()
        .filter(|input| input.tick > anchor.tick)
        .map(|input| (input.tick, input.flags))
        .collect();
    reconcile_candidate_physics_correction(
        ticker,
        physics,
        anchor.position,
        anchor.tick,
        false,
        PhysicsCorrectionMode::ReplayIfRetained,
        &SurfaceWater,
    )
    .unwrap();
    assert_eq!(physics.state(), expected.state());
    assert_eq!(physics.mode(), expected.mode());
    assert_eq!(physics.in_water(), expected.in_water());
    assert_eq!(
        ticker
            .pending_snapshots()
            .iter()
            .map(|input| (input.tick, input.flags))
            .collect::<Vec<_>>(),
        flags
    );
    for (frame, prior) in physics
        .controller_history
        .iter()
        .zip(&expected.controller_history)
    {
        assert_eq!(
            frame.input.liquid_contact_height,
            prior.input.liquid_contact_height
        );
        assert_eq!(
            frame.input.liquid_contact_height,
            physics
                .history
                .input_at(frame.tick)
                .unwrap()
                .liquid_contact_height
        );
    }
}

#[test]
fn riptide_partial_water_contact_blocks_gliding_in_live_input_and_replay() {
    for (feet_y, spinning, water) in [
        (0.0, true, true),
        (0.6, true, true),
        (1.4, true, false),
        (0.6, false, false),
    ] {
        let (mut physics, mut ticker) = controller(feet_y);
        if spinning {
            assert!(physics.request_riptide(3, [0.0; 2]));
        }
        let anchor = advance(&mut physics, &mut ticker, false);
        let sample = advance(&mut physics, &mut ticker, true);
        assert_eq!(physics.in_water(), water);
        assert_eq!(sample.processed.mode == sim::MovementMode::Gliding, !water);
        let flags = ticker.pending_snapshots().last().unwrap().flags;
        assert_eq!(
            flags.bits() & PlayerInputFlags::START_GLIDING.bits() != 0,
            !water
        );
        assert_replay(&mut physics, &mut ticker, &anchor);
    }
}

#[test]
fn riptide_liquid_probe_changes_at_start_and_expiry_without_a_stale_glide_gate() {
    let (mut physics, mut ticker) = controller(0.6);
    assert!(physics.request_riptide(3, [0.0; 2]));
    let mut anchor = None;
    // Travel suppression holds the partial-contact origin while real spin ticks age.
    for index in 0..LOCAL_PHYSICS_HISTORY_CAPACITY {
        let spinning_before = physics.state().unwrap().spin_attack_ticks.is_some();
        physics.skip_next_travel_tick();
        let sample = advance(&mut physics, &mut ticker, false);
        if anchor.is_none() {
            anchor = Some(sample.clone());
        }
        assert_eq!(sample.processed.spin_started, index == 0);
        assert_eq!(physics.in_water(), spinning_before);
        let frame = physics.controller_history.back().unwrap();
        let height = if spinning_before {
            sim::PLAYER_WIDTH
        } else {
            sim::PLAYER_HEIGHT
        };
        assert_eq!(frame.input.liquid_contact_height, Some(height));
        if !sample.processed.spinning {
            assert!(index > 0, "the launch must start a retained spin");
            break;
        }
    }
    assert!(
        physics.state().unwrap().spin_attack_ticks.is_none(),
        "spin must expire within retained history"
    );
    let sample = advance(&mut physics, &mut ticker, true);
    assert!(!physics.in_water());
    assert_eq!(sample.processed.mode, sim::MovementMode::Gliding);
    assert_eq!(
        physics
            .controller_history
            .back()
            .unwrap()
            .input
            .liquid_contact_height,
        Some(sim::PLAYER_HEIGHT)
    );
    assert_replay(&mut physics, &mut ticker, &anchor.unwrap());
}
