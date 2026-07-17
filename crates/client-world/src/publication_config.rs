/// Frozen elapsed-time publication service bounds for the Phase 2 gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicationServiceConfig {
    pub minimum_items_per_second: u32,
    pub minimum_bytes_per_second: u64,
    pub target_items_per_second: u32,
    pub target_bytes_per_second: u64,
    pub maximum_frame_items: usize,
    pub maximum_frame_bytes: u64,
    pub maximum_burst_items: usize,
    pub maximum_burst_bytes: u64,
    pub maximum_zero_byte_operations_per_frame: usize,
}

impl PublicationServiceConfig {
    pub const PHASE2_GATE: Self = Self {
        minimum_items_per_second: 4_096,
        minimum_bytes_per_second: 64 * 1024 * 1024,
        target_items_per_second: 8_192,
        target_bytes_per_second: 128 * 1024 * 1024,
        maximum_frame_items: 512,
        maximum_frame_bytes: 64 * 1024 * 1024,
        maximum_burst_items: 8_192,
        maximum_burst_bytes: 128 * 1024 * 1024,
        maximum_zero_byte_operations_per_frame: 256,
    };
}
