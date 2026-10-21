//! Read-only native body, arm and item-parent poses retained for unchanged captured frames.
use super::*;
use std::borrow::Cow;

type RenderPose = Arc<[render_model::RenderBoneTransform]>;

struct NativeBodyPose {
    bones: [RenderPose; 2],
    skin: Option<Arc<[actor_animation::SkinRenderLayer]>>,
}

struct NativePoseInput {
    consume: Option<u32>,
    animation: Option<actor_animation::AttachableAnimationInput<'static>>,
    alpha: f32,
    camera: Option<([f32; 2], [f32; 3])>,
    sample_skin: bool,
}

#[derive(PartialEq)]
struct PoseKey {
    source: Option<SourceKey>,
    tick: (u32, u64, u64),
    actor_alpha: u32,
    physics_alpha: Option<u32>,
    camera: Option<([f32; 2], [f32; 3])>,
    sample_skin: bool,
}

/// Keeps one lifetime's sampled pose instead of resampling unchanged captured inputs.
#[derive(Default)]
pub(in super::super) struct NativePoseCache {
    rest: Option<(
        (
            actor_animation::ActorLifetimeId,
            actor_animation::EntityRigId,
            u64,
        ),
        RenderPose,
    )>,
    key: Option<PoseKey>,
    pose: Option<[RenderPose; 2]>,
    skin_layers: Option<Arc<[actor_animation::SkinRenderLayer]>>,
    pub(in super::super) held_item_scale: Option<f32>,
    #[cfg(test)]
    pub(super) sample_work: (u64, u64),
}

impl NativePoseCache {
    /// Converts immutable rest geometry once for the bare-arm camera stack.
    pub(super) fn rest_pose(
        &mut self,
        rig: &actor_animation::ActorRigSnapshot<'_>,
    ) -> Option<RenderPose> {
        let key = (rig.actor, rig.rig, rig.rest_reset_generation);
        if self
            .rest
            .as_ref()
            .is_none_or(|(retained, _)| *retained != key)
        {
            self.rest = Some((key, crate::presentation::actors::convert_bones(rig.rest)?));
        }
        self.rest.as_ref().map(|(_, pose)| Arc::clone(pose))
    }

    /// Applies the sampled parent only when a native arm or attachable consumes its bones.
    pub(in super::super) fn apply(&mut self, inputs: &mut HandInputs<'_>) {
        if let Some([previous, current]) = self.sample(
            inputs.stream,
            &inputs.presentation,
            inputs.consume_ticks,
            inputs.item_animation,
            inputs.alpha,
            inputs.sampling_camera,
        ) {
            inputs.presentation.submission.input.previous_bones = previous;
            inputs.presentation.submission.input.current_bones = current;
        }
    }

    /// The body-only sampled persona slice; borrowed completed layers stay on the actor snapshot.
    pub(in super::super) fn skin_layers(&self) -> Option<&[actor_animation::SkinRenderLayer]> {
        self.skin_layers.as_deref()
    }

    /// Shares a body's native sample with its equipment and persona layers.
    pub(in super::super) fn apply_body(
        &mut self,
        stream: &WorldStream,
        presentation: &mut ActorRigPresentation,
        alpha: f32,
        camera: Option<([f32; 2], [f32; 3])>,
    ) {
        if let Some([previous, current]) = self.sample_with_skin(
            stream,
            presentation,
            NativePoseInput {
                consume: None,
                animation: None,
                alpha,
                camera,
                sample_skin: true,
            },
        ) {
            presentation.submission.input.previous_bones = previous;
            presentation.submission.input.current_bones = current;
        }
    }

    /// Publishes a remote body and returns its sampled persona layers from the shared frame.
    pub(in super::super) fn apply_frame_body(
        frame: &mut actor_animation::ActorRenderFrame<'_>,
        presentation: &mut ActorRigPresentation,
    ) -> Option<Arc<[actor_animation::SkinRenderLayer]>> {
        let runtime_id = presentation.submission.input.identity.runtime_id;
        let NativeBodyPose {
            bones: [previous, current],
            skin,
        } = body_pose(presentation, frame.layers_with_skin(runtime_id)?)?;
        presentation.submission.input.previous_bones = previous;
        presentation.submission.input.current_bones = current;
        skin
    }

    /// Samples native parent bones without requesting unused first-person persona layers.
    pub(super) fn sample(
        &mut self,
        stream: &WorldStream,
        presentation: &ActorRigPresentation,
        consume: Option<u32>,
        animation: Option<actor_animation::AttachableAnimationInput<'static>>,
        alpha: f32,
        camera: Option<([f32; 2], [f32; 3])>,
    ) -> Option<[RenderPose; 2]> {
        self.sample_with_skin(
            stream,
            presentation,
            NativePoseInput {
                consume,
                animation,
                alpha,
                camera,
                sample_skin: false,
            },
        )
    }

    /// Retains the exact native consumer's read-only result for unchanged captured inputs.
    fn sample_with_skin(
        &mut self,
        stream: &WorldStream,
        presentation: &ActorRigPresentation,
        input: NativePoseInput,
    ) -> Option<[RenderPose; 2]> {
        let NativePoseInput {
            consume,
            animation,
            alpha,
            camera,
            sample_skin,
        } = input;
        let runtime_id = presentation.submission.input.identity.runtime_id;
        let rig = stream.authority().actor_rig(runtime_id)?;
        let key = PoseKey {
            source: source_key(stream, consume, animation, alpha),
            tick: (rig.rig.0, rig.completed_tick, rig.reset_generation),
            actor_alpha: alpha.to_bits(),
            physics_alpha: rig.java.local_swing_alpha.map(f32::to_bits),
            camera,
            sample_skin,
        };
        if camera.is_some() && self.key.as_ref() == Some(&key) {
            return self.pose.clone();
        }
        #[cfg(test)]
        let allocated = test_alloc::thread_allocations();
        let mut frame = stream.authority().actor_render_frame(alpha);
        self.held_item_scale = frame.sample_held_item_scale(rig).held_item_scale;
        self.skin_layers = None;
        let sampled = if sample_skin {
            frame.layers_with_skin(runtime_id)
        } else {
            frame
                .layers(runtime_id)
                .map(|render| actor_animation::ActorRenderLayers {
                    render,
                    skin: Cow::Borrowed(&[]),
                    held_item_scale: self.held_item_scale,
                })
        };
        let pose = sampled.and_then(|sampled| {
            let pose = body_pose(presentation, sampled)?;
            self.skin_layers = pose.skin;
            Some(pose.bones)
        });
        #[cfg(test)]
        {
            self.sample_work.0 += 1;
            self.sample_work.1 += test_alloc::thread_allocations() - allocated;
        }
        self.key = Some(key);
        self.pose = pose.clone();
        pose
    }
}

/// Converts sampled native endpoints only when they match the runtime body's skeleton.
fn body_pose(
    presentation: &ActorRigPresentation,
    sampled: actor_animation::ActorRenderLayers<'_>,
) -> Option<NativeBodyPose> {
    let Cow::Owned(layers) = sampled.render else {
        return None;
    };
    let layer = layers.iter().find(|layer| {
        layer.geometry.is_none()
            && layer.pose.len() == presentation.submission.input.current_bones.len()
            && layer.previous_pose.len() == presentation.submission.input.previous_bones.len()
            && !layer.pose.is_empty()
    })?;
    let current = crate::presentation::actors::convert_bones(&layer.pose)?;
    let previous = if Arc::ptr_eq(&layer.previous_pose, &layer.pose) {
        Arc::clone(&current)
    } else {
        crate::presentation::actors::convert_bones(&layer.previous_pose)?
    };
    let skin = match sampled.skin {
        Cow::Owned(skin) => Some(skin.into()),
        Cow::Borrowed(_) => None,
    };
    Some(NativeBodyPose {
        bones: [previous, current],
        skin,
    })
}
