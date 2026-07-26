use protocol::PlayerInputMode;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct PhysicsTickSampleEvidence {
    pub(super) session_generation: u64,
    pub(super) tick: u64,
    pub(super) network_position: [f32; 3],
    pub(super) input_mode: PlayerInputMode,
    pub(super) movement: [f32; 2],
    pub(super) jump_held: bool,
    pub(super) grounded_before_tick: bool,
    pub(super) grounded_after_tick: bool,
    pub(super) jump_started: bool,
    pub(super) jump_repeated: bool,
    pub(super) jump_released: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PhysicsTickEvidenceContext {
    pub(crate) fifo_sequence: u64,
    pub(crate) pose_generation: u64,
    pub(crate) dimension: i32,
    pub(crate) perspective: semantic_input::PerspectiveMode,
    pub(crate) camera_blocked: bool,
    pub(crate) camera_fallback: bool,
    pub(crate) local_avatar_visible: bool,
    pub(crate) look_delta: [f32; 2],
    pub(crate) outbound_authorized: bool,
    pub(crate) outbox_depth: usize,
    pub(crate) outbox_drops: u64,
    pub(crate) free_camera_packet_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PhysicsTickEvidence {
    pub(crate) session_generation: u64,
    pub(crate) tick: u64,
    pub(crate) network_position: [f32; 3],
    pub(crate) input_mode: PlayerInputMode,
    pub(crate) movement: [f32; 2],
    pub(crate) jump_held: bool,
    pub(crate) grounded_before_tick: bool,
    pub(crate) grounded_after_tick: bool,
    pub(crate) jump_started: bool,
    pub(crate) jump_repeated: bool,
    pub(crate) jump_released: bool,
    pub(crate) context: PhysicsTickEvidenceContext,
}
