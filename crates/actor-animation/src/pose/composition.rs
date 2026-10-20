//! Parent-first composition for an immutable admitted skeleton.

use super::*;

/// Instructions write original bone indices, so callers retain the geometry's output span.
#[derive(Debug)]
pub(crate) struct Composition {
    steps: Box<[Step]>,
}

#[derive(Debug)]
struct Step {
    output: usize,
    parent: Option<usize>,
    offsets: [[f32; 3]; 3],
    rotation: [f32; 3],
}

// Instruction payload has no nested allocations outside the accounted boxed slice.
const _: () = assert!(!std::mem::needs_drop::<Step>());

impl Composition {
    /// Resolves arbitrary bone order once; rejects missing parents and cycles.
    pub(crate) fn new(bones: &[RuntimeBone]) -> Option<Self> {
        let mut emitted = vec![false; bones.len()];
        let mut steps = Vec::with_capacity(bones.len());
        let mut path = Vec::new();
        for start in 0..bones.len() {
            let mut next = Some(start);
            while let Some(index) = next {
                if *emitted.get(index)? {
                    break;
                }
                if path.contains(&index) {
                    return None;
                }
                path.push(index);
                next = bones[index].parent;
            }
            for index in path.drain(..).rev() {
                let bone = &bones[index];
                let pivots = [
                    bone.pivot,
                    [0.0; 3],
                    [
                        bone.pivot[0],
                        bone.pivot[1] - MODEL_PART_ORIGIN_Y,
                        bone.pivot[2],
                    ],
                ];
                steps.push(Step {
                    output: index,
                    parent: bone.parent,
                    offsets: pivots.map(|pivot| {
                        std::array::from_fn(|axis| {
                            pivot[axis]
                                - bone.parent.map_or(0.0, |parent| bones[parent].pivot[axis])
                        })
                    }),
                    rotation: bone.rotation,
                });
                emitted[index] = true;
            }
        }
        Some(Self {
            steps: steps.into(),
        })
    }

    /// Accounts the instruction allocation; the containing plan accounts this struct.
    pub(crate) fn heap_bytes(&self) -> usize {
        std::mem::size_of_val(self.steps.as_ref())
    }

    /// Writes one skeleton without allocation, including mapped skin drivers and legacy attacks.
    /// `bones` must be the admitted skeleton; only its attachable root modes may change.
    /// A failure leaves partially written output that the caller must discard.
    pub(crate) fn compose_into(
        &self,
        bones: &[RuntimeBone],
        local: impl Fn(usize) -> LocalDelta + Copy,
        output: &mut [BoneTransform],
    ) -> Option<()> {
        self.compose_with_targets_into(bones, local, &[], output)?;
        let mut legacy = false;
        for (index, transform) in output.iter_mut().enumerate() {
            if let Some(delta) = local(index).legacy_swing {
                *transform = super::super::legacy_swing::target(*transform, delta);
                finite(transform)?;
                legacy = true;
            }
        }
        if legacy {
            self.write(bones, local, &[], true, output)?;
        }
        Some(())
    }

    /// Replaces explicit model-space targets and composes their descendants in original order.
    /// As in the recursive composer, the caller first validates an untargeted pose.
    pub(crate) fn compose_with_targets_into(
        &self,
        bones: &[RuntimeBone],
        local: impl Fn(usize) -> LocalDelta,
        targets: &[Option<BoneTransform>],
        output: &mut [BoneTransform],
    ) -> Option<()> {
        self.write(bones, local, targets, false, output)
    }

    /// Runs the program; legacy targets already occupy their final output slots on the second pass.
    fn write(
        &self,
        bones: &[RuntimeBone],
        local: impl Fn(usize) -> LocalDelta,
        targets: &[Option<BoneTransform>],
        legacy: bool,
        output: &mut [BoneTransform],
    ) -> Option<()> {
        if output.len() != self.steps.len() || bones.len() != self.steps.len() {
            return None;
        }
        for step in &self.steps {
            let index = step.output;
            if let Some(target) = targets.get(index).copied().flatten() {
                finite(&target)?;
                output[index] = target;
                continue;
            }
            let delta = local(index);
            if legacy && delta.legacy_swing.is_some() {
                continue;
            }
            let mode = match bones[index].attachable_root {
                AttachableRootFrame::Actor => 0,
                AttachableRootFrame::MatchingOwnerName => 1,
                AttachableRootFrame::BindingExpression => 2,
            };
            let translation = std::array::from_fn(|axis| {
                let offset = if axis == 0 {
                    -delta.translation[axis]
                } else {
                    delta.translation[axis]
                };
                step.offsets[mode][axis] + offset
            });
            let root_rotation = if mode == 1 { [0.0; 3] } else { step.rotation };
            let [x, y, z] = std::array::from_fn(|axis| root_rotation[axis] + delta.rotation[axis]);
            let rotation = quat_from_euler([-x, -y, z]);
            let transform = if let Some(parent) = step.parent {
                let parent = output[parent];
                let parent_scale = total_scale(&parent);
                let scaled = std::array::from_fn(|axis| translation[axis] * parent_scale[axis]);
                let rotated = rotate_vector(parent.rotation, scaled);
                let (rotation, scale) = if delta.rotation_relative_to_entity {
                    (rotation, delta.scale)
                } else {
                    (
                        quat_multiply(parent.rotation, rotation),
                        std::array::from_fn(|axis| parent_scale[axis] * delta.scale[axis]),
                    )
                };
                with_scale(
                    rotation,
                    std::array::from_fn(|axis| parent.translation_scale[axis] + rotated[axis]),
                    scale,
                )
            } else {
                with_scale(rotation, translation, delta.scale)
            };
            finite(&transform)?;
            output[index] = transform;
        }
        Some(())
    }
}

/// Rejects nonfinite composed values and explicit targets at the same boundary as the interpreter.
fn finite(transform: &BoneTransform) -> Option<()> {
    transform
        .rotation
        .iter()
        .chain(&transform.translation_scale)
        .chain(&transform.axis_scale)
        .all(|value| value.is_finite())
        .then_some(())
}

#[cfg(test)]
mod tests;
