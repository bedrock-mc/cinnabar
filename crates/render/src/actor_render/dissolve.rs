//! Routes companion color layers with their late depth mask.
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    sync::Arc,
};

use crate::actor::gpu::ActorDrawSpan;
use crate::actor::{ActorDrawManifestEntry, ActorGpuInstance, ActorRenderIdentity};

/// Stores sorted draw ranges in flat storage and keeps dependent passes adjacent.
#[derive(Default)]
pub(super) struct SortedDraws {
    pub(super) indices: Vec<usize>,
    pub(super) ranges: Vec<Range<usize>>,
}

impl SortedDraws {
    /// Reuses the previous frame's storage and queues each coupled actor as one item.
    pub(super) fn prepare(&mut self, spans: &[ActorDrawSpan], manifest: &[ActorDrawManifestEntry]) {
        self.indices.clear();
        self.ranges.clear();
        let mut pairs: BTreeMap<_, Vec<usize>> = BTreeMap::new();
        for (index, span) in spans
            .iter()
            .enumerate()
            .filter(|(_, span)| super::phase::sorted(span.material))
        {
            if let Some(actor) = pair_owner(span, manifest) {
                pairs.entry(actor).or_default().push(index);
            }
        }
        for (index, span) in spans
            .iter()
            .enumerate()
            .filter(|(_, span)| super::phase::sorted(span.material))
        {
            let start = self.indices.len();
            if let Some(actor) = pair_owner(span, manifest) {
                let Some(pair) = pairs.remove(&actor) else {
                    continue;
                };
                self.indices.extend(pair);
            } else {
                self.indices.push(index);
            }
            self.ranges.push(start..self.indices.len());
        }
    }
}

/// Identifies sorted depth masks and the companion color layers routed with them.
fn pair_owner(
    span: &ActorDrawSpan,
    manifest: &[ActorDrawManifestEntry],
) -> Option<ActorRenderIdentity> {
    (span.material & assets::EntityRenderMaterialState::KIND_MASK
        == assets::EntityRenderMaterial::DissolveDepth as u32
        || span.material & crate::actor::material::LATE_DISSOLVE_COLOR != 0)
        .then(|| {
            manifest
                .get(span.first as usize)
                .map(|entry| owner(entry.identity))
        })
        .flatten()
}

/// Matches layers from the same published actor state independently of their draw layer.
fn owner(identity: ActorRenderIdentity) -> ActorRenderIdentity {
    ActorRenderIdentity {
        layer: 0,
        ..identity
    }
}

/// Leaves ordinary dissolve pairs opaque and routes color with an owner's sorted depth mask.
/// The sorted draw plan keeps each marked color adjacent to its depth mask.
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
