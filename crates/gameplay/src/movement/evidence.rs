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
pub struct PhysicsTickEvidenceContext {
    pub fifo_sequence: u64,
    pub pose_generation: u64,
    pub dimension: i32,
    pub perspective: semantic_input::PerspectiveMode,
    pub camera_blocked: bool,
    pub camera_fallback: bool,
    pub local_avatar_visible: bool,
    pub look_delta: [f32; 2],
    pub outbound_authorized: bool,
    pub outbox_depth: usize,
    pub outbox_drops: u64,
    pub free_camera_packet_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhysicsTickEvidence {
    pub session_generation: u64,
    pub tick: u64,
    pub network_position: [f32; 3],
    pub input_mode: PlayerInputMode,
    pub movement: [f32; 2],
    pub jump_held: bool,
    pub grounded_before_tick: bool,
    pub grounded_after_tick: bool,
    pub jump_started: bool,
    pub jump_repeated: bool,
    pub jump_released: bool,
    pub context: PhysicsTickEvidenceContext,
}
