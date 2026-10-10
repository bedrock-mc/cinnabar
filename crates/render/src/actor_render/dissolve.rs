//! Routes companion color layers with their late depth mask.
use std::{collections::BTreeSet, sync::Arc};

use crate::actor::{ActorDrawManifestEntry, ActorGpuInstance, ActorRenderIdentity};

/// Matches layers from the same published actor state independently of their draw layer.
fn owner(identity: ActorRenderIdentity) -> ActorRenderIdentity {
    ActorRenderIdentity {
        layer: 0,
        ..identity
    }
}

/// Leaves ordinary dissolve pairs opaque and routes color with an owner's sorted depth mask.
/// Layers retain the same world origin, so stable distance sorting keeps their mask first.
pub(super) fn instances(
    input: &Arc<[ActorGpuInstance]>,
    manifest: &[ActorDrawManifestEntry],
) -> Arc<[ActorGpuInstance]> {
    let late: BTreeSet<_> = input
        .iter()
        .zip(manifest)
        .filter(|(instance, _)| {
            instance.material & assets::EntityRenderMaterialState::KIND_MASK
                == assets::EntityRenderMaterial::DissolveDepth as u32
                && super::phase::sorted(instance.material)
        })
        .map(|(_, entry)| owner(entry.identity))
        .collect();
    let mut output = Arc::clone(input);
    if late.is_empty() {
        return output;
    }
    for (index, (instance, entry)) in input.iter().zip(manifest).enumerate() {
        if instance.material & assets::EntityRenderMaterialState::KIND_MASK
            == assets::EntityRenderMaterial::DissolveColor as u32
            && late.contains(&owner(entry.identity))
        {
            Arc::make_mut(&mut output)[index].material |=
                crate::actor::material::LATE_DISSOLVE_COLOR;
        }
    }
    output
}
