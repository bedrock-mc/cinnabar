//! Read-only native body, arm and item-parent poses retained for unchanged captured frames.
use super::*;
use std::borrow::Cow;

type RenderPose = Arc<[render_model::RenderBoneTransform]>;

struct NativePoseInput {
    consume: Option<u32>,
    animation: Option<client_world::AttachableAnimationInput<'static>>,
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
    key: Option<PoseKey>,
    pose: Option<[RenderPose; 2]>,
    skin_layers: Option<Arc<[client_world::SkinRenderLayer]>>,
    #[cfg(test)]
    pub(super) sample_work: (u64, u64),
}

impl NativePoseCache {
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
    pub(in super::super) fn skin_layers(&self) -> Option<&[client_world::SkinRenderLayer]> {
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

    /// Samples native parent bones without requesting unused first-person persona layers.
    pub(super) fn sample(
        &mut self,
        stream: &WorldStream,
        presentation: &ActorRigPresentation,
        consume: Option<u32>,
        animation: Option<client_world::AttachableAnimationInput<'static>>,
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
        let allocated = crate::test_allocations::count();
        let mut frame = stream.authority().actor_render_frame(alpha);
        self.skin_layers = None;
        let sampled = if sample_skin {
            frame.layers_with_skin(runtime_id)
        } else {
            frame
                .layers(runtime_id)
                .map(|render| client_world::ActorRenderLayers {
                    render,
                    skin: Cow::Borrowed(&[]),
                })
        };
        let pose = sampled.and_then(|sampled| {
            let Cow::Owned(layers) = sampled.render else {
                return None;
            };
            let layer = layers.iter().find(|layer| {
                layer.geometry.is_none()
                    && layer.pose.len() == presentation.submission.input.current_bones.len()
                    && layer.previous_pose.len()
                        == presentation.submission.input.previous_bones.len()
                    && !layer.pose.is_empty()
            })?;
            let current = crate::presentation::actors::convert_bones(&layer.pose)?;
            let previous = if Arc::ptr_eq(&layer.previous_pose, &layer.pose) {
                Arc::clone(&current)
            } else {
                crate::presentation::actors::convert_bones(&layer.previous_pose)?
            };
            if let Cow::Owned(skin) = sampled.skin {
                self.skin_layers = Some(skin.into());
            }
            Some([previous, current])
        });
        #[cfg(test)]
        {
            self.sample_work.0 += 1;
            self.sample_work.1 += crate::test_allocations::count() - allocated;
        }
        self.key = Some(key);
        self.pose = pose.clone();
        pose
    }
}
