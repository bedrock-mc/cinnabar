//! Server timeline edits replay movement without disturbing block breaking or attack input.

use std::time::Duration;

use protocol::{NetworkItemStack, VerifiedNetworkItemStack};
use sim::DestroyConditions;

use super::integration_tests::{VersionedFloor, evidence_context};
use super::{
    LocalPhysicsController, MovementSource, MovementTicker, PhysicsCorrectionMode,
    flush_player_auth_inputs, reconcile_candidate_physics_correction, reconcile_timeline_rewind,
};
use crate::melee::MeleeRuntime;
use crate::mining::FrozenMiningSelection;
use crate::survival_mining::{
    BlockBreakingAuthority, DestroyInput, DestroyTarget, SurvivalMiningRuntime,
};

const ORIGIN: [f32; 3] = [0.0, 2.620_01, 0.0];

/// A block that takes many ticks to break by hand.
fn slow_block() -> DestroyTarget {
    DestroyTarget {
        local_runtime_id: 8,
        position: [0, 1, -3],
        face: 1,
        runtime_id: 9,
        wire_runtime_id: 9,
        relative_hit: [0.5, 1.0, 0.5],
        block: sim::BlockDestroyInfo::from_facts(
            assets::compiled::block_destroy::builtin_destroy_facts("minecraft:stone"),
        ),
        conditions: DestroyConditions::default(),
        selection: FrozenMiningSelection {
            slot: 0,
            item: VerifiedNetworkItemStack::try_new(
                NetworkItemStack::empty(),
                NetworkItemStack::empty().nbt_digest,
            )
            .unwrap(),
        },
        wear: None,
        instant: false,
    }
}

/// Grounded prediction with its completed ticks queued for transport.
struct Session {
    world: VersionedFloor,
    physics: LocalPhysicsController,
    ticker: MovementTicker,
}

impl Session {
    fn new() -> Self {
        let mut physics = LocalPhysicsController::default();
        physics.reanchor_network_position(ORIGIN, 100, true);
        let mut ticker = MovementTicker::default();
        ticker.reset(7, 100, ORIGIN);
        ticker.set_source(MovementSource::Physics);
        let mut session = Self {
            world: VersionedFloor(1),
            physics,
            ticker,
        };
        session.advance(2);
        session
    }

    /// Simulates and queues `ticks` stationary ticks.
    fn advance(&mut self, ticks: u64) {
        let frame = self.physics.advance(
            Duration::from_millis(50 * ticks),
            sim::MovementInput::default(),
            &self.world,
        );
        for sample in frame.samples {
            self.ticker.enqueue_completed_physics(sample).unwrap();
        }
    }

    /// Writes every queued tick and reports which ones carried block actions.
    fn flush(&mut self) -> Vec<bool> {
        let mut identities = Vec::new();
        let mut carried = Vec::new();
        flush_player_auth_inputs(
            &mut self.ticker,
            usize::MAX,
            Some(evidence_context()),
            |identity, packet| {
                identities.push(identity);
                carried.push(
                    protocol::player_auth_input_trace_sample(&packet)
                        .unwrap()
                        .flag_names
                        .contains(&"PerformBlockActions"),
                );
                Ok::<_, ()>(())
            },
        )
        .unwrap();
        for identity in identities {
            assert!(self.ticker.acknowledge_physics_send(identity));
        }
        carried
    }

    /// Holds attack on `target` for every unsent tick.
    fn mine(&mut self, mining: &mut SurvivalMiningRuntime, target: &DestroyTarget) {
        mining.step_ticks(
            &mut self.ticker,
            DestroyInput::Held(Some(target)),
            BlockBreakingAuthority::Server,
            |_| {},
            |_, _| None,
            |_| {},
        );
    }

    /// Applies a tick-stamped knockback the server sent for an already-simulated tick.
    fn knockback(&mut self) {
        let tick = self.physics.state().unwrap().tick - 1;
        let rewind = self
            .physics
            .queue_server_motion([0.4, 0.3, 0.0], tick)
            .unwrap();
        reconcile_timeline_rewind(&mut self.ticker, &mut self.physics, rewind, &self.world)
            .unwrap();
    }

    /// Applies a small position correction for an already-simulated tick.
    fn correct(&mut self) {
        let state = self.physics.state().unwrap();
        let tick = state.tick;
        let mut corrected = [
            state.position.x as f32,
            state.position.y as f32 + protocol::PLAYER_NETWORK_OFFSET,
            state.position.z as f32,
        ];
        corrected[0] += 0.25;
        reconcile_candidate_physics_correction(
            &mut self.ticker,
            &mut self.physics,
            corrected,
            tick,
            true,
            PhysicsCorrectionMode::ReplayIfRetained,
            &self.world,
        )
        .unwrap();
    }
}

/// Knockback and replayed corrections neither abort nor restart a held block break, so a
/// player taking hits still finishes it; the vanilla rewind only replays movement.
#[test]
fn replayed_server_edits_keep_a_held_block_break_in_progress() {
    for edit in [Session::knockback as fn(&mut Session), Session::correct] {
        let mut session = Session::new();
        let mut mining = SurvivalMiningRuntime::default();
        let stone = slow_block();
        session.mine(&mut mining, &stone);
        assert_eq!(session.flush(), [true, false], "the break starts once");

        edit(&mut session);
        session.advance(2);
        session.mine(&mut mining, &stone);
        assert_eq!(
            mining.destroying_target(),
            Some((stone.position, stone.face))
        );
        assert_eq!(
            session.flush(),
            [false, false],
            "holding on the same block stays silent after the replay"
        );
    }
}

/// An attack click waiting behind a replay is still delivered afterwards.
#[test]
fn replayed_server_edits_keep_a_pending_attack_press() {
    let mut session = Session::new();
    session.flush();
    let mut melee = MeleeRuntime::default();
    melee.synchronize(session.ticker.interaction_authority_identity());
    assert!(melee.observe_input(true, false));
    session.knockback();
    melee.synchronize(session.ticker.interaction_authority_identity());
    assert!(melee.press_pending());
}

/// A replay that discards an unsent tick carrying a block action revokes queued interactions,
/// so the destroy machine restarts instead of assuming the server saw the lost action.
#[test]
fn a_replay_dropping_an_unsent_block_action_revokes_interactions() {
    let mut session = Session::new();
    let mut mining = SurvivalMiningRuntime::default();
    let stone = slow_block();
    session.mine(&mut mining, &stone);
    let before = session.ticker.interaction_authority_identity();
    session.knockback();
    assert_ne!(session.ticker.interaction_authority_identity(), before);
    session.advance(1);
    session.mine(&mut mining, &stone);
    assert_eq!(
        session.flush(),
        [true, false],
        "the break restarts on the next tick"
    );
}
