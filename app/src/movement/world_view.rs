//! Synchronous adapter from the published chunk stream to gameplay queries.

/// Borrows the current stream without creating another world owner.
pub(crate) struct GameplayWorldView<'a>(pub(crate) &'a client_world::WorldStream);

impl gameplay::GameplayWorld for GameplayWorldView<'_> {
    /// Resolves one authoritative stack through the existing stream.
    fn canonical_item_stack(
        &self,
        stack: &protocol::NetworkItemStack,
    ) -> Option<client_world::CanonicalItemStack> {
        self.0.canonical_item_stack(stack)
    }
    /// Borrows one actor by persistent identity.
    fn actor_by_unique_id(&self, unique_id: i64) -> Option<&client_world::ActorSnapshot> {
        self.0.actor_by_unique_id(unique_id)
    }
    /// Borrows one actor by session identity.
    fn actor(&self, runtime_id: u64) -> Option<&client_world::ActorSnapshot> {
        self.0.actor(runtime_id)
    }
    /// Reads the session's local runtime identity.
    fn local_player_runtime_id(&self) -> u64 {
        self.0.local_player_runtime_id()
    }
    /// Reads the session's local persistent identity.
    fn local_player_unique_id(&self) -> i64 {
        self.0.local_player_unique_id()
    }
    /// Reads the current mount seat pose.
    fn local_rider_seat_pose(&self) -> Option<([f32; 3], f32)> {
        self.0.local_rider_seat_pose()
    }
    /// Reads the current network palette encoding.
    fn network_id_mode(&self) -> assets::NetworkIdMode {
        self.0.network_id_mode()
    }
    /// Resolves a wire block ID into the existing store palette.
    fn resolve_block_network_id(&self, network_id: u32) -> u32 {
        self.0.resolve_block_network_id(network_id)
    }
    /// Reads the current store's air ID.
    fn air_block_id(&self) -> u32 {
        self.0.air_block_id()
    }
}
