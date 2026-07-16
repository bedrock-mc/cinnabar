/// Registry facts needed by pure chunk meshing.
///
/// Air cannot be inferred from the numeric value: protocol 1001 may use a
/// sequential runtime ID or a high-bit block-state network hash. Callers must
/// therefore supply the air value advertised by their active registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockClassifier {
    air_network_id: u32,
}

impl BlockClassifier {
    #[must_use]
    pub const fn new(air_network_id: u32) -> Self {
        Self { air_network_id }
    }

    #[must_use]
    pub const fn air_network_id(self) -> u32 {
        self.air_network_id
    }

    #[must_use]
    pub const fn is_air(self, runtime_id: u32) -> bool {
        runtime_id == self.air_network_id
    }
}
