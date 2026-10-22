//! Frozen interaction selection, targets and pick reach shared by mining and block use.

use protocol::identity::FifoSequence;

use render_api::PoseGeneration;
use semantic_input::identity::{InputAuthorityGeneration, InputSequence};
use simulation_clock::PhysicsTick;
use simulation_clock::identity::{PositionAuthorityGeneration, SessionGeneration};

use protocol::{PlayerAuthInputInteractions, PlayerInputMode, VerifiedNetworkItemStack};
use semantic_input::InputMode;
use sim::WorldCollisionIdentity;

/// Creative pick ranges observed for the three input modes the app exposes.
pub const CREATIVE_MOUSE_REACH_BLOCKS: f64 = 5.7_f32 as f64;
/// Creative controller pick distance.
pub const CREATIVE_GAMEPAD_REACH_BLOCKS: f64 = 5.6_f32 as f64;
/// Creative touch pick distance.
pub const CREATIVE_TOUCH_REACH_BLOCKS: f64 = 12.0;
/// Survival keeps the mouse and gamepad ranges; touch is shorter.
pub const SURVIVAL_TOUCH_REACH_BLOCKS: f64 = 6.7_f32 as f64;

/// The identities of one frozen interaction frame. Different domains cannot be substituted.
///
/// ```compile_fail
/// use gameplay::mining::FrozenMiningFrame;
/// fn mix_identity(frame: FrozenMiningFrame) {
///     let _ = FrozenMiningFrame { session_generation: frame.fifo_sequence, ..frame };
/// }
/// ```
///
/// ```compile_fail
/// use gameplay::mining::FrozenMiningFrame;
/// fn mix_identity(frame: FrozenMiningFrame) {
///     let _ = FrozenMiningFrame { position_authority_generation: frame.session_generation, ..frame };
/// }
/// ```
///
/// ```compile_fail
/// use gameplay::mining::FrozenMiningFrame;
/// fn mix_identity(frame: FrozenMiningFrame) {
///     let _ = FrozenMiningFrame { input_authority_generation: frame.input_frame_sequence, ..frame };
/// }
/// ```
///
/// ```compile_fail
/// use gameplay::mining::FrozenMiningFrame;
/// fn mix_identity(frame: FrozenMiningFrame) {
///     let _ = FrozenMiningFrame { input_frame_sequence: frame.fifo_sequence, ..frame };
/// }
/// ```
///
/// ```compile_fail
/// use gameplay::mining::FrozenMiningFrame;
/// fn mix_identity(frame: FrozenMiningFrame) {
///     let _ = FrozenMiningFrame { fifo_sequence: frame.physics_tick, ..frame };
/// }
/// ```
///
/// ```compile_fail
/// use gameplay::mining::FrozenMiningFrame;
/// fn mix_identity(frame: FrozenMiningFrame) {
///     let _ = FrozenMiningFrame { physics_tick: frame.pose_generation, ..frame };
/// }
/// ```
///
/// ```compile_fail
/// use gameplay::mining::FrozenMiningFrame;
/// fn mix_identity(frame: FrozenMiningFrame) {
///     let _ = FrozenMiningFrame { pose_generation: frame.session_generation, ..frame };
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrozenMiningFrame {
    pub session_generation: SessionGeneration,
    pub position_authority_generation: PositionAuthorityGeneration,
    pub input_authority_generation: InputAuthorityGeneration,
    pub input_frame_sequence: InputSequence,
    pub fifo_sequence: FifoSequence,
    pub physics_tick: PhysicsTick,
    pub pose_generation: PoseGeneration,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FrozenMiningRay {
    pub origin: [f32; 3],
    pub direction: [f32; 3],
    pub movement_world_identity: WorldCollisionIdentity,
    pub world_identity: WorldCollisionIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrozenMiningSelection {
    pub slot: u8,
    pub item: VerifiedNetworkItemStack,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FrozenMiningTarget {
    pub position: [i32; 3],
    pub face: u8,
    pub relative_hit: [f32; 3],
    /// Unclamped world-space intercept used for actor occlusion.
    pub hit_world: [f32; 3],
    pub runtime_id: u32,
    pub identity: WorldCollisionIdentity,
}

/// One tick's committed block actions, item interaction and mining request.
#[derive(Debug, Clone, PartialEq)]
pub struct QueuedMiningInteraction {
    pub creative_destroy: Option<crate::survival_mining::CreativeDestroyBlock>,
    pub interactions: PlayerAuthInputInteractions,
    pub mining_request: Option<protocol::MineBlockRequest>,
}

pub const fn protocol_input_mode(input_mode: InputMode) -> PlayerInputMode {
    match input_mode {
        InputMode::KeyboardMouse => PlayerInputMode::Mouse,
        InputMode::GamePad => PlayerInputMode::GamePad,
        InputMode::Touch => PlayerInputMode::Touch,
    }
}

pub const fn survival_reach(input_mode: PlayerInputMode) -> f64 {
    match input_mode {
        PlayerInputMode::Touch => SURVIVAL_TOUCH_REACH_BLOCKS,
        PlayerInputMode::Mouse | PlayerInputMode::GamePad => creative_reach(input_mode),
    }
}

pub const fn creative_reach(input_mode: PlayerInputMode) -> f64 {
    match input_mode {
        PlayerInputMode::Mouse => CREATIVE_MOUSE_REACH_BLOCKS,
        PlayerInputMode::GamePad => CREATIVE_GAMEPAD_REACH_BLOCKS,
        PlayerInputMode::Touch => CREATIVE_TOUCH_REACH_BLOCKS,
    }
}

#[cfg(test)]
mod reach_tests {
    use super::*;

    #[test]
    fn pick_ranges_preserve_single_precision_limits() {
        for (input, creative, survival) in [
            (PlayerInputMode::Mouse, 5.7_f32, 5.7_f32),
            (PlayerInputMode::GamePad, 5.6_f32, 5.6_f32),
            (PlayerInputMode::Touch, 12.0_f32, 6.7_f32),
        ] {
            assert_eq!(creative_reach(input), f64::from(creative));
            assert_eq!(survival_reach(input), f64::from(survival));
        }
    }
}
