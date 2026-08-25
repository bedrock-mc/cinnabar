use std::collections::VecDeque;

use thiserror::Error;

use crate::{
    CollisionWorld, MovementInput, PlayerState, SimulationError, Simulator, TickResult, Vec3,
    simulator::validate_player_state,
};

#[derive(Debug, Clone, PartialEq)]
struct PredictedFrame {
    input: MovementInput,
    state: PlayerState,
}

/// One server-authoritative velocity replacement applied immediately before
/// the named tick's simulation.
///
/// Bedrock sends impulses such as knockback as absolute post-hit velocities,
/// not accelerations. Keying the replacement by tick keeps live prediction and
/// rewind replay deterministic: re-running a retained range re-applies the
/// same overlay at the same tick instead of losing it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionOverlay {
    pub tick: u64,
    pub velocity: Vec3,
}

/// Bounded tick-keyed prediction history used by rewind corrections.
#[derive(Debug, Clone, PartialEq)]
pub struct PredictionHistory {
    capacity: usize,
    frames: VecDeque<PredictedFrame>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayResult {
    pub corrected_tick: u64,
    pub replayed_ticks: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PredictionError {
    #[error("prediction history capacity must be greater than zero")]
    ZeroCapacity,
    #[error("prediction state tick {state_tick} does not match newest retained tick {newest_tick}")]
    StateHistoryDiverged { state_tick: u64, newest_tick: u64 },
    #[error("correction tick {tick} is not retained (oldest={oldest:?}, newest={newest:?})")]
    CorrectionNotRetained {
        tick: u64,
        oldest: Option<u64>,
        newest: Option<u64>,
    },
    #[error("prediction simulation failed: {0}")]
    Simulation(#[from] SimulationError),
}

impl PredictionHistory {
    pub fn new(capacity: usize) -> Result<Self, PredictionError> {
        if capacity == 0 {
            return Err(PredictionError::ZeroCapacity);
        }
        Ok(Self {
            capacity,
            frames: VecDeque::with_capacity(capacity),
        })
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    #[must_use]
    pub fn oldest_tick(&self) -> Option<u64> {
        self.frames.front().map(|frame| frame.state.tick)
    }

    #[must_use]
    pub fn newest_tick(&self) -> Option<u64> {
        self.frames.back().map(|frame| frame.state.tick)
    }

    #[must_use]
    pub fn state_at(&self, tick: u64) -> Option<&PlayerState> {
        self.frames
            .iter()
            .find(|frame| frame.state.tick == tick)
            .map(|frame| &frame.state)
    }

    /// Returns the exact input recorded for one retained tick's prediction.
    ///
    /// Correction replays re-feed these inputs verbatim, so callers rebuilding
    /// derived per-tick facts can read the same request edges the replayed
    /// simulation acted on without retaining a second copy.
    #[must_use]
    pub fn input_at(&self, tick: u64) -> Option<&MovementInput> {
        self.frames
            .iter()
            .find(|frame| frame.state.tick == tick)
            .map(|frame| &frame.input)
    }

    /// Predicts and records one tick. Simulation failure leaves both state and
    /// history unchanged.
    pub fn predict(
        &mut self,
        state: &mut PlayerState,
        input: MovementInput,
        simulator: &Simulator,
        world: &impl CollisionWorld,
    ) -> Result<TickResult, PredictionError> {
        if let Some(newest_tick) = self.newest_tick()
            && newest_tick != state.tick
        {
            return Err(PredictionError::StateHistoryDiverged {
                state_tick: state.tick,
                newest_tick,
            });
        }
        let result = simulator.tick(state, input, world)?;
        if self.frames.len() == self.capacity {
            self.frames.pop_front();
        }
        self.frames.push_back(PredictedFrame {
            input,
            state: state.clone(),
        });
        Ok(result)
    }

    /// Replaces the retained post-tick state at the correction tick and
    /// deterministically replays every later retained input.
    ///
    /// The caller must provide the same collision-world snapshot (or a proven
    /// equivalent snapshot) used by the original predictions. Packet-specific
    /// eye/feet and delta interpretation belongs at the protocol boundary.
    pub fn rewind_and_replay(
        &mut self,
        current: &mut PlayerState,
        corrected: PlayerState,
        simulator: &Simulator,
        world: &impl CollisionWorld,
    ) -> Result<ReplayResult, PredictionError> {
        self.rewind_and_replay_traced(current, corrected, simulator, world)
            .map(|(result, _)| result)
    }

    /// Rewinds like [`Self::rewind_and_replay`] and also returns the fresh
    /// immutable result of every replayed tick in ascending tick order.
    ///
    /// Callers that retain outbound predictions use this trace to prove that
    /// replacement samples were evaluated against the exact same collision
    /// identity as the samples they replace. Failure remains transactional.
    pub fn rewind_and_replay_traced(
        &mut self,
        current: &mut PlayerState,
        corrected: PlayerState,
        simulator: &Simulator,
        world: &impl CollisionWorld,
    ) -> Result<(ReplayResult, Vec<TickResult>), PredictionError> {
        self.rewind_and_replay_traced_with_overlays(current, corrected, simulator, world, &[])
    }

    /// [`Self::rewind_and_replay_traced`] with server motion overlays.
    ///
    /// Each overlay replaces the pre-tick velocity of exactly one replayed
    /// tick, mirroring what live prediction applied when the impulse first
    /// arrived. Unknown ticks never match; overlays are never consumed so
    /// repeated replays stay deterministic.
    pub fn rewind_and_replay_traced_with_overlays(
        &mut self,
        current: &mut PlayerState,
        corrected: PlayerState,
        simulator: &Simulator,
        world: &impl CollisionWorld,
        overlays: &[MotionOverlay],
    ) -> Result<(ReplayResult, Vec<TickResult>), PredictionError> {
        validate_player_state(&corrected)?;
        let Some(index) = self
            .frames
            .iter()
            .position(|frame| frame.state.tick == corrected.tick)
        else {
            return Err(PredictionError::CorrectionNotRetained {
                tick: corrected.tick,
                oldest: self.oldest_tick(),
                newest: self.newest_tick(),
            });
        };

        let mut candidate = self.clone();
        candidate.frames[index].state = corrected.clone();
        let mut replayed_state = corrected;
        let mut ticks = Vec::with_capacity(candidate.frames.len() - index - 1);
        for frame_index in (index + 1)..candidate.frames.len() {
            let simulated_tick = candidate.frames[frame_index].state.tick;
            if let Some(overlay) = overlays
                .iter()
                .find(|overlay| overlay.tick == simulated_tick)
            {
                replayed_state.velocity = overlay.velocity;
            }
            let input = candidate.frames[frame_index].input;
            let tick = simulator.tick(&mut replayed_state, input, world)?;
            candidate.frames[frame_index].state = replayed_state.clone();
            ticks.push(tick);
        }
        let replayed_ticks = candidate.frames.len() - index - 1;
        let corrected_tick = candidate.frames[index].state.tick;
        *current = replayed_state;
        *self = candidate;
        Ok((
            ReplayResult {
                corrected_tick,
                replayed_ticks,
            },
            ticks,
        ))
    }
}
