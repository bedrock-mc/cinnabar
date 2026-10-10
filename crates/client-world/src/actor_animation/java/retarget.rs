//! Render-time retargeting: a rig's pose with model-space targets replacing some joints, every
//! other bone keeping its animated transform relative to its parent.

use std::{collections::HashMap, sync::Arc};

use super::super::{
    ActorAnimationStore, BoneTransform, RuntimeBone, SkinRenderLayer,
    pose::{quat_multiply, rotate_vector, total_scale, with_scale},
};

/// Frames a rig's retained tick transforms survive without being retargeted.
const RETENTION_FRAMES: u64 = 2;

/// Slot of a rig's own pose in [`JavaRetargetCache`]; skin layers follow it by index.
const BODY_SLOT: usize = 0;

/// Each bone's transform relative to its parent in one tick's two poses. Every frame of the
/// tick blends these, so they are kept while the parents and both poses stay bit-identical.
#[derive(Debug, Default)]
struct TickLocals {
    parents: Vec<Option<usize>>,
    previous: Vec<BoneTransform>,
    current: Vec<BoneTransform>,
    locals: Vec<Local>,
    #[cfg(test)]
    rebuilds: u64,
}

#[derive(Clone, Copy, Debug)]
struct Local {
    /// The previous and current transforms relative to the parent, or of a root itself; `None`
    /// when the parent lies outside the pose, which retargeting then indexes directly.
    sides: Option<[BoneTransform; 2]>,
    /// The blend of bit-identical sides. It is the same for every finite frame fraction whose
    /// sign is positive, because each lerp then adds a positive zero.
    settled: Option<BoneTransform>,
}

impl TickLocals {
    /// Recomputes the locals unless `bones`' parents and both poses match the retained inputs
    /// bit for bit. Both poses must have one transform per bone.
    fn refresh(
        &mut self,
        bones: &[RuntimeBone],
        previous: &[BoneTransform],
        current: &[BoneTransform],
    ) {
        if self.parents.len() == bones.len()
            && self
                .parents
                .iter()
                .zip(bones)
                .all(|(parent, bone)| *parent == bone.parent)
            && same_bits(&self.previous, previous)
            && same_bits(&self.current, current)
        {
            return;
        }
        #[cfg(test)]
        {
            self.rebuilds += 1;
        }
        self.parents.clear();
        self.parents.extend(bones.iter().map(|bone| bone.parent));
        self.previous.clear();
        self.previous.extend_from_slice(previous);
        self.current.clear();
        self.current.extend_from_slice(current);
        self.locals.clear();
        self.locals
            .extend(bones.iter().enumerate().map(|(index, bone)| {
                let sides = match bone.parent {
                    None => Some([previous[index], current[index]]),
                    Some(parent) if parent < bones.len() => Some([
                        relative(previous[parent], previous[index]),
                        relative(current[parent], current[index]),
                    ]),
                    Some(_) => None,
                };
                let settled = sides
                    .filter(|[from, to]| bits(from) == bits(to))
                    .map(|[from, to]| blend(from, to, 0.0));
                Local { sides, settled }
            }));
    }
}

/// Bit patterns of every component, so `-0.0` and NaN payloads compare exactly.
fn bits(bone: &BoneTransform) -> [u32; 11] {
    let [r0, r1, r2, r3] = bone.rotation.map(f32::to_bits);
    let [t0, t1, t2, t3] = bone.translation_scale.map(f32::to_bits);
    let [s0, s1, s2] = bone.axis_scale.map(f32::to_bits);
    [r0, r1, r2, r3, t0, t1, t2, t3, s0, s1, s2]
}

fn same_bits(left: &[BoneTransform], right: &[BoneTransform]) -> bool {
    left.len() == right.len() && left.iter().zip(right).all(|(a, b)| bits(a) == bits(b))
}

/// Retargeting scratch and each rig's tick-constant transforms, kept across frames so a frame
/// between ticks only blends and composes.
#[derive(Debug, Default)]
pub struct JavaRetargetCache {
    /// By runtime id and slot: the rig's own pose, then its skin layers.
    rigs: HashMap<(u64, usize), (TickLocals, u64)>,
    posed: Vec<Option<BoneTransform>>,
    output: Vec<BoneTransform>,
    targets: Vec<Option<BoneTransform>>,
    frame: u64,
}

impl JavaRetargetCache {
    /// Starts a frame, releasing rigs no recent frame retargeted.
    pub fn begin_frame(&mut self) {
        self.frame += 1;
        let oldest = self.frame.saturating_sub(RETENTION_FRAMES);
        self.rigs.retain(|_, (_, used)| *used >= oldest);
    }

    /// [`retarget`] through the retained locals of `key`, borrowing the result.
    fn retarget(
        &mut self,
        key: (u64, usize),
        bones: &[RuntimeBone],
        [previous, current]: [&[BoneTransform]; 2],
        alpha: f32,
        targets: &[Option<BoneTransform>],
    ) -> Option<&[BoneTransform]> {
        if previous.len() != bones.len() || current.len() != bones.len() {
            return None;
        }
        let frame = self.frame;
        let (locals, used) = self.rigs.entry(key).or_default();
        *used = frame;
        locals.refresh(bones, previous, current);
        retarget_with(
            bones,
            [previous, current],
            alpha,
            targets,
            &locals.locals,
            &mut self.posed,
            &mut self.output,
        )?;
        Some(&self.output)
    }
}

/// The pose at `alpha` with `targets` (model-space bones by index) replacing their joints;
/// every other bone keeps its animated transform relative to its parent.
pub(in crate::actor_animation) fn retarget(
    bones: &[RuntimeBone],
    previous: &[BoneTransform],
    current: &[BoneTransform],
    alpha: f32,
    targets: &[Option<BoneTransform>],
) -> Option<Vec<BoneTransform>> {
    if previous.len() != bones.len() || current.len() != bones.len() {
        return None;
    }
    let mut locals = TickLocals::default();
    locals.refresh(bones, previous, current);
    let mut output = Vec::with_capacity(bones.len());
    retarget_with(
        bones,
        [previous, current],
        alpha,
        targets,
        &locals.locals,
        &mut Vec::new(),
        &mut output,
    )?;
    Some(output)
}

/// Retargets every bone into `output`, reusing `posed` as the per-bone memo.
fn retarget_with(
    bones: &[RuntimeBone],
    poses: [&[BoneTransform]; 2],
    alpha: f32,
    targets: &[Option<BoneTransform>],
    locals: &[Local],
    posed: &mut Vec<Option<BoneTransform>>,
    output: &mut Vec<BoneTransform>,
) -> Option<()> {
    posed.clear();
    posed.resize(bones.len(), None);
    let frame = Frame {
        bones,
        poses,
        alpha,
        settled: alpha.is_finite() && alpha.is_sign_positive(),
        targets,
        locals,
    };
    for index in 0..bones.len() {
        frame.bone(index, posed, 0)?;
    }
    output.clear();
    for bone in posed.iter() {
        output.push((*bone)?);
    }
    Some(())
}

/// One retargeting pass's inputs.
struct Frame<'a> {
    bones: &'a [RuntimeBone],
    poses: [&'a [BoneTransform]; 2],
    alpha: f32,
    /// Whether settled blends hold at `alpha`.
    settled: bool,
    targets: &'a [Option<BoneTransform>],
    locals: &'a [Local],
}

impl Frame<'_> {
    /// Model-space transform of bone `index`, posing its ancestors first.
    fn bone(
        &self,
        index: usize,
        posed: &mut [Option<BoneTransform>],
        depth: usize,
    ) -> Option<BoneTransform> {
        if let Some(done) = posed[index] {
            return Some(done);
        }
        if depth > self.bones.len() {
            return None;
        }
        let bone = match (
            self.targets.get(index).copied().flatten(),
            self.bones[index].parent,
        ) {
            (Some(target), _) => target,
            (None, None) => self.local(index, None),
            (None, Some(parent)) => {
                let local = self.local(index, Some(parent));
                let parent = self.bone(parent, posed, depth + 1)?;
                compose(parent, local)
            }
        };
        posed[index] = Some(bone);
        Some(bone)
    }

    /// Bone `index` relative to `parent` (or itself as a root) at the frame's fraction.
    fn local(&self, index: usize, parent: Option<usize>) -> BoneTransform {
        let local = self.locals[index];
        if self.settled
            && let Some(settled) = local.settled
        {
            return settled;
        }
        let [previous, current] = self.poses;
        match (local.sides, parent) {
            (Some([from, to]), _) => blend(from, to, self.alpha),
            (None, None) => blend(previous[index], current[index], self.alpha),
            (None, Some(parent)) => blend(
                relative(previous[parent], previous[index]),
                relative(current[parent], current[index]),
                self.alpha,
            ),
        }
    }
}

impl ActorAnimationStore {
    /// The rig's pose at `alpha` with `targets` replacing their joints in model space.
    pub(crate) fn retargeted_pose(
        &self,
        runtime_id: u64,
        alpha: f32,
        targets: &[Option<BoneTransform>],
    ) -> Option<Vec<BoneTransform>> {
        let state = self.rigs.get(self.runtime_to_lifetime.get(&runtime_id)?)?;
        retarget(
            state.posed_bones(),
            &state.previous,
            &state.current,
            alpha.clamp(0.0, 1.0),
            targets,
        )
    }

    /// [`Self::retargeted_pose`] reusing the rig's tick transforms retained in `cache`.
    pub(crate) fn retargeted_pose_cached<'c>(
        &self,
        runtime_id: u64,
        alpha: f32,
        targets: &[Option<BoneTransform>],
        cache: &'c mut JavaRetargetCache,
    ) -> Option<&'c [BoneTransform]> {
        let state = self.rigs.get(self.runtime_to_lifetime.get(&runtime_id)?)?;
        cache.retarget(
            (runtime_id, BODY_SLOT),
            state.posed_bones(),
            [&state.previous, &state.current],
            alpha.clamp(0.0, 1.0),
            targets,
        )
    }

    /// The animated skin layers at `alpha`, each retargeted by the targets `targets` writes
    /// from its skeleton's bone names and rest pose, reusing the tick transforms in `cache`.
    pub(crate) fn retargeted_layers(
        &self,
        runtime_id: u64,
        alpha: f32,
        mut targets: impl FnMut(
            &[Box<str>],
            &[BoneTransform],
            &mut Vec<Option<BoneTransform>>,
        ) -> Option<()>,
        cache: &mut JavaRetargetCache,
    ) -> Option<Vec<SkinRenderLayer>> {
        let state = self.rigs.get(self.runtime_to_lifetime.get(&runtime_id)?)?;
        let skeletons = state
            .skin_skeleton()
            .map_or(&[][..], |skin| &skin.prepared.layers);
        let mut layers = Vec::with_capacity(state.skin_layers.len());
        for (index, layer) in state.skin_layers.iter().enumerate() {
            let skeleton = skeletons.iter().find(|skeleton| skeleton.poses(layer))?;
            let mut layer_targets = std::mem::take(&mut cache.targets);
            let written = targets(&skeleton.names, &skeleton.rest, &mut layer_targets);
            let pose = written.and_then(|()| {
                cache
                    .retarget(
                        (runtime_id, BODY_SLOT + 1 + index),
                        &skeleton.bones,
                        [&layer.previous, &layer.current],
                        alpha.clamp(0.0, 1.0),
                        &layer_targets,
                    )
                    .map(Arc::<[BoneTransform]>::from)
            });
            cache.targets = layer_targets;
            let pose = pose?;
            layers.push(SkinRenderLayer {
                previous: Arc::clone(&pose),
                current: pose,
                ..layer.clone()
            });
        }
        Some(layers)
    }
}

/// Reads the translation component without including the packed scale.
pub(super) fn translation(bone: BoneTransform) -> [f32; 3] {
    [
        bone.translation_scale[0],
        bone.translation_scale[1],
        bone.translation_scale[2],
    ]
}

/// `child` in `parent`'s frame, matching the pose composer's scale handling.
fn relative(parent: BoneTransform, child: BoneTransform) -> BoneTransform {
    let inverse = conjugate(parent.rotation);
    // A parent hidden by a zero scale leaves its children's offsets unscaled.
    let parent_scale = total_scale(&parent).map(|scale| {
        if scale.abs() > f32::EPSILON {
            scale
        } else {
            1.0
        }
    });
    let offset: [f32; 3] =
        std::array::from_fn(|axis| translation(child)[axis] - translation(parent)[axis]);
    let local = rotate_vector(inverse, offset);
    let child_scale = total_scale(&child);
    with_scale(
        quat_multiply(inverse, child.rotation),
        std::array::from_fn(|axis| local[axis] / parent_scale[axis]),
        std::array::from_fn(|axis| child_scale[axis] / parent_scale[axis]),
    )
}

/// Places a local transform in its parent frame, including nonuniform scale.
fn compose(parent: BoneTransform, local: BoneTransform) -> BoneTransform {
    let parent_scale = total_scale(&parent);
    let scaled = std::array::from_fn(|axis| translation(local)[axis] * parent_scale[axis]);
    let offset = rotate_vector(parent.rotation, scaled);
    let local_scale = total_scale(&local);
    with_scale(
        quat_multiply(parent.rotation, local.rotation),
        std::array::from_fn(|axis| translation(parent)[axis] + offset[axis]),
        std::array::from_fn(|axis| parent_scale[axis] * local_scale[axis]),
    )
}

/// Inverts a unit rotation quaternion.
fn conjugate([x, y, z, w]: [f32; 4]) -> [f32; 4] {
    [-x, -y, -z, w]
}

/// Interpolates translation and scale with a normalized shortest-path rotation.
fn blend(from: BoneTransform, to: BoneTransform, alpha: f32) -> BoneTransform {
    let lerp = |a: f32, b: f32| a + (b - a) * alpha;
    let mut end = to.rotation;
    let dot: f32 = (0..4).map(|i| from.rotation[i] * end[i]).sum();
    if dot < 0.0 {
        end = end.map(|value| -value);
    }
    let mixed: [f32; 4] = std::array::from_fn(|i| lerp(from.rotation[i], end[i]));
    let length = mixed.iter().map(|value| value * value).sum::<f32>().sqrt();
    let rotation = if length > f32::EPSILON {
        mixed.map(|value| value / length)
    } else {
        to.rotation
    };
    let (from_scale, to_scale) = (total_scale(&from), total_scale(&to));
    with_scale(
        rotation,
        std::array::from_fn(|axis| lerp(translation(from)[axis], translation(to)[axis])),
        std::array::from_fn(|axis| lerp(from_scale[axis], to_scale[axis])),
    )
}

#[cfg(test)]
mod tests;
