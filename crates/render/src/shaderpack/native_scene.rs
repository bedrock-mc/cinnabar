//! Admits replacement from pack, camera and terrain readiness alone.

use super::*;

/// Uses one replacement decision for view uniforms and the native queue handoff.
pub(super) fn replacement_ready(
    pack: &Arc<PackGpu>,
    cameras: impl Iterator<Item = Entity>,
    geometry: Option<&ShaderpackGeometry>,
    states: &HashMap<Entity, ViewState>,
) -> bool {
    let mut cameras = cameras.peekable();
    geometry.is_some_and(|geometry| {
        geometry.supported() && geometry.origin().is_some() && geometry.all_sections_fit()
    }) && super::super::draws::complete_world(|kind| {
        pack.draws.iter().any(|draw| draw.kind == kind)
    }) && cameras.peek().is_some()
        && cameras.all(|entity| {
            states.get(&entity).is_some_and(|state| {
                Arc::ptr_eq(&state.pack, pack) && state.geometry_targets_ready()
            })
        })
}
