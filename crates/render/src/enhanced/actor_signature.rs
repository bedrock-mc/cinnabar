//! Actor cache keys follow the complete GPU instance layout.

use std::hash::{Hash, Hasher};

/// Hashes every packed shader input except the separately evaluated interpolation fraction.
/// Callers hash bone poses separately, so advancing an unchanged pose retains its cache.
pub(super) fn hash_instance(instance: &render_model::ActorGpuInstance, hash: &mut impl Hasher) {
    let mut input = *instance;
    input.partial_tick = 0.0;
    bytemuck::bytes_of(&input).hash(hash);
}

/// Tracks the complete actor appearance and bone snapshots without frame-clock churn.
pub(super) fn frame_signature(frame: &render_scene::ActorRenderFrame) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    frame.skin_revision.hash(&mut hash);
    frame.artwork.identity().hash(&mut hash);
    frame.instance_pages.hash(&mut hash);
    frame.rig.geometry_revision.hash(&mut hash);
    bytemuck::cast_slice::<_, u8>(&frame.rig.current_bones).hash(&mut hash);
    bytemuck::cast_slice::<_, u8>(&frame.rig.previous_bones).hash(&mut hash);
    for instance in frame.rig.instances.iter() {
        hash_instance(instance, &mut hash);
    }
    hash.finish()
}
