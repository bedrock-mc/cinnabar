//! Synchronous world facts needed by local gameplay decisions.

/// Read-only authoritative facts without exposing chunk publication or transport.
pub trait GameplayWorld {
    /// Resolves the retained identity and properties of one item stack.
    fn canonical_item_stack(
        &self,
        stack: &protocol::NetworkItemStack,
    ) -> Option<client_world::CanonicalItemStack>;
    /// Borrows an actor by its persistent identity.
    fn actor_by_unique_id(&self, unique_id: i64) -> Option<&client_world::ActorSnapshot>;
    /// Borrows an actor by its session runtime identity.
    fn actor(&self, runtime_id: u64) -> Option<&client_world::ActorSnapshot>;
    /// Reads the local actor's runtime identity.
    fn local_player_runtime_id(&self) -> u64;
    /// Reads the local actor's persistent identity.
    fn local_player_unique_id(&self) -> i64;
    /// Reads the local player's resolved mount seat.
    fn local_rider_seat_pose(&self) -> Option<([f32; 3], f32)>;
    /// Reads the network palette's ID encoding mode.
    fn network_id_mode(&self) -> assets::NetworkIdMode;
    /// Resolves one network block ID into the store's palette identity.
    fn resolve_block_network_id(&self, network_id: u32) -> u32;
    /// Reads the store's air identity.
    fn air_block_id(&self) -> u32;
}
