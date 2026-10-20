//! Immutable per-clip unions; live clip selection still controls the frame union.
use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ClipMasks {
    geometry: Option<u32>,
    clips: Box<[ClipMask]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ClipMask {
    clip: usize,
    bones: Box<[(usize, [u8; 3])]>,
}

impl ClipMasks {
    /// Counts each boxed record and its sparse bone payload against the shared plan budget.
    pub(super) fn retained_bytes(&self) -> Option<usize> {
        let Self { geometry: _, clips } = self;
        clips.iter().try_fold(
            std::mem::size_of::<Self>().checked_add(std::mem::size_of_val(clips.as_ref()))?,
            |bytes, ClipMask { clip: _, bones }| {
                bytes.checked_add(std::mem::size_of_val(bones.as_ref()))
            },
        )
    }
}

impl MotionSampling {
    /// Compiles reachable clips for the selected skeleton and every alternate render geometry.
    pub(super) fn bind_masks(
        &self,
        assets: &RuntimeEntityAssets,
        rig: usize,
        geometry: usize,
        controllers: &[ControllerState],
        extra_clips: &[usize],
    ) -> Box<[ClipMasks]> {
        let (_, mut clips) =
            super::super::sampling::topology_expressions(assets, geometry, controllers);
        clips.extend(extra_clips.iter().copied());
        let mut geometries = BTreeSet::new();
        for layer in assets.render_layers(rig) {
            let first = layer.first_geometry as usize;
            geometries.extend(
                assets.render_data().geometries[first..first + usize::from(layer.geometry_count)]
                    .iter()
                    .map(|choice| Some(choice.geometry)),
            );
        }
        geometries.insert(None);
        geometries
            .into_iter()
            .filter_map(|selected| {
                let source = selected.unwrap_or(assets.rig_geometries().get(geometry)?.geometry);
                let (_, names) = resolve_bones(assets, source as usize)?;
                let mapped: BTreeSet<_> = clips
                    .iter()
                    .filter_map(|&clip| match selected {
                        Some(geometry) => assets
                            .clip_for_geometry(assets.animation_clips().get(clip)?.symbol, geometry)
                            .map(|clip| clip as usize),
                        None => Some(clip),
                    })
                    .collect();
                Some(ClipMasks {
                    geometry: selected,
                    clips: mapped
                        .into_iter()
                        .map(|clip| {
                            let mask = self.mask_unbound(assets, &names, [weighted(clip)], None);
                            ClipMask {
                                clip,
                                bones: mask
                                    .into_iter()
                                    .enumerate()
                                    .filter(|(_, bits)| *bits != [0; 3])
                                    .collect(),
                            }
                        })
                        .collect(),
                })
            })
            .collect()
    }

    /// Unions prepared masks for the live clips, retaining the old path for unbound topology.
    pub(crate) fn mask(
        &self,
        assets: &RuntimeEntityAssets,
        names: &[Box<str>],
        clips: impl IntoIterator<Item = tick::WeightedClip>,
        geometry: Option<u32>,
    ) -> Vec<[u8; 3]> {
        let mut mask = vec![[0; 3]; names.len()];
        let prepared = self.masks.iter().find(|masks| masks.geometry == geometry);
        for clip in clips {
            let Some(mapped) = (match geometry {
                Some(geometry) => render::clip_for_layer(assets, clip, geometry),
                None => Some(clip),
            }) else {
                continue;
            };
            if let Some(bits) = prepared.and_then(|prepared| {
                prepared
                    .clips
                    .binary_search_by_key(&mapped.clip, |mask| mask.clip)
                    .ok()
                    .map(|slot| &prepared.clips[slot].bones)
            }) {
                for &(bone, bits) in bits {
                    if let Some(target) = mask.get_mut(bone) {
                        for axis in 0..3 {
                            target[axis] |= bits[axis];
                        }
                    }
                }
            } else {
                for (target, bits) in
                    mask.iter_mut()
                        .zip(self.mask_unbound(assets, names, [mapped], None))
                {
                    for axis in 0..3 {
                        target[axis] |= bits[axis];
                    }
                }
            }
        }
        mask
    }
}

/// Supplies only the clip ID to the static mask analysis, which ignores live clock and weight.
fn weighted(clip: usize) -> tick::WeightedClip {
    tick::WeightedClip {
        clip,
        weight: 1.0,
        started_tick: 0,
        clock: clock::Basis::Direct,
        time: 0.0,
        blend: None,
    }
}

#[cfg(test)]
mod tests;
