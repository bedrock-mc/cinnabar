//! The world's actor rig as a shader-pack [`RigSource`]: the prepared instance, geometry and
//! bone buffers the actor pass draws from, with each instance's artwork layer and actor.

use super::*;
use crate::shaderpack::actors::{LayerTexture, RigBuffers, RigInstance, RigSource};

/// The prepared actor rig, borrowed for one frame.
pub(crate) struct ShaderpackActors<'a> {
    gpu: &'a ActorGpu,
    buffers: RigBuffers<'a>,
}

/// The actor rig when it has instances and uploaded geometry.
pub(crate) fn shaderpack_actors(world: &World) -> Option<ShaderpackActors<'_>> {
    let gpu = world.get_resource::<ActorGpu>()?;
    if gpu.instance_count == 0 || !gpu.geometry_ready {
        return None;
    }
    let buffers = RigBuffers {
        instances: &gpu.instance_buffer,
        vertices: gpu.geometry_vertices.buffer()?,
        spans: gpu.geometry_span_buffer.as_ref()?,
        previous_bones: &gpu.previous_bone_buffer,
        current_bones: &gpu.current_bone_buffer,
    };
    Some(ShaderpackActors { gpu, buffers })
}

/// Light levels of a packed actor light word; unlit materials read full light.
fn light_levels(light: u32) -> [u8; 2] {
    if light & render_api::ACTOR_LIGHT_WORLD == 0 {
        return [15, 15];
    }
    [(light & 15) as u8, ((light >> 4) & 15) as u8]
}

impl RigSource for ShaderpackActors<'_> {
    fn buffers(&self) -> RigBuffers<'_> {
        self.buffers
    }

    fn for_each_instance(&self, visit: &mut dyn FnMut(RigInstance<'_>)) {
        let gpu = self.gpu;
        let camera_count = gpu.manifest.len() as u32;
        for span in &gpu.spans {
            if span.page != 0 && !gpu.artwork_current {
                continue;
            }
            let page = (span.page != 0)
                .then(|| gpu.artwork.page(usize::from(span.page) - 1))
                .flatten();
            for index in span.first..span.first + span.count {
                let Some(data) = gpu.instances.get(index as usize) else {
                    continue;
                };
                let layer = data.texture_layer & 0x00ff_ffff;
                let texture = match page {
                    Some(page) => Some(page.texture()),
                    None if span.page == 0 => {
                        gpu.skins.texture((data.texture_layer >> 24) as usize)
                    }
                    None => None,
                };
                let Some(texture) = texture else {
                    continue;
                };
                visit(RigInstance {
                    index,
                    data,
                    vertex_count: span.vertex_count,
                    texture: LayerTexture { texture, layer },
                    runtime_id: gpu
                        .manifest
                        .get(index as usize)
                        .map(|entry| entry.identity.runtime_id),
                    camera: index < camera_count,
                    light: light_levels(data.light),
                    blend: None,
                    held: None,
                });
            }
        }
    }

    fn glint(&self, item: bool) -> Option<&Texture> {
        let glint = if item {
            &self.gpu.artwork.item_glint
        } else {
            &self.gpu.artwork.glint
        };
        glint.texture.as_ref().map(|(texture, _)| texture)
    }
}

#[cfg(test)]
mod tests {
    use super::light_levels;

    #[test]
    fn world_lit_actors_carry_their_levels_and_unlit_ones_read_full_light() {
        let lit = render_model::pack_actor_light(7, 12);
        assert_eq!(light_levels(lit), [7, 12]);
        assert_eq!(
            light_levels(render_model::pack_actor_light_without_lightmap()),
            [15, 15]
        );
    }
}
