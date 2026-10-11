use bevy::prelude::SystemSet;
/// Main-world queue application boundary. Systems ordered after this set
/// observe spawned/updated/despawned chunk entities after deferred commands
/// and component observers have been applied.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkRenderApplySet;
