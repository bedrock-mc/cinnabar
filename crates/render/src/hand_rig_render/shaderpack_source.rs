//! The first-person rig as a shader-pack [`RigSource`]: the hand pass's uploaded buffers,
//! with each instance's skin or held-item atlas layer, the hand's light and its field of view.

use super::*;
use crate::shaderpack::actors::{Hand, LayerTexture, RigBuffers, RigInstance, RigSource};

/// The uploaded first-person rig, borrowed for one frame.
pub(crate) struct ShaderpackHand<'a> {
    gpu: &'a HandRigGpu,
    frame: &'a HandRigFrame,
    buffers: RigBuffers<'a>,
}

/// The first-person rig when this frame's pose and geometry are uploaded.
pub(crate) fn shaderpack_hand(world: &World) -> Option<ShaderpackHand<'_>> {
    let gpu = world.get_resource::<HandRigGpu>()?;
    let frame = world.get_resource::<HandRigScene>()?.frame.as_ref()?;
    if gpu.revision != Some(frame.revision) || gpu.instance_count == 0 {
        return None;
    }
    let buffers = RigBuffers {
        instances: gpu.instances.as_ref()?,
        vertices: gpu.vertices.buffer()?,
        spans: gpu.spans.as_ref()?,
        previous_bones: gpu.previous_bones.as_ref()?,
        current_bones: gpu.current_bones.as_ref()?,
    };
    Some(ShaderpackHand {
        gpu,
        frame,
        buffers,
    })
}

impl RigSource for ShaderpackHand<'_> {
    fn buffers(&self) -> RigBuffers<'_> {
        self.buffers
    }

    fn for_each_instance(&self, visit: &mut dyn FnMut(RigInstance<'_>)) {
        let light = [
            self.frame.light.block_level.min(15) as u8,
            self.frame.light.sky_level.min(15) as u8,
        ];
        for (index, data) in self.frame.rig.instances.iter().enumerate() {
            let Some(span) = self.frame.rig.geometry_spans.get(data.geometry_id as usize) else {
                continue;
            };
            let flags = data.texture_layer;
            let item = flags & HAND_ITEM_LAYER_FLAG != 0;
            let hand = if flags & HAND_OFFHAND_LAYER_FLAG != 0 {
                Hand::Off
            } else {
                Hand::Main
            };
            let held = item.then_some(hand);
            let texture = match held {
                Some(Hand::Main) => raw(self.gpu.atlases[0]
                    .as_ref()
                    .map(|a| (&a._texture, a.raw_views))),
                Some(Hand::Off) => raw(self.gpu.atlases[1]
                    .as_ref()
                    .map(|a| (&a._texture, a.raw_views))),
                None => raw(self
                    .gpu
                    .skin
                    .as_ref()
                    .map(|skin| (&skin._texture, skin.raw_views))),
            };
            let Some(texture) = texture else {
                continue;
            };
            visit(RigInstance {
                index: index as u32,
                data,
                vertex_count: span.vertex_count,
                texture: LayerTexture {
                    texture,
                    layer: flags & HAND_TEXTURE_LAYER_MASK,
                },
                runtime_id: None,
                camera: true,
                light,
                blend: Some(item && flags & HAND_BLEND_LAYER_FLAG != 0),
                held,
            });
        }
    }

    fn glint(&self, item: bool) -> Option<&Texture> {
        item.then_some(())
            .and(self.gpu.glint.as_ref())
            .and_then(|glint| raw(Some((&glint._texture, glint.raw_views))))
    }
}

/// A texture the pack can read through a raw gamma-byte view.
fn raw(texture: Option<(&Texture, crate::actor::gpu::ArtworkRawViews)>) -> Option<&Texture> {
    texture.and_then(|(texture, raw_views)| raw_views.0.then_some(texture))
}
